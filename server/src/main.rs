mod auth;
mod db;

use std::collections::{HashMap, HashSet};
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

const GRACE: Duration = Duration::from_secs(config::RECONNECT_GRACE_SECS);

const CLIENT_CHAN_CAP: usize = 128;

/// Largest WebSocket message a client may send. Client messages are a few dozen
/// bytes; the library default (64 MiB) would let any peer make the server buffer
/// that much per message before our own decoding even runs.
const MAX_CLIENT_MESSAGE: usize = 64 * 1024;

/// Capacity of the queue feeding the manager loop. A safety net behind the
/// per-connection rate limit: once full, a connection waits to enqueue, which
/// pushes back on that client's socket instead of growing memory without bound.
const CMD_CHAN_CAP: usize = 4096;

/// Sustained message rate one client may send, and the burst it may save up.
/// The fastest legitimate player (DAS speed 5 ms) sends about 200 moves/s. The
/// burst is sized for a network stall: a client keeps sending while its link is
/// stuck, then everything lands at once. 1200 covers ~5 s of the fastest play,
/// so a real player is never cut; a flooder gains one small reserve at most.
const CLIENT_MSG_RATE: f64 = 400.0;
const CLIENT_MSG_BURST: f64 = 1200.0;

/// Over-limit messages tolerated within one second before the client is treated
/// as a flood and disconnected, rather than left to cost work indefinitely.
const FLOOD_DROPS_PER_SEC: u32 = 1000;

/// Minimum delay between two invitations sent by the same connection.
const INVITE_COOLDOWN: Duration = Duration::from_secs(2);

/// Token bucket bounding how fast one client's messages reach the manager loop.
struct RateLimiter {
    tokens: f64,
    last: Instant,
    window_start: Instant,
    drops_in_window: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Allow,
    Drop,
    Disconnect,
}

impl RateLimiter {
    fn new(now: Instant) -> Self {
        Self {
            tokens: CLIENT_MSG_BURST,
            last: now,
            window_start: now,
            drops_in_window: 0,
        }
    }

    fn check(&mut self, now: Instant) -> Verdict {
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * CLIENT_MSG_RATE).min(CLIENT_MSG_BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Verdict::Allow;
        }
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            self.window_start = now;
            self.drops_in_window = 0;
        }
        self.drops_in_window += 1;
        if self.drops_in_window > FLOOD_DROPS_PER_SEC {
            Verdict::Disconnect
        } else {
            Verdict::Drop
        }
    }
}

/// A friendship lookup the manager needs but must never wait for: the loop runs
/// every room at 60 Hz, so blocking it on the database would freeze them all.
/// Queued in `Manager::friend_checks`, run off the loop by `manager_loop`, and
/// answered with `Command::FriendCheckDone`. Each variant records what the
/// world looked like when it was asked, so the answer can be checked against
/// what may have changed while it was pending.
#[derive(Debug, Clone)]
enum FriendCheck {
    /// `conn` asked to join friends-only room `room`, hosted by `host`, while in
    /// room `from`.
    Join {
        conn: ConnId,
        room: RoomId,
        host: Token,
        from: Option<RoomId>,
        joiner: Uuid,
        host_user: Uuid,
    },
    /// `conn` invited user `target` into room `room`.
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
            FriendCheck::Join { conn, .. } | FriendCheck::Invite { conn, .. } => *conn,
        }
    }

    fn users(&self) -> (Uuid, Uuid) {
        match self {
            FriendCheck::Join { joiner, host_user, .. } => (*joiner, *host_user),
            FriendCheck::Invite { inviter, target, .. } => (*inviter, *target),
        }
    }
}

const JOIN_UNAVAILABLE: &str = "Room indisponible";
const JOIN_FRIENDS_ONLY: &str = "Cette room est réservée aux amis de l'hôte.";

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
            connected: self.members.iter().filter(|m| m.conn.is_some()).count() as u8,
            your_slot: (idx + 1) as u8,
            is_host: self.members[idx].token == self.host,
            countdown: match self.phase {
                Phase::CountingDown(t) => Some(t.ceil() as u8),
                _ => None,
            },
        }
    }

    /// A game is under way and has not been decided yet.
    fn game_running(&self) -> bool {
        matches!(self.phase, Phase::Playing) && !self.sim.finished
    }

    /// The result of the current game, won by `winner_slot` (1 or 2).
    fn match_record(&self, winner_slot: u8) -> db::MatchRecord {
        db::MatchRecord {
            duration_secs: self.sim.start.elapsed().as_secs_f64(),
            winner_slot,
            user_ids: [
                self.members.first().and_then(|m| m.user_id),
                self.members.get(1).and_then(|m| m.user_id),
            ],
            max_chain: self.sim.max_chain,
            total_chains: self.sim.total_chains,
            nuisance_sent: self.sim.nuisance_sent,
            all_clears: self.sim.all_clears,
            pieces_placed: self.sim.pieces_placed,
        }
    }

    /// The player in `slot` walks away from a running game: settle it as their
    /// loss. Must be called before that member is removed, while member and board
    /// indices still line up.
    ///
    /// Returns `None` when there is nothing to settle: no game running, no
    /// opponent, or an opponent who is disconnected themselves. That last case is
    /// voided rather than awarded to either side — otherwise leaving during an
    /// opponent's short network blip would hand them a loss they did nothing to
    /// earn. An opponent who never comes back still loses, when their grace
    /// period expires.
    fn forfeit(&mut self, slot: usize) -> Option<db::MatchRecord> {
        if !self.game_running() || self.members.len() != 2 || slot > 1 {
            return None;
        }
        let winner = 1 - slot;
        // Opponent disconnected themselves: void, see above.
        self.members[winner].conn?;
        self.sim.finished = true;
        Some(self.match_record((winner + 1) as u8))
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
    /// Friendship lookups to run off the loop. See [`FriendCheck`].
    friend_checks: Vec<FriendCheck>,
    /// Connections with a lookup pending: at most one each, so a client cannot
    /// turn repeated requests into a stream of database queries.
    checks_in_flight: HashSet<ConnId>,
    last_invite: HashMap<ConnId, Instant>,
    /// Decided games waiting to be written to the database: natural game ends
    /// from `tick`, and forfeits from commands or grace expiry. Drained by the
    /// manager loop, which saves them off the game loop.
    unsaved_matches: Vec<db::MatchRecord>,
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
    /// Answer to a queued [`FriendCheck`], sent back by the task that ran it.
    FriendCheckDone {
        check: FriendCheck,
        friends: bool,
    },
}

impl Command {
    /// The client connection this command speaks for, or `None` for the ones that
    /// open a connection or come from the server itself. Deliberately exhaustive:
    /// a new command has to decide which it is.
    fn client(&self) -> Option<ConnId> {
        match self {
            Command::Register { .. } | Command::FriendCheckDone { .. } => None,
            Command::Unregister { conn }
            | Command::Hello { conn, .. }
            | Command::RequestRoomList { conn }
            | Command::CreateRoom { conn, .. }
            | Command::JoinRoom { conn, .. }
            | Command::LeaveRoom { conn }
            | Command::SetSetting { conn, .. }
            | Command::ToggleCountdown { conn }
            | Command::ReturnToLobby { conn }
            | Command::Input { conn, .. }
            | Command::TogglePause { conn }
            | Command::Restart { conn }
            | Command::InviteFriend { conn, .. } => Some(*conn),
        }
    }
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
    fn new() -> Manager {
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
            friend_checks: Vec::new(),
            checks_in_flight: HashSet::new(),
            last_invite: HashMap::new(),
            unsaved_matches: Vec::new(),
        }
    }

    fn room_of(&self, conn: ConnId) -> Option<RoomId> {
        self.clients.get(&conn).copied().flatten()
    }

    /// Settles the running game in room `id` as a loss for `slot` and queues the
    /// result for saving, if there is one to settle. See [`Room::forfeit`].
    fn record_forfeit(&mut self, id: RoomId, slot: usize, why: &str) {
        if let Some(rec) = self.rooms.get_mut(&id).and_then(|r| r.forfeit(slot)) {
            info!("Forfait room #{id} : slot {} perd ({why})", slot + 1);
            self.unsaved_matches.push(rec);
        }
    }

    fn take_unsaved_matches(&mut self) -> Vec<db::MatchRecord> {
        std::mem::take(&mut self.unsaved_matches)
    }

    fn take_friend_checks(&mut self) -> Vec<FriendCheck> {
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

    /// Validates a join and either completes it, or — for a friends-only room —
    /// queues the friendship lookup and completes it when the answer comes back.
    fn request_join(&mut self, conn: ConnId, id: RoomId) {
        if !self.conn_token.contains_key(&conn) {
            return;
        }
        // Already here. Going through `leave_current` would empty the room
        // first — deleting it outright when we are its only member.
        if self.room_of(conn) == Some(id) {
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
            // Guests have no friends to check against.
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

    /// Moves `conn` into room `id`, if it still has a free seat.
    fn complete_join(&mut self, conn: ConnId, id: RoomId) {
        let Some(token) = self.conn_token.get(&conn).cloned() else {
            return;
        };
        if !self.rooms.get(&id).is_some_and(|r| r.members.len() < 2) {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        }
        self.leave_current(conn);
        let user_id = self.conn_user_id.get(&conn).copied();
        // Leaving cannot close the target room: it is not the one we were in.
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

    fn finish_join_check(&mut self, conn: ConnId, room: RoomId, host: Token, from: Option<RoomId>, friends: bool) {
        // Superseded: the player left, or went somewhere else while we waited.
        if !self.senders.contains_key(&conn) || self.room_of(conn) != from {
            return;
        }
        let Some(r) = self.rooms.get(&room) else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if r.settings.friends_only {
            if r.host != host {
                // The host changed while we waited: the answer is about the wrong
                // person. Ask again about the current one.
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

    /// Invitations go to friends only, one per `INVITE_COOLDOWN` per connection:
    /// otherwise anyone could push banners at any user id they have seen.
    fn request_invite(&mut self, conn: ConnId, target: &str) {
        let Some(inviter) = self.conn_user_id.get(&conn).copied() else {
            return;
        };
        let Some(room) = self.room_of(conn) else { return };
        let Ok(target) = Uuid::parse_str(target) else { return };
        // Offline targets have nobody to deliver to: skip the lookup entirely.
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
        // Not friends, or the inviter has since left that room.
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
        // A connection exists from its Register to its Unregister. A command from
        // outside that window — one its socket queued just before dying, landing
        // after the Unregister — must not bring it back: a late Hello would bind
        // a seat to a dead connection, clear its grace timer and resume the game
        // against nobody.
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
            } => {
                if let Some(reason) = last_disconnect_reason {
                    warn!("WS {conn} reconnecte après coupure client : {reason}");
                }
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
            Command::JoinRoom { conn, id } => self.request_join(conn, id),
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
                                    // A seat held for a disconnected player counts
                                    // towards `members`, so check connections too:
                                    // otherwise the game starts without them and
                                    // their board runs out into a recorded loss.
                                    Phase::Lobby if room.members.len() >= 2 && room.all_connected() => {
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
            Command::ReturnToLobby { conn } => {
                let Some(id) = self.room_of(conn) else { return };
                let host_slot = self
                    .rooms
                    .get(&id)
                    .filter(|room| room.is_host_conn(conn))
                    .and_then(|room| room.slot_of_conn(conn));
                let Some(slot) = host_slot else { return };
                // Pulling everyone out of an undecided game is the host abandoning it.
                self.record_forfeit(id, slot, "l'hôte a interrompu la partie");
                if let Some(room) = self.rooms.get_mut(&id) {
                    room.phase = Phase::Lobby;
                    room.sim.finished = false;
                    room.sim.paused = false;
                }
                self.send_lobby(id);
                self.room_list_dirty = true;
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
                            // A disconnect pause ends when the player comes back.
                            // Letting the other one lift it would run the absent
                            // player's board out into a loss.
                            let allowed = room.game_running()
                                && room.all_connected()
                                && room.settings.pause.allows(room.is_host_conn(conn));
                            if allowed {
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
                            // Only a decided game may be restarted: restarting a
                            // running one would discard it without a result. And
                            // only with both players here, like the countdown:
                            // otherwise the new game runs against an absent player.
                            if matches!(room.phase, Phase::Playing) && room.sim.finished && room.all_connected() {
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
            Command::InviteFriend { conn, target_user_id } => self.request_invite(conn, &target_user_id),
            Command::FriendCheckDone { check, friends } => {
                self.checks_in_flight.remove(&check.conn());
                match check {
                    FriendCheck::Join {
                        conn, room, host, from, ..
                    } => self.finish_join_check(conn, room, host, from, friends),
                    FriendCheck::Invite { conn, room, target, .. } => {
                        self.finish_invite_check(conn, room, target, friends)
                    }
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
            if matches!(room.phase, Phase::CountingDown(_)) {
                // Otherwise the game starts without them (see ToggleCountdown).
                room.phase = Phase::Lobby;
                self.room_list_dirty = true;
            }
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
        self.checks_in_flight.remove(&conn);
        self.last_invite.remove(&conn);
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
        self.record_forfeit(id, slot, "a quitté la partie");
        self.remove_member(id, slot);
    }

    fn remove_member(&mut self, id: RoomId, slot: usize) {
        let closed = if let Some(room) = self.rooms.get_mut(&id) {
            if slot >= room.members.len() {
                return;
            }
            let removed = room.members.remove(slot);
            // Only seats held for disconnected players remain: nobody here can
            // play or host, yet the room would stay listed and joinable, leaving a
            // newcomer stuck under a ghost host until the grace ran out. Close it;
            // those players land on the room list if they reconnect.
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

    fn tick(&mut self, dt: f32, do_broadcast: bool) {
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
                self.record_forfeit(id, slot, "jamais revenu après sa déconnexion");
                self.remove_member(id, slot);
            }
        }

        let mut outgoing: Vec<(ConnId, Vec<u8>)> = Vec::new();
        let mut list_changed = false;

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
                        let rec = room.match_record(winner_slot);
                        info!(
                            "Match terminé room #{} → slot {winner_slot} gagne ({:.0}s)",
                            room.id, rec.duration_secs
                        );
                        self.unsaved_matches.push(rec);
                    }
                }
                if (do_broadcast && advanced) || just_finished {
                    let baseline = room.sim.last_sent_piece_id;
                    let pid = [room.sim.boards[0].piece_id, room.sim.boards[1].piece_id];
                    let rng_if_changed = |i: usize| {
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

/// Runs everything the manager queued that needs the database, off the game
/// loop: the manager itself holds no pool, so it cannot stall a tick on I/O.
fn run_side_effects(mgr: &mut Manager, pool: &db::DbPool, cmd_tx: &mpsc::Sender<Command>) {
    for rec in mgr.take_unsaved_matches() {
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = db::record_match_result(&pool, rec).await {
                error!("Match save: {e}");
            }
        });
    }
    for check in mgr.take_friend_checks() {
        let (pool, cmd_tx) = (pool.clone(), cmd_tx.clone());
        tokio::spawn(async move {
            let (a, b) = check.users();
            let friends = db::are_friends(&pool, a, b).await.unwrap_or_else(|e| {
                error!("Friend check: {e}");
                false
            });
            // Only fails if the manager is gone, i.e. the server is shutting down.
            let _ = cmd_tx.send(Command::FriendCheckDone { check, friends }).await;
        });
    }
}

async fn manager_loop(mut cmd_rx: mpsc::Receiver<Command>, cmd_tx: mpsc::Sender<Command>, pool: db::DbPool) {
    let tick_dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let mut ticker = interval(Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64));
    let broadcast_period = Duration::from_secs_f64(1.0 / config::STATE_BROADCAST_HZ as f64);
    let mut mgr = Manager::new();
    let mut last_broadcast = Instant::now();
    let mut profile = TickProfile::new();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let do_broadcast = last_broadcast.elapsed() >= broadcast_period;
                if do_broadcast { last_broadcast = Instant::now(); }
                let t0 = Instant::now();
                mgr.tick(tick_dt, do_broadcast);
                profile.record(t0.elapsed(), mgr.rooms.len());
                mgr.reap_dead();
                run_side_effects(&mut mgr, &pool, &cmd_tx);
            }
            Some(cmd) = cmd_rx.recv() => {
                // Commands queue work too: a forfeit to save, a friendship to check.
                mgr.handle(cmd);
                mgr.reap_dead();
                run_side_effects(&mut mgr, &pool, &cmd_tx);
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

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(CMD_CHAN_CAP);
    tokio::spawn(manager_loop(cmd_rx, cmd_tx.clone(), pool.clone()));

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

    let routes = ws_route(cmd_tx, pool.clone())
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

/// The `/ws` endpoint. Split out of `main` so tests can drive it in-process.
fn ws_route(
    cmd_tx: mpsc::Sender<Command>,
    pool: db::DbPool,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let conn_counter = Arc::new(AtomicU64::new(1));
    warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || cmd_tx.clone()))
        .and(warp::any().map(move || conn_counter.clone()))
        .and(warp::any().map(move || pool.clone()))
        .map(|ws: warp::ws::Ws, cmd_tx, counter: Arc<AtomicU64>, pool: db::DbPool| {
            let conn = counter.fetch_add(1, Ordering::Relaxed);
            ws.max_message_size(MAX_CLIENT_MESSAGE)
                .max_frame_size(MAX_CLIENT_MESSAGE)
                .on_upgrade(move |socket| handle_connection(socket, cmd_tx, conn, pool))
        })
}

async fn handle_connection(ws: warp::ws::WebSocket, cmd_tx: mpsc::Sender<Command>, conn: ConnId, pool: db::DbPool) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    info!("WS {conn} ouverture");

    let _ = cmd_tx
        .send(Command::Register {
            conn,
            sender: to_client_tx,
        })
        .await;

    let mut send_task = tokio::spawn(async move {
        while let Some(payload) = to_client_rx.recv().await {
            if user_ws_tx.send(warp::ws::Message::binary(payload)).await.is_err() {
                break;
            }
        }
    });

    let cmd_tx_recv = cmd_tx.clone();
    let mut recv_task = tokio::spawn(async move {
        let mut limiter = RateLimiter::new(Instant::now());
        // Hello does a database lookup and the client sends it once per socket:
        // later ones are ignored, so they cannot become a query stream.
        let mut greeted = false;
        while let Some(result) = user_ws_rx.next().await {
            // Counted before any decoding, so over-limit frames cost nothing more.
            match limiter.check(Instant::now()) {
                Verdict::Allow => {}
                Verdict::Drop => continue,
                Verdict::Disconnect => {
                    warn!("WS {conn} flood, fermeture");
                    break;
                }
            }
            if let Ok(msg) = result {
                if msg.is_binary() {
                    if let Some(client_msg) = shared::decode::<ClientMessage>(msg.as_bytes()) {
                        let cmd = match client_msg {
                            ClientMessage::Hello { .. } if greeted => continue,
                            ClientMessage::Hello {
                                player_id,
                                auth_token,
                                username,
                                last_disconnect_reason,
                            } => {
                                greeted = true;
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
                                    last_disconnect_reason,
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
                        // Awaits when the queue is full: back-pressure lands on this
                        // client's socket, not on memory.
                        if cmd_tx_recv.send(cmd).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    // Awaited, never dropped: a lost Unregister would leave the seat bound to a
    // dead connection forever.
    let _ = cmd_tx.send(Command::Unregister { conn }).await;
    info!("WS {conn} fermeture");
}

#[cfg(test)]
mod tests;
