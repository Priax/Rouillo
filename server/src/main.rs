mod auth;
mod db;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use rand::RngExt;
use shared::{
    config, Board, ClientMessage, GameState, InputKind, LobbyInfo, RoomId, RoomInfo, RoomSettings, ServerMessage,
};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;
use warp::Filter;

type ConnId = u64;
type Token = String;

const GRACE: Duration = Duration::from_secs(120);

const CLIENT_CHAN_CAP: usize = 128;

enum Phase {
    Lobby,
    CountingDown(f32),
    Playing,
}

struct Sim {
    boards: [Board; 2],
    paused: bool,
    finished: bool,
    last_seq: [u32; 2],
    last_restart: Option<Instant>,
    start: Instant,
    max_chain: [u32; 2],
    total_chains: [u32; 2],
    nuisance_sent: [u32; 2],
    all_clears: [u32; 2],
    pieces_placed: [u32; 2],
    prev_chain: [u32; 2],
    prev_all_clear: [bool; 2],
    prev_piece_id: [u32; 2],
    // piece_id of each board as of the last broadcast. The board RNG only advances when a
    // piece spawns (which always bumps piece_id), so an unchanged piece_id means we can
    // omit the RNG from the next StateUpdate. `None` forces a full send (game start /
    // first frame), guaranteeing the client has an RNG before any RNG-less update arrives.
    last_sent_piece_id: Option<[u32; 2]>,
}

impl Sim {
    fn new(settings: &RoomSettings) -> Sim {
        Sim {
            boards: Self::fresh_boards(settings),
            paused: false,
            finished: false,
            last_seq: [0; 2],
            last_restart: None,
            start: Instant::now(),
            max_chain: [0; 2],
            total_chains: [0; 2],
            nuisance_sent: [0; 2],
            all_clears: [0; 2],
            pieces_placed: [0; 2],
            prev_chain: [0; 2],
            prev_all_clear: [false; 2],
            prev_piece_id: [0; 2],
            last_sent_piece_id: None,
        }
    }

    fn fresh_boards(s: &RoomSettings) -> [Board; 2] {
        let seed: u64 = rand::rng().random();
        [
            Board::new(
                config::GRID_WIDTH,
                config::GRID_HEIGHT,
                seed,
                s.starting_level,
                s.colors,
            ),
            Board::new(
                config::GRID_WIDTH,
                config::GRID_HEIGHT,
                seed,
                s.starting_level,
                s.colors,
            ),
        ]
    }

    fn reset_boards(&mut self, s: &RoomSettings) {
        self.boards = Self::fresh_boards(s);
        self.boards[0].spawn_piece();
        self.boards[1].spawn_piece();
        self.paused = false;
        self.finished = false;
        self.last_seq = [0; 2];
        self.start = Instant::now();
        self.max_chain = [0; 2];
        self.total_chains = [0; 2];
        self.nuisance_sent = [0; 2];
        self.all_clears = [0; 2];
        self.pieces_placed = [0; 2];
        self.prev_chain = [0; 2];
        self.prev_all_clear = [false; 2];
        self.prev_piece_id = [self.boards[0].piece_id, self.boards[1].piece_id];
        self.last_sent_piece_id = None;
    }
}

struct Member {
    token: Token,
    conn: Option<ConnId>,
    disconnect_at: Option<Instant>,
    user_id: Option<Uuid>,
}

struct Room {
    id: RoomId,
    name: String,
    host: Token,
    members: Vec<Member>,
    settings: RoomSettings,
    phase: Phase,
    sim: Sim,
}

impl Room {
    fn slot_of_conn(&self, conn: ConnId) -> Option<usize> {
        self.members.iter().position(|m| m.conn == Some(conn))
    }

    fn is_host_conn(&self, conn: ConnId) -> bool {
        self.slot_of_conn(conn)
            .is_some_and(|s| self.members[s].token == self.host)
    }

    fn all_connected(&self) -> bool {
        self.members.iter().all(|m| m.conn.is_some())
    }

    fn connected_conns(&self) -> Vec<ConnId> {
        self.members.iter().filter_map(|m| m.conn).collect()
    }

    fn lobby_info_for(&self, idx: usize) -> LobbyInfo {
        LobbyInfo {
            id: self.id,
            name: self.name.clone(),
            settings: self.settings,
            players: self.members.len() as u8,
            your_slot: (idx + 1) as u8,
            is_host: self.members[idx].token == self.host,
            countdown: match self.phase {
                Phase::CountingDown(t) => Some(t.ceil() as u8),
                _ => None,
            },
        }
    }

    fn info(&self) -> RoomInfo {
        RoomInfo {
            id: self.id,
            name: self.name.clone(),
            players: self.members.len() as u8,
            max: 2,
            in_game: !matches!(self.phase, Phase::Lobby),
            friends_only: self.settings.friends_only,
        }
    }
}

struct Manager {
    rooms: HashMap<RoomId, Room>,
    clients: HashMap<ConnId, Option<RoomId>>,
    conn_token: HashMap<ConnId, Token>,
    conn_user_id: HashMap<ConnId, Uuid>,
    conn_username: HashMap<ConnId, String>,
    senders: HashMap<ConnId, mpsc::Sender<Vec<u8>>>,
    dead: Vec<ConnId>,
    next_id: RoomId,
    room_list_dirty: bool,
    last_room_list: Instant,
    pool: db::DbPool,
}

enum Command {
    Register {
        conn: ConnId,
        sender: mpsc::Sender<Vec<u8>>,
    },
    Unregister {
        conn: ConnId,
    },
    Hello {
        conn: ConnId,
        token: Token,
        user_id: Option<Uuid>,
        username: Option<String>,
    },
    RequestRoomList {
        conn: ConnId,
    },
    CreateRoom {
        conn: ConnId,
        name: String,
    },
    JoinRoom {
        conn: ConnId,
        id: RoomId,
    },
    LeaveRoom {
        conn: ConnId,
    },
    SetSetting {
        conn: ConnId,
        index: u8,
        dir: i32,
    },
    ToggleCountdown {
        conn: ConnId,
    },
    ReturnToLobby {
        conn: ConnId,
    },
    Input {
        conn: ConnId,
        kind: InputKind,
        seq: u32,
    },
    TogglePause {
        conn: ConnId,
    },
    Restart {
        conn: ConnId,
    },
    InviteFriend {
        conn: ConnId,
        target_user_id: String,
    },
}

fn clean_name(name: String) -> String {
    let n = name.trim();
    if n.is_empty() {
        "Room".to_string()
    } else {
        n.chars().take(24).collect()
    }
}

impl Manager {
    fn new(pool: db::DbPool) -> Manager {
        Manager {
            rooms: HashMap::new(),
            clients: HashMap::new(),
            conn_token: HashMap::new(),
            conn_user_id: HashMap::new(),
            conn_username: HashMap::new(),
            senders: HashMap::new(),
            dead: Vec::new(),
            next_id: 1,
            room_list_dirty: false,
            last_room_list: Instant::now(),
            pool,
        }
    }

    fn room_of(&self, conn: ConnId) -> Option<RoomId> {
        self.clients.get(&conn).copied().flatten()
    }

    fn with_room<R>(&mut self, id: RoomId, f: impl FnOnce(&mut Room) -> R) -> Option<R> {
        self.rooms.get_mut(&id).map(f)
    }

    fn deliver(&mut self, conn: ConnId, payload: Vec<u8>) {
        if let Some(sender) = self.senders.get(&conn) {
            if sender.try_send(payload).is_err() {
                self.dead.push(conn);
            }
        }
    }

    /// Encode `msg` and deliver it to a single connection, dropping the message
    /// (with a log line) rather than sending a corrupt payload if encoding fails.
    fn deliver_msg(&mut self, conn: ConnId, msg: &ServerMessage) {
        match shared::encode(msg) {
            Ok(payload) => self.deliver(conn, payload),
            Err(e) => error!("encode failed, dropping message: {e}"),
        }
    }

    /// Encode `msg` once and deliver it to every connected member of a room.
    fn send_room_msg(&mut self, id: RoomId, msg: &ServerMessage) {
        match shared::encode(msg) {
            Ok(payload) => self.send_room(id, payload),
            Err(e) => error!("encode failed, dropping message: {e}"),
        }
    }

    fn send_room(&mut self, id: RoomId, payload: Vec<u8>) {
        let conns = match self.rooms.get(&id) {
            Some(room) => room.connected_conns(),
            None => return,
        };
        for c in conns {
            self.deliver(c, payload.clone());
        }
    }

    fn send_lobby(&mut self, id: RoomId) {
        let msgs: Vec<(ConnId, Vec<u8>)> = match self.rooms.get(&id) {
            Some(room) => room
                .members
                .iter()
                .enumerate()
                .filter_map(|(i, m)| {
                    let c = m.conn?;
                    let msg = ServerMessage::Lobby {
                        info: room.lobby_info_for(i),
                    };
                    match shared::encode(&msg) {
                        Ok(payload) => Some((c, payload)),
                        Err(e) => {
                            error!("encode Lobby failed: {e}");
                            None
                        }
                    }
                })
                .collect(),
            None => return,
        };
        for (c, payload) in msgs {
            self.deliver(c, payload);
        }
    }

    fn send_snapshot(&mut self, id: RoomId) {
        let msg = match self.rooms.get(&id) {
            // A snapshot is a fresh baseline (reconnect / resume), so it always carries the
            // full RNG state so the receiver is fully in sync.
            Some(room) => ServerMessage::StateUpdate {
                p1_board: Box::new(room.sim.boards[0].clone()),
                p2_board: Box::new(room.sim.boards[1].clone()),
                p1_rng: Some(Box::new(room.sim.boards[0].rng_state())),
                p2_rng: Some(Box::new(room.sim.boards[1].rng_state())),
                p1_ack: room.sim.last_seq[0],
                p2_ack: room.sim.last_seq[1],
            },
            None => return,
        };
        self.send_room_msg(id, &msg);
    }

    fn public_room_list(&self) -> Vec<RoomInfo> {
        self.rooms
            .values()
            .filter(|r| !r.settings.friends_only)
            .map(|r| r.info())
            .collect()
    }

    fn broadcast_room_list(&mut self) {
        let payload = match shared::encode(&ServerMessage::RoomList {
            rooms: self.public_room_list(),
        }) {
            Ok(p) => p,
            Err(e) => {
                error!("encode RoomList failed: {e}");
                return;
            }
        };
        let browsing: Vec<ConnId> = self
            .clients
            .iter()
            .filter_map(|(&c, loc)| loc.is_none().then_some(c))
            .collect();
        for c in browsing {
            self.deliver(c, payload.clone());
        }
    }

    fn send_room_list_to(&mut self, conn: ConnId) {
        self.deliver_msg(
            conn,
            &ServerMessage::RoomList {
                rooms: self.public_room_list(),
            },
        );
    }

    fn room_of_token(&self, token: &str) -> Option<RoomId> {
        self.rooms
            .values()
            .find(|r| r.members.iter().any(|m| m.token == token))
            .map(|r| r.id)
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Register { conn, sender } => {
                self.senders.insert(conn, sender);
                self.clients.insert(conn, None);
            }
            Command::Hello {
                conn,
                token,
                user_id,
                username,
            } => {
                self.conn_token.insert(conn, token.clone());
                if let Some(uid) = user_id {
                    self.conn_user_id.insert(conn, uid);
                }
                if let Some(name) = username {
                    self.conn_username.insert(conn, name);
                }
                if let Some(id) = self.room_of_token(&token) {
                    self.rejoin(conn, id);
                } else {
                    self.send_room_list_to(conn);
                }
            }
            Command::Unregister { conn } => {
                self.drop_connection(conn);
            }
            Command::RequestRoomList { conn } => {
                self.send_room_list_to(conn);
            }
            Command::CreateRoom { conn, name } => {
                let token = match self.conn_token.get(&conn) {
                    Some(t) => t.clone(),
                    None => return,
                };
                self.leave_current(conn);
                let id = self.next_id;
                self.next_id += 1;
                let settings = RoomSettings::default();
                let user_id = self.conn_user_id.get(&conn).copied();
                let room = Room {
                    id,
                    name: clean_name(name),
                    host: token.clone(),
                    members: vec![Member {
                        token,
                        conn: Some(conn),
                        disconnect_at: None,
                        user_id,
                    }],
                    settings,
                    phase: Phase::Lobby,
                    sim: Sim::new(&settings),
                };
                self.rooms.insert(id, room);
                self.clients.insert(conn, Some(id));
                self.send_lobby(id);
                self.room_list_dirty = true;
                info!("Room #{id} créée (conn {conn})");
            }
            Command::JoinRoom { conn, id } => {
                let token = match self.conn_token.get(&conn) {
                    Some(t) => t.clone(),
                    None => return,
                };
                let joinable = matches!(self.rooms.get(&id), Some(r) if r.members.len() < 2);
                if !joinable {
                    self.deliver_msg(
                        conn,
                        &ServerMessage::JoinFailed {
                            reason: "Room indisponible".into(),
                        },
                    );
                    return;
                }
                if let Some(room) = self.rooms.get(&id) {
                    if room.settings.friends_only {
                        let host_uid = room.members.first().and_then(|m| m.user_id);
                        let joiner_uid = self.conn_user_id.get(&conn).copied();
                        let allowed = match (host_uid, joiner_uid) {
                            (Some(h), Some(j)) => tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current().block_on(db::are_friends(&self.pool, h, j))
                            })
                            .unwrap_or(false),
                            _ => false,
                        };
                        if !allowed {
                            self.deliver_msg(
                                conn,
                                &ServerMessage::JoinFailed {
                                    reason: "Cette room est réservée aux amis de l'hôte.".into(),
                                },
                            );
                            return;
                        }
                    }
                }
                self.leave_current(conn);
                let join_user_id = self.conn_user_id.get(&conn).copied();
                let pushed = self
                    .with_room(id, |room| {
                        if room.members.len() < 2 {
                            room.members.push(Member {
                                token,
                                conn: Some(conn),
                                disconnect_at: None,
                                user_id: join_user_id,
                            });
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if pushed {
                    self.clients.insert(conn, Some(id));
                    self.sync_after_attach(id, conn);
                } else {
                    self.deliver_msg(
                        conn,
                        &ServerMessage::JoinFailed {
                            reason: "Room indisponible".into(),
                        },
                    );
                }
            }
            Command::LeaveRoom { conn } => {
                self.leave_current(conn);
                self.send_room_list_to(conn);
            }
            Command::SetSetting { conn, index, dir } => {
                if let Some(id) = self.room_of(conn) {
                    let changed = self
                        .with_room(id, |room| {
                            if room.is_host_conn(conn) && matches!(room.phase, Phase::Lobby) {
                                room.settings.adjust(index as usize, dir);
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if changed {
                        self.send_lobby(id);
                    }
                }
            }
            Command::ToggleCountdown { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let toggled = self
                        .with_room(id, |room| {
                            if room.is_host_conn(conn) {
                                room.phase = match room.phase {
                                    Phase::Lobby if room.members.len() >= 2 => Phase::CountingDown(3.0),
                                    Phase::Lobby => Phase::Lobby,
                                    Phase::CountingDown(_) => Phase::Lobby,
                                    Phase::Playing => Phase::Playing,
                                };
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if toggled {
                        self.send_lobby(id);
                        self.room_list_dirty = true;
                    }
                }
            }
            Command::ReturnToLobby { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let done = self
                        .with_room(id, |room| {
                            if room.is_host_conn(conn) {
                                room.phase = Phase::Lobby;
                                room.sim.finished = false;
                                room.sim.paused = false;
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if done {
                        self.send_lobby(id);
                        self.room_list_dirty = true;
                    }
                }
            }
            Command::Input { conn, kind, seq } => {
                if let Some(id) = self.room_of(conn) {
                    self.with_room(id, |room| {
                        if let Some(idx) = room.slot_of_conn(conn) {
                            room.sim.last_seq[idx] = seq;
                            if matches!(room.phase, Phase::Playing) && !room.sim.paused && !room.sim.finished {
                                room.sim.boards[idx].apply_input(kind);
                            }
                        }
                    });
                }
            }
            Command::TogglePause { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let toggled = self
                        .with_room(id, |room| {
                            if matches!(room.phase, Phase::Playing) && !room.sim.finished {
                                room.sim.paused = !room.sim.paused;
                                let p = room.sim.paused;
                                room.sim.boards.iter_mut().for_each(|b| b.set_paused(p));
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if toggled {
                        self.send_snapshot(id);
                    }
                }
            }

            Command::Restart { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let restarted = self
                        .with_room(id, |room| {
                            if matches!(room.phase, Phase::Playing) {
                                let now = Instant::now();
                                let in_cooldown = room
                                    .sim
                                    .last_restart
                                    .is_some_and(|t| now.duration_since(t) < Duration::from_secs(2));
                                if !in_cooldown {
                                    room.sim.last_restart = Some(now);
                                    room.sim.reset_boards(&room.settings);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if restarted {
                        self.send_room_msg(id, &ServerMessage::Restart);
                    }
                }
            }
            Command::InviteFriend { conn, target_user_id } => {
                let from_username = self
                    .conn_username
                    .get(&conn)
                    .cloned()
                    .unwrap_or_else(|| "Un joueur".to_string());
                let room_info = self
                    .room_of(conn)
                    .and_then(|id| self.rooms.get(&id).map(|r| (id, r.name.clone())));
                let Some((room_id, room_name)) = room_info else { return };
                let target_uuid = match Uuid::parse_str(&target_user_id) {
                    Ok(u) => u,
                    Err(_) => return,
                };
                let target_conn = self
                    .conn_user_id
                    .iter()
                    .find(|(_, uid)| **uid == target_uuid)
                    .map(|(&c, _)| c);
                if let Some(tc) = target_conn {
                    self.deliver_msg(
                        tc,
                        &ServerMessage::FriendInvitation {
                            from_username,
                            room_id,
                            room_name,
                        },
                    );
                }
            }
        }
    }

    fn rejoin(&mut self, conn: ConnId, id: RoomId) {
        let token = match self.conn_token.get(&conn) {
            Some(t) => t.clone(),
            None => return,
        };
        let mut replaced: Option<ConnId> = None;
        if let Some(room) = self.rooms.get_mut(&id) {
            if let Some(slot) = room.members.iter().position(|m| m.token == token) {
                replaced = room.members[slot].conn.filter(|&c| c != conn);
                room.members[slot].conn = Some(conn);
                room.members[slot].disconnect_at = None;
            }
        }
        self.clients.insert(conn, Some(id));
        if let Some(old) = replaced {
            self.clients.remove(&old);
            self.conn_token.remove(&old);
            self.conn_user_id.remove(&old);
            self.conn_username.remove(&old);
            self.senders.remove(&old);
        }
        self.sync_after_attach(id, conn);
        info!("Reconnexion room #{id} (conn {conn})");
    }

    fn sync_after_attach(&mut self, id: RoomId, conn: ConnId) {
        let resume = if let Some(room) = self.rooms.get_mut(&id) {
            let playing = matches!(room.phase, Phase::Playing);
            let all = room.all_connected();
            if playing && all {
                room.sim.paused = false;
                room.sim.boards.iter_mut().for_each(|b| b.set_paused(false));
            }
            (playing, all)
        } else {
            return;
        };

        self.send_lobby(id);
        match resume {
            (true, true) => {
                self.send_room_msg(id, &ServerMessage::GameStart);
            }
            (true, false) => {
                self.deliver_msg(conn, &ServerMessage::GameStart);
                self.deliver_msg(conn, &ServerMessage::OpponentDisconnected);
            }
            _ => {}
        }
        if resume.0 {
            self.send_snapshot(id);
        }
        self.room_list_dirty = true;
    }

    fn mark_disconnected(&mut self, conn: ConnId) {
        let id = match self.room_of(conn) {
            Some(id) => id,
            None => return,
        };
        let (notify, in_game) = if let Some(room) = self.rooms.get_mut(&id) {
            let slot = match room.slot_of_conn(conn) {
                Some(s) => s,
                None => return,
            };
            room.members[slot].conn = None;
            room.members[slot].disconnect_at = Some(Instant::now());
            let playing = matches!(room.phase, Phase::Playing);
            let notify = playing && !room.sim.finished;
            if notify {
                room.sim.paused = true;
                room.sim.boards.iter_mut().for_each(|b| b.set_paused(true));
            }
            (notify, playing)
        } else {
            return;
        };

        if notify {
            self.send_room_msg(id, &ServerMessage::OpponentDisconnected);
        }
        if !in_game {
            self.send_lobby(id);
        }
        info!("WS {conn} déconnecté (grâce {}s)", GRACE.as_secs());
    }

    fn drop_connection(&mut self, conn: ConnId) {
        self.mark_disconnected(conn);
        self.clients.remove(&conn);
        self.conn_token.remove(&conn);
        self.conn_user_id.remove(&conn);
        self.conn_username.remove(&conn);
        self.senders.remove(&conn);
    }

    fn reap_dead(&mut self) {
        while !self.dead.is_empty() {
            for conn in std::mem::take(&mut self.dead) {
                if self.senders.contains_key(&conn) {
                    warn!("WS {conn} trop lente, fermeture");
                    self.drop_connection(conn);
                }
            }
        }
    }

    fn leave_current(&mut self, conn: ConnId) {
        let id = match self.room_of(conn) {
            Some(id) => id,
            None => return,
        };
        self.clients.insert(conn, None);
        let slot = match self.rooms.get(&id).and_then(|r| r.slot_of_conn(conn)) {
            Some(s) => s,
            None => return,
        };
        self.remove_member(id, slot);
    }

    fn remove_member(&mut self, id: RoomId, slot: usize) {
        let closed = if let Some(room) = self.rooms.get_mut(&id) {
            if slot >= room.members.len() {
                return;
            }
            let removed = room.members.remove(slot);
            if room.members.is_empty() {
                true
            } else {
                if removed.token == room.host {
                    room.host = room.members[0].token.clone();
                }
                if room.members.len() < 2 && !matches!(room.phase, Phase::Lobby) {
                    room.phase = Phase::Lobby;
                    room.sim.finished = false;
                    room.sim.paused = false;
                }
                false
            }
        } else {
            return;
        };

        if closed {
            self.rooms.remove(&id);
        } else {
            self.send_lobby(id);
        }
        self.room_list_dirty = true;
    }

    fn tick(&mut self, dt: f32, do_broadcast: bool) -> Vec<db::MatchRecord> {
        if self.room_list_dirty && self.last_room_list.elapsed() >= Duration::from_millis(200) {
            self.broadcast_room_list();
            self.room_list_dirty = false;
            self.last_room_list = Instant::now();
        }

        let now = Instant::now();
        let expired: Vec<(RoomId, Token)> = self
            .rooms
            .values()
            .flat_map(|r| {
                r.members
                    .iter()
                    .filter(|m| m.disconnect_at.is_some_and(|t| now.duration_since(t) >= GRACE))
                    .map(move |m| (r.id, m.token.clone()))
            })
            .collect();
        for (id, token) in expired {
            if let Some(slot) = self
                .rooms
                .get(&id)
                .and_then(|r| r.members.iter().position(|m| m.token == token))
            {
                info!("Grâce expirée, retrait room #{id}");
                self.remove_member(id, slot);
            }
        }

        let mut outgoing: Vec<(ConnId, Vec<u8>)> = Vec::new();
        let mut list_changed = false;
        let mut finished_matches: Vec<db::MatchRecord> = Vec::new();

        for room in self.rooms.values_mut() {
            if matches!(room.phase, Phase::Lobby) {
                continue;
            }

            let mut start_now = false;
            let mut cd_changed = false;
            if let Phase::CountingDown(t) = &mut room.phase {
                let before = t.ceil() as u8;
                *t -= dt;
                if *t <= 0.0 {
                    start_now = true;
                } else if t.ceil() as u8 != before {
                    cd_changed = true;
                }
            }
            if start_now {
                room.phase = Phase::Playing;
                room.sim.reset_boards(&room.settings);
                match shared::encode(&ServerMessage::GameStart) {
                    Ok(payload) => {
                        for m in &room.members {
                            if let Some(c) = m.conn {
                                outgoing.push((c, payload.clone()));
                            }
                        }
                    }
                    Err(e) => error!("encode GameStart failed: {e}"),
                }
                list_changed = true;
            } else if cd_changed {
                for (i, m) in room.members.iter().enumerate() {
                    if let Some(c) = m.conn {
                        let msg = ServerMessage::Lobby {
                            info: room.lobby_info_for(i),
                        };
                        match shared::encode(&msg) {
                            Ok(payload) => outgoing.push((c, payload)),
                            Err(e) => error!("encode Lobby failed: {e}"),
                        }
                    }
                }
            }

            if matches!(room.phase, Phase::Playing) {
                let advanced = !room.sim.paused && !room.sim.finished;
                let mut just_finished = false;
                if advanced {
                    let g0 = room.sim.boards[0].tick(dt);
                    let g1 = room.sim.boards[1].tick(dt);
                    room.sim.boards[1].pending_garbage += g0;
                    room.sim.boards[0].pending_garbage += g1;

                    room.sim.nuisance_sent[0] += g0;
                    room.sim.nuisance_sent[1] += g1;

                    for i in 0..2 {
                        let cc = room.sim.boards[i].chain_count;
                        if cc > 0 && room.sim.prev_chain[i] == 0 {
                            room.sim.total_chains[i] += 1;
                        }
                        room.sim.max_chain[i] = room.sim.max_chain[i].max(cc);
                        room.sim.prev_chain[i] = cc;

                        let ac = room.sim.boards[i].last_was_all_clear;
                        if ac && !room.sim.prev_all_clear[i] {
                            room.sim.all_clears[i] += 1;
                        }
                        room.sim.prev_all_clear[i] = ac;

                        let pid = room.sim.boards[i].piece_id;
                        if pid != room.sim.prev_piece_id[i] {
                            room.sim.pieces_placed[i] += 1;
                            room.sim.prev_piece_id[i] = pid;
                        }
                    }

                    if room.sim.boards[0].state == GameState::GameOver
                        || room.sim.boards[1].state == GameState::GameOver
                    {
                        room.sim.finished = true;
                        just_finished = true;
                        let winner_slot = if room.sim.boards[0].state == GameState::GameOver
                            && room.sim.boards[1].state != GameState::GameOver
                        {
                            2u8
                        } else {
                            1u8
                        };
                        let duration_secs = room.sim.start.elapsed().as_secs_f64();
                        info!(
                            "Match terminé room #{} → slot {winner_slot} gagne ({:.0}s)",
                            room.id, duration_secs
                        );
                        finished_matches.push(db::MatchRecord {
                            duration_secs,
                            winner_slot,
                            user_ids: [
                                room.members.first().and_then(|m| m.user_id),
                                room.members.get(1).and_then(|m| m.user_id),
                            ],
                            max_chain: room.sim.max_chain,
                            total_chains: room.sim.total_chains,
                            nuisance_sent: room.sim.nuisance_sent,
                            all_clears: room.sim.all_clears,
                            pieces_placed: room.sim.pieces_placed,
                        });
                    }
                }
                if (do_broadcast && advanced) || just_finished {
                    // Include each board's RNG only if it advanced since the last broadcast
                    // (piece_id changed) or there is no baseline yet; otherwise omit it and
                    // let the client reuse the RNG it already has.
                    let baseline = room.sim.last_sent_piece_id;
                    let pid = [room.sim.boards[0].piece_id, room.sim.boards[1].piece_id];
                    let rng_if_changed = |i: usize| {
                        // `just_finished` forces a full RNG send: a garbage drop that ends
                        // the game advances the RNG without bumping piece_id, so the normal
                        // change detector would miss it and leave the final snapshot's RNG
                        // one step stale.
                        let changed = just_finished || baseline.is_none_or(|ids| ids[i] != pid[i]);
                        changed.then(|| Box::new(room.sim.boards[i].rng_state()))
                    };
                    let msg = ServerMessage::StateUpdate {
                        p1_board: Box::new(room.sim.boards[0].clone()),
                        p2_board: Box::new(room.sim.boards[1].clone()),
                        p1_rng: rng_if_changed(0),
                        p2_rng: rng_if_changed(1),
                        p1_ack: room.sim.last_seq[0],
                        p2_ack: room.sim.last_seq[1],
                    };
                    room.sim.last_sent_piece_id = Some(pid);
                    match shared::encode(&msg) {
                        Ok(upd) => {
                            for m in &room.members {
                                if let Some(c) = m.conn {
                                    outgoing.push((c, upd.clone()));
                                }
                            }
                        }
                        Err(e) => error!("encode StateUpdate failed: {e}"),
                    }
                }
            }
        }

        for (conn, payload) in outgoing {
            self.deliver(conn, payload);
        }
        if list_changed {
            self.room_list_dirty = true;
        }
        finished_matches
    }
}

struct TickProfile {
    enabled: bool,
    budget: Duration,
    sum: Duration,
    max: Duration,
    count: u32,
    since_report: Instant,
}

impl TickProfile {
    fn new() -> TickProfile {
        TickProfile {
            enabled: std::env::var("PUYO_PROFILE").is_ok(),
            budget: Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64),
            sum: Duration::ZERO,
            max: Duration::ZERO,
            count: 0,
            since_report: Instant::now(),
        }
    }

    fn record(&mut self, elapsed: Duration, rooms: usize) {
        self.sum += elapsed;
        self.max = self.max.max(elapsed);
        self.count += 1;
        if self.since_report.elapsed() < Duration::from_secs(5) {
            return;
        }
        if self.enabled {
            let avg = self.sum / self.count.max(1);
            let peak_load = self.max.as_secs_f64() / self.budget.as_secs_f64() * 100.0;
            info!(
                "[tick] rooms={rooms} avg={avg:?} max={:?} peak={peak_load:.1}%",
                self.max
            );
        }
        self.sum = Duration::ZERO;
        self.max = Duration::ZERO;
        self.count = 0;
        self.since_report = Instant::now();
    }
}

async fn manager_loop(mut cmd_rx: mpsc::UnboundedReceiver<Command>, pool: db::DbPool) {
    let tick_dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let mut ticker = interval(Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64));
    let broadcast_period = Duration::from_secs_f64(1.0 / config::STATE_BROADCAST_HZ as f64);
    let mut mgr = Manager::new(pool.clone());
    let mut last_broadcast = Instant::now();
    let mut profile = TickProfile::new();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let do_broadcast = last_broadcast.elapsed() >= broadcast_period;
                if do_broadcast { last_broadcast = Instant::now(); }
                let t0 = Instant::now();
                let finished = mgr.tick(tick_dt, do_broadcast);
                profile.record(t0.elapsed(), mgr.rooms.len());
                mgr.reap_dead();
                for rec in finished {
                    let pool = pool.clone();
                    tokio::spawn(async move {
                        if let Err(e) = db::record_match_result(&pool, rec).await {
                            error!("Match save: {e}");
                        }
                    });
                }
            }
            Some(cmd) = cmd_rx.recv() => {
                mgr.handle(cmd);
                mgr.reap_dead();
            }
        }
    }
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "console")]
    console_subscriber::init();
    #[cfg(not(feature = "console"))]
    {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .init();
    }

    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = db::init_pool(&database_url)
        .await
        .expect("Failed to connect to database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");
    info!("DB connectée");
    tokio::task::spawn_blocking(db::dummy_hash)
        .await
        .expect("dummy hash init failed");

    let port = config::SERVER_PORT;
    info!("Écoute sur :{port}");

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
    tokio::spawn(manager_loop(cmd_rx, pool.clone()));

    let pool_cleanup = pool.clone();
    tokio::spawn(async move {
        let start = tokio::time::Instant::now() + Duration::from_secs(3600);
        let mut ticker = tokio::time::interval_at(start, Duration::from_secs(3600));
        loop {
            ticker.tick().await;
            match db::cleanup_expired_sessions(&pool_cleanup).await {
                Ok(n) if n > 0 => info!("Sessions expirées : {n} supprimées"),
                Err(e) => error!("Session cleanup: {e}"),
                _ => {}
            }
        }
    });

    let conn_counter = Arc::new(AtomicU64::new(1));
    let pool_ws = pool.clone();
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || cmd_tx.clone()))
        .and(warp::any().map(move || conn_counter.clone()))
        .and(warp::any().map(move || pool_ws.clone()))
        .map(|ws: warp::ws::Ws, cmd_tx, counter: Arc<AtomicU64>, pool: db::DbPool| {
            let conn = counter.fetch_add(1, Ordering::Relaxed);
            ws.on_upgrade(move |socket| handle_connection(socket, cmd_tx, conn, pool))
        });

    let routes = ws_route
        .or(auth::routes(pool))
        .recover(auth::handle_rejection)
        .with(warp::log::custom(|info| {
            if info.status() == warp::http::StatusCode::SWITCHING_PROTOCOLS {
                return;
            }
            let ms = info.elapsed().as_millis();
            let status = info.status();
            let msg = format!("{} {} {} {}ms", info.method(), info.path(), status.as_u16(), ms);
            if status.is_server_error() {
                error!("{msg}");
            } else if status.is_client_error() {
                warn!("{msg}");
            } else {
                info!("{msg}");
            }
        }));

    warp::serve(routes).run((config::SERVER_BIND_ADDRESS, port)).await;
}

async fn handle_connection(
    ws: warp::ws::WebSocket,
    cmd_tx: mpsc::UnboundedSender<Command>,
    conn: ConnId,
    pool: db::DbPool,
) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    info!("WS {conn} ouverture");

    let _ = cmd_tx.send(Command::Register {
        conn,
        sender: to_client_tx,
    });

    let mut send_task = tokio::spawn(async move {
        while let Some(payload) = to_client_rx.recv().await {
            if user_ws_tx.send(warp::ws::Message::binary(payload)).await.is_err() {
                break;
            }
        }
    });

    let cmd_tx_recv = cmd_tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = user_ws_rx.next().await {
            if let Ok(msg) = result {
                if msg.is_binary() {
                    if let Some(client_msg) = shared::decode::<ClientMessage>(msg.as_bytes()) {
                        let cmd = match client_msg {
                            ClientMessage::Hello {
                                player_id,
                                auth_token,
                                username,
                            } => {
                                let user_id = match auth_token.as_deref().and_then(|t| Uuid::parse_str(t).ok()) {
                                    Some(token_uuid) => db::find_user_by_token(&pool, token_uuid)
                                        .await
                                        .ok()
                                        .flatten()
                                        .map(|u| u.id),
                                    None => None,
                                };
                                Command::Hello {
                                    conn,
                                    token: player_id,
                                    user_id,
                                    username,
                                }
                            }
                            ClientMessage::Input { kind, seq } => Command::Input { conn, kind, seq },
                            ClientMessage::TogglePause => Command::TogglePause { conn },
                            ClientMessage::RequestRestart => Command::Restart { conn },
                            ClientMessage::RequestRoomList => Command::RequestRoomList { conn },
                            ClientMessage::CreateRoom { name } => Command::CreateRoom { conn, name },
                            ClientMessage::JoinRoom { id } => Command::JoinRoom { conn, id },
                            ClientMessage::LeaveRoom => Command::LeaveRoom { conn },
                            ClientMessage::SetRoomSetting { index, dir } => Command::SetSetting { conn, index, dir },
                            ClientMessage::ToggleCountdown => Command::ToggleCountdown { conn },
                            ClientMessage::ReturnToLobby => Command::ReturnToLobby { conn },
                            ClientMessage::InviteFriend { user_id } => Command::InviteFriend {
                                conn,
                                target_user_id: user_id,
                            },
                        };
                        if cmd_tx_recv.send(cmd).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    let _ = cmd_tx.send(Command::Unregister { conn });
    info!("WS {conn} fermeture");
}

#[cfg(test)]
mod tests;
