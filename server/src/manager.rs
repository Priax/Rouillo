use std::collections::{HashMap, HashSet};

use shared::{config, InputKind, RoomId, RoomInfo, RoomSettings, ServerMessage};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::room::{Member, Phase, Room};
use crate::sim::Sim;
use crate::{db, ConnId, Token};

pub const GRACE: Duration = Duration::from_secs(config::RECONNECT_GRACE_SECS);

pub const INVITE_COOLDOWN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum FriendCheck {
    Join {
        conn: ConnId,
        room: RoomId,
        host: Token,
        from: Option<RoomId>,
        joiner: Uuid,
        host_user: Uuid,
    },
    Invite {
        conn: ConnId,
        room: RoomId,
        inviter: Uuid,
        target: Uuid,
    },
}

impl FriendCheck {
    fn conn(&self) -> ConnId {
        match self {
            Self::Join { conn, .. } | Self::Invite { conn, .. } => *conn,
        }
    }

    pub fn users(&self) -> (Uuid, Uuid) {
        match self {
            Self::Join { joiner, host_user, .. } => (*joiner, *host_user),
            Self::Invite { inviter, target, .. } => (*inviter, *target),
        }
    }
}

const JOIN_UNAVAILABLE: &str = "Room indisponible";
const JOIN_FRIENDS_ONLY: &str = "Cette room est réservée aux amis de l'hôte.";
const JOIN_MAINTENANCE: &str = "Le serveur redémarre, réessaie dans un instant.";
const JOIN_SAME_ACCOUNT: &str = "Ce compte est déjà dans cette room.";

pub struct Manager {
    pub rooms: HashMap<RoomId, Room>,
    pub clients: HashMap<ConnId, Option<RoomId>>,
    pub conn_token: HashMap<ConnId, Token>,
    conn_user_id: HashMap<ConnId, Uuid>,
    conn_username: HashMap<ConnId, String>,
    senders: HashMap<ConnId, mpsc::Sender<Vec<u8>>>,
    dead: Vec<ConnId>,
    next_id: RoomId,
    room_list_dirty: bool,
    last_room_list: Instant,
    friend_checks: Vec<FriendCheck>,
    pub checks_in_flight: HashSet<ConnId>,
    pub last_invite: HashMap<ConnId, Instant>,
    unsaved_matches: Vec<db::MatchRecord>,
    closing: bool,
}

pub enum Command {
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
        last_disconnect_reason: Option<String>,
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
        tick: u32,
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
    FriendCheckDone {
        check: FriendCheck,
        friends: bool,
    },
    Shutdown,
}

impl Command {
    fn client(&self) -> Option<ConnId> {
        match self {
            Self::Register { .. } | Self::FriendCheckDone { .. } | Self::Shutdown => None,
            Self::Unregister { conn }
            | Self::Hello { conn, .. }
            | Self::RequestRoomList { conn }
            | Self::CreateRoom { conn, .. }
            | Self::JoinRoom { conn, .. }
            | Self::LeaveRoom { conn }
            | Self::SetSetting { conn, .. }
            | Self::ToggleCountdown { conn }
            | Self::ReturnToLobby { conn }
            | Self::Input { conn, .. }
            | Self::TogglePause { conn }
            | Self::Restart { conn }
            | Self::InviteFriend { conn, .. } => Some(*conn),
        }
    }
}

fn clean_name(name: &str) -> String {
    let n = name.trim();
    if n.is_empty() {
        "Room".to_string()
    } else {
        n.chars().take(24).collect()
    }
}

impl Manager {
    pub fn new() -> Self {
        Self {
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
            friend_checks: Vec::new(),
            checks_in_flight: HashSet::new(),
            last_invite: HashMap::new(),
            unsaved_matches: Vec::new(),
            closing: false,
        }
    }

    fn begin_shutdown(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        let counting: Vec<RoomId> = self
            .rooms
            .values()
            .filter(|r| matches!(r.phase, Phase::CountingDown(_)))
            .map(|r| r.id)
            .collect();
        for id in counting {
            if let Some(room) = self.rooms.get_mut(&id) {
                room.phase = Phase::Lobby;
            }
            self.send_lobby(id);
            self.room_list_dirty = true;
        }
        let conns: Vec<ConnId> = self.senders.keys().copied().collect();
        for conn in conns {
            self.deliver_msg(conn, &ServerMessage::Maintenance);
        }
        info!(
            "Maintenance: {} partie(s) en cours à laisser finir",
            self.games_running()
        );
    }

    pub fn closing(&self) -> bool {
        self.closing
    }

    pub fn games_running(&self) -> usize {
        self.rooms.values().filter(|r| r.game_running()).count()
    }

    pub fn room_of(&self, conn: ConnId) -> Option<RoomId> {
        self.clients.get(&conn).copied().flatten()
    }

    fn record_forfeit(&mut self, id: RoomId, slot: usize, why: &str) {
        if let Some(rec) = self.rooms.get_mut(&id).and_then(|r| r.forfeit(slot)) {
            info!("Forfait room #{id}: slot {} perd ({why})", slot + 1);
            self.unsaved_matches.push(rec);
        }
    }

    pub fn take_unsaved_matches(&mut self) -> Vec<db::MatchRecord> {
        std::mem::take(&mut self.unsaved_matches)
    }

    pub fn take_friend_checks(&mut self) -> Vec<FriendCheck> {
        std::mem::take(&mut self.friend_checks)
    }

    fn join_failed(&mut self, conn: ConnId, reason: &str) {
        self.deliver_msg(
            conn,
            &ServerMessage::JoinFailed {
                reason: reason.to_string(),
            },
        );
    }

    fn conn_of_user(&self, user: Uuid) -> Option<ConnId> {
        self.conn_user_id.iter().find(|(_, u)| **u == user).map(|(&c, _)| c)
    }

    fn request_join(&mut self, conn: ConnId, id: RoomId) {
        if !self.conn_token.contains_key(&conn) {
            return;
        }
        if self.room_of(conn) == Some(id) {
            return;
        }
        if self.closing {
            self.join_failed(conn, JOIN_MAINTENANCE);
            return;
        }
        let Some(room) = self.rooms.get(&id).filter(|r| r.members.len() < 2) else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if !room.settings.friends_only {
            self.complete_join(conn, id);
            return;
        }
        let host = room.host.clone();
        let host_user = room.members.iter().find(|m| m.token == host).and_then(|m| m.user_id);
        let (Some(host_user), Some(joiner)) = (host_user, self.conn_user_id.get(&conn).copied()) else {
            self.join_failed(conn, JOIN_FRIENDS_ONLY);
            return;
        };
        if !self.checks_in_flight.insert(conn) {
            return;
        }
        let from = self.room_of(conn);
        self.friend_checks.push(FriendCheck::Join {
            conn,
            room: id,
            host,
            from,
            joiner,
            host_user,
        });
    }

    fn complete_join(&mut self, conn: ConnId, id: RoomId) {
        let Some(token) = self.conn_token.get(&conn).cloned() else {
            return;
        };
        if self.closing {
            self.join_failed(conn, JOIN_MAINTENANCE);
            return;
        }
        if self.rooms.get(&id).is_none_or(|r| r.members.len() >= 2) {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        }
        let user_id = self.conn_user_id.get(&conn).copied();
        if user_id.is_some() && self.rooms[&id].members.iter().any(|m| m.user_id == user_id) {
            self.join_failed(conn, JOIN_SAME_ACCOUNT);
            return;
        }
        self.leave_current(conn);
        if let Some(room) = self.rooms.get_mut(&id) {
            room.members.push(Member {
                token,
                conn: Some(conn),
                disconnect_at: None,
                user_id,
            });
        }
        self.clients.insert(conn, Some(id));
        self.sync_after_attach(id, conn);
    }

    fn finish_join_check(&mut self, conn: ConnId, room: RoomId, host: &str, from: Option<RoomId>, friends: bool) {
        if !self.senders.contains_key(&conn) || self.room_of(conn) != from {
            return;
        }
        let Some(r) = self.rooms.get(&room) else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if r.settings.friends_only {
            if r.host != host {
                self.request_join(conn, room);
                return;
            }
            if !friends {
                self.join_failed(conn, JOIN_FRIENDS_ONLY);
                return;
            }
        }
        self.complete_join(conn, room);
    }

    fn request_invite(&mut self, conn: ConnId, target: &str) {
        let Some(inviter) = self.conn_user_id.get(&conn).copied() else {
            return;
        };
        let Some(room) = self.room_of(conn) else { return };
        let Ok(target) = Uuid::parse_str(target) else { return };
        if target == inviter || self.conn_of_user(target).is_none() {
            return;
        }
        let now = Instant::now();
        if self
            .last_invite
            .get(&conn)
            .is_some_and(|&t| now.duration_since(t) < INVITE_COOLDOWN)
        {
            return;
        }
        if !self.checks_in_flight.insert(conn) {
            return;
        }
        self.last_invite.insert(conn, now);
        self.friend_checks.push(FriendCheck::Invite {
            conn,
            room,
            inviter,
            target,
        });
    }

    fn finish_invite_check(&mut self, conn: ConnId, room: RoomId, target: Uuid, friends: bool) {
        if !friends || self.room_of(conn) != Some(room) {
            return;
        }
        let Some(target_conn) = self.conn_of_user(target) else {
            return;
        };
        let Some(room_name) = self.rooms.get(&room).map(|r| r.name.clone()) else {
            return;
        };
        let from_username = self
            .conn_username
            .get(&conn)
            .cloned()
            .unwrap_or_else(|| "Un joueur".to_string());
        self.deliver_msg(
            target_conn,
            &ServerMessage::FriendInvitation {
                from_username,
                room_id: room,
                room_name,
            },
        );
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

    fn deliver_msg(&mut self, conn: ConnId, msg: &ServerMessage) {
        match shared::encode(msg) {
            Ok(payload) => self.deliver(conn, payload),
            Err(e) => error!("encode failed, dropping message: {e}"),
        }
    }

    fn send_room_msg(&mut self, id: RoomId, msg: &ServerMessage) {
        match shared::encode(msg) {
            Ok(payload) => self.send_room(id, &payload),
            Err(e) => error!("encode failed, dropping message: {e}"),
        }
    }

    fn send_room(&mut self, id: RoomId, payload: &[u8]) {
        let Some(conns) = self.rooms.get(&id).map(Room::connected_conns) else {
            return;
        };
        for c in conns {
            self.deliver(c, payload.to_vec());
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

    pub fn send_snapshot(&mut self, id: RoomId) {
        let msg = match self.rooms.get(&id) {
            Some(room) => room.sim.state_update(true),
            None => return,
        };
        self.send_room_msg(id, &msg);
    }

    pub fn public_room_list(&self) -> Vec<RoomInfo> {
        self.rooms
            .values()
            .filter(|r| !r.settings.friends_only)
            .map(Room::info)
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

    pub fn handle(&mut self, cmd: Command) {
        if cmd.client().is_some_and(|conn| !self.senders.contains_key(&conn)) {
            return;
        }
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
                last_disconnect_reason,
            } => self.on_hello(conn, token, user_id, username, last_disconnect_reason.as_deref()),
            Command::Unregister { conn } => self.drop_connection(conn),
            Command::RequestRoomList { conn } => self.send_room_list_to(conn),
            Command::CreateRoom { conn, name } => self.create_room(conn, &name),
            Command::JoinRoom { conn, id } => self.request_join(conn, id),
            Command::LeaveRoom { conn } => {
                self.leave_current(conn);
                self.send_room_list_to(conn);
            }
            Command::SetSetting { conn, index, dir } => self.set_setting(conn, index, dir),
            Command::ToggleCountdown { conn } => self.toggle_countdown(conn),
            Command::ReturnToLobby { conn } => self.return_to_lobby(conn),
            Command::Input { conn, kind, seq, tick } => self.on_input(conn, kind, seq, tick),
            Command::TogglePause { conn } => self.toggle_pause(conn),
            Command::Restart { conn } => self.restart(conn),
            Command::InviteFriend { conn, target_user_id } => self.request_invite(conn, &target_user_id),
            Command::FriendCheckDone { check, friends } => self.friend_check_done(check, friends),
            Command::Shutdown => self.begin_shutdown(),
        }
    }

    fn on_hello(
        &mut self,
        conn: ConnId,
        token: Token,
        user_id: Option<Uuid>,
        username: Option<String>,
        last_disconnect_reason: Option<&str>,
    ) {
        if let Some(reason) = last_disconnect_reason {
            warn!("WS {conn} reconnecte après coupure client: {reason}");
        }
        let room = self.room_of_token(&token);
        self.conn_token.insert(conn, token);
        if let Some(uid) = user_id {
            self.conn_user_id.insert(conn, uid);
        }
        if let Some(name) = username {
            self.conn_username.insert(conn, name);
        }
        if let Some(id) = room {
            self.rejoin(conn, id);
        } else {
            self.send_room_list_to(conn);
        }
        if self.closing {
            self.deliver_msg(conn, &ServerMessage::Maintenance);
        }
    }

    fn create_room(&mut self, conn: ConnId, name: &str) {
        let token = match self.conn_token.get(&conn) {
            Some(t) => t.clone(),
            None => return,
        };
        if self.closing {
            self.join_failed(conn, JOIN_MAINTENANCE);
            return;
        }
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

    fn set_setting(&mut self, conn: ConnId, index: u8, dir: i32) {
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

    fn toggle_countdown(&mut self, conn: ConnId) {
        let closing = self.closing;
        if let Some(id) = self.room_of(conn) {
            let toggled = self
                .with_room(id, |room| {
                    if room.is_host_conn(conn) {
                        room.phase = match room.phase {
                            Phase::Lobby if !closing && room.members.len() >= 2 && room.all_connected() => {
                                Phase::CountingDown(3.0)
                            }
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

    fn return_to_lobby(&mut self, conn: ConnId) {
        let Some(id) = self.room_of(conn) else { return };
        let host_slot = self
            .rooms
            .get(&id)
            .filter(|room| room.is_host_conn(conn))
            .and_then(|room| room.slot_of_conn(conn));
        let Some(slot) = host_slot else { return };
        self.record_forfeit(id, slot, "l'hôte a interrompu la partie");
        if let Some(room) = self.rooms.get_mut(&id) {
            room.phase = Phase::Lobby;
            room.sim.finished = false;
            room.sim.paused = false;
        }
        self.send_lobby(id);
        self.room_list_dirty = true;
    }

    fn on_input(&mut self, conn: ConnId, kind: InputKind, seq: u32, tick: u32) {
        if let Some(id) = self.room_of(conn) {
            self.with_room(id, |room| {
                let Some(idx) = room.slot_of_conn(conn) else {
                    return;
                };
                if matches!(room.phase, Phase::Playing) && !room.sim.paused && !room.sim.finished {
                    room.sim.queue_input(idx, tick, seq, kind);
                } else {
                    room.sim.last_seq[idx] = seq;
                }
            });
        }
    }

    fn toggle_pause(&mut self, conn: ConnId) {
        if let Some(id) = self.room_of(conn) {
            let toggled = self
                .with_room(id, |room| {
                    let allowed = room.game_running()
                        && room.all_connected()
                        && room.settings.pause.allows(room.is_host_conn(conn));
                    if allowed {
                        room.sim.set_paused(!room.sim.paused);
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

    fn restart(&mut self, conn: ConnId) {
        if self.closing {
            return;
        }
        if let Some(id) = self.room_of(conn) {
            let restarted = self
                .with_room(id, |room| {
                    if matches!(room.phase, Phase::Playing) && room.sim.finished && room.all_connected() {
                        let now = Instant::now();
                        let in_cooldown = room
                            .sim
                            .last_restart
                            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(2));
                        if in_cooldown {
                            false
                        } else {
                            room.sim.last_restart = Some(now);
                            room.sim.reset_boards(&room.settings);
                            true
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

    fn friend_check_done(&mut self, check: FriendCheck, friends: bool) {
        self.checks_in_flight.remove(&check.conn());
        match check {
            FriendCheck::Join {
                conn, room, host, from, ..
            } => self.finish_join_check(conn, room, &host, from, friends),
            FriendCheck::Invite { conn, room, target, .. } => {
                self.finish_invite_check(conn, room, target, friends);
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
                room.sim.set_paused(false);
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
        let Some(id) = self.room_of(conn) else {
            return;
        };
        let (notify, in_game) = if let Some(room) = self.rooms.get_mut(&id) {
            let Some(slot) = room.slot_of_conn(conn) else {
                return;
            };
            room.members[slot].conn = None;
            room.members[slot].disconnect_at = Some(Instant::now());
            if matches!(room.phase, Phase::CountingDown(_)) {
                room.phase = Phase::Lobby;
                self.room_list_dirty = true;
            }
            let playing = matches!(room.phase, Phase::Playing);
            let notify = playing && !room.sim.finished;
            if notify {
                room.sim.set_paused(true);
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
        self.checks_in_flight.remove(&conn);
        self.last_invite.remove(&conn);
    }

    pub fn reap_dead(&mut self) {
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
        let Some(id) = self.room_of(conn) else {
            return;
        };
        self.clients.insert(conn, None);
        let Some(slot) = self.rooms.get(&id).and_then(|r| r.slot_of_conn(conn)) else {
            return;
        };
        self.record_forfeit(id, slot, "a quitté la partie");
        self.remove_member(id, slot);
    }

    fn remove_member(&mut self, id: RoomId, slot: usize) {
        let closed = if let Some(room) = self.rooms.get_mut(&id) {
            if slot >= room.members.len() {
                return;
            }
            let removed = room.members.remove(slot);
            if room.members.iter().all(|m| m.conn.is_none()) {
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

    pub fn tick(&mut self, dt: f32, do_broadcast: bool) {
        if self.room_list_dirty && self.last_room_list.elapsed() >= Duration::from_millis(200) {
            self.broadcast_room_list();
            self.room_list_dirty = false;
            self.last_room_list = Instant::now();
        }
        self.expire_grace_periods();

        let mut outgoing: Vec<(ConnId, Vec<u8>)> = Vec::new();
        let mut list_changed = false;
        for room in self.rooms.values_mut() {
            list_changed |= room.tick_countdown(dt, &mut outgoing);
            if let Some(rec) = room.tick_game(do_broadcast, &mut outgoing) {
                self.unsaved_matches.push(rec);
            }
        }

        for (conn, payload) in outgoing {
            self.deliver(conn, payload);
        }
        if list_changed {
            self.room_list_dirty = true;
        }
    }

    fn expire_grace_periods(&mut self) {
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
                self.record_forfeit(id, slot, "jamais revenu après sa déconnexion");
                self.remove_member(id, slot);
            }
        }
    }
}
