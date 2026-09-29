mod auth;
mod db;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use rand::RngExt;
use shared::{
    config, Board, ClientMessage, GameState, IncomingGarbage, InputKind, LobbyInfo, RngPosition, RoomId, RoomInfo,
    RoomSettings, ServerMessage,
};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;
use warp::{Filter, Reply};

type ConnId = u64;
type Token = String;

const GRACE: Duration = Duration::from_secs(config::RECONNECT_GRACE_SECS);

const CLIENT_CHAN_CAP: usize = 128;

const CLIENT_SILENCE_TIMEOUT: Duration = Duration::from_secs(300);

const _: () = assert!(CLIENT_SILENCE_TIMEOUT.as_secs() as f64 > 10.0 * config::PING_INTERVAL_SECS);

const MAX_CLIENT_MESSAGE: usize = 64 * 1024;

const CMD_CHAN_CAP: usize = 4096;

const CLIENT_MSG_RATE: f64 = 400.0;
const CLIENT_MSG_BURST: f64 = 1200.0;

const FLOOD_DROPS_PER_SEC: u32 = 1000;

const INVITE_COOLDOWN: Duration = Duration::from_secs(2);

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

#[derive(Debug, Clone)]
enum FriendCheck {
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

    fn users(&self) -> (Uuid, Uuid) {
        match self {
            Self::Join { joiner, host_user, .. } => (*joiner, *host_user),
            Self::Invite { inviter, target, .. } => (*inviter, *target),
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

const INPUT_QUEUE_CAP: usize = 2048;

const _: () = assert!(INPUT_QUEUE_CAP as f64 > CLIENT_MSG_BURST);

struct GarbageDelivery {
    at: u32,
    slot: usize,
    amount: u32,
}

struct PendingInput {
    at: u32,
    seq: u32,
    kind: InputKind,
}

struct Sim {
    boards: [Board; 2],
    tick: u32,
    queued_inputs: [Vec<PendingInput>; 2],
    garbage_in_flight: Vec<GarbageDelivery>,
    late_inputs: [u32; 2],
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
    last_sent_rng: Option<[RngPosition; 2]>,
}

impl Sim {
    /// Updates the per-player match statistics after a step.
    fn record_stats(&mut self) {
        for i in 0..2 {
            let cc = self.boards[i].chain_count;
            if cc > 0 && self.prev_chain[i] == 0 {
                self.total_chains[i] += 1;
            }
            self.max_chain[i] = self.max_chain[i].max(cc);
            self.prev_chain[i] = cc;

            let ac = self.boards[i].last_was_all_clear;
            if ac && !self.prev_all_clear[i] {
                self.all_clears[i] += 1;
            }
            self.prev_all_clear[i] = ac;

            let pid = self.boards[i].piece_id;
            if pid != self.prev_piece_id[i] {
                self.pieces_placed[i] += 1;
                self.prev_piece_id[i] = pid;
            }
        }
    }

    fn new(settings: &RoomSettings) -> Self {
        Self {
            boards: Self::fresh_boards(settings),
            tick: 0,
            queued_inputs: [Vec::new(), Vec::new()],
            garbage_in_flight: Vec::new(),
            late_inputs: [0; 2],
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
            last_sent_rng: None,
        }
    }

    fn queue_input(&mut self, slot: usize, tick: u32, seq: u32, kind: InputKind) {
        if self.queued_inputs[slot].len() >= INPUT_QUEUE_CAP {
            warn!("File d'inputs pleine (slot {slot}), input {seq} abandonné");
            return;
        }
        if tick < self.tick {
            self.late_inputs[slot] += 1;
        }
        let at = tick.clamp(self.tick, self.tick.saturating_add(config::MAX_INPUT_LEAD_TICKS));
        self.queued_inputs[slot].push(PendingInput { at, seq, kind });
    }

    fn send_garbage(&mut self, from: usize, amount: u32, at: u32) {
        debug_assert!(from < 2, "slot {from} does not exist");
        if amount == 0 {
            return;
        }
        self.garbage_in_flight.push(GarbageDelivery {
            at,
            slot: 1 - from,
            amount,
        });
    }

    fn take_due_garbage(&mut self, slot: usize) -> u32 {
        let now = self.tick;
        let mut landed = 0;
        let mut i = 0;
        while i < self.garbage_in_flight.len() {
            let g = &self.garbage_in_flight[i];
            if g.slot == slot && g.at <= now {
                landed += g.amount;
                self.nuisance_sent[1 - slot] += g.amount;
                self.garbage_in_flight.swap_remove(i);
            } else {
                i += 1;
            }
        }
        landed
    }

    fn incoming(&self, slot: usize) -> Vec<IncomingGarbage> {
        self.garbage_in_flight
            .iter()
            .filter(|g| g.slot == slot)
            .map(|g| IncomingGarbage {
                at: g.at,
                amount: g.amount,
            })
            .collect()
    }

    fn take_due_inputs(&mut self, slot: usize) -> Vec<InputKind> {
        let now = self.tick;
        let queue = &mut self.queued_inputs[slot];
        let due = queue.iter().position(|p| p.at > now).unwrap_or(queue.len());
        if let Some(last) = queue[..due].last() {
            self.last_seq[slot] = last.seq;
        }
        queue.drain(..due).map(|p| p.kind).collect()
    }

    fn advance(&mut self) {
        self.tick += 1;
        let at = self.tick + config::GARBAGE_TRAVEL_TICKS;
        let mut produced = [0; 2];
        for (slot, sent) in produced.iter_mut().enumerate() {
            let inputs = self.take_due_inputs(slot);
            let landed = self.take_due_garbage(slot);
            *sent = self.boards[slot].step(inputs, landed);
        }
        for (slot, sent) in produced.into_iter().enumerate() {
            self.send_garbage(slot, sent, at);
        }
    }

    fn rng_positions(&self) -> [RngPosition; 2] {
        [self.boards[0].rng_position(), self.boards[1].rng_position()]
    }

    fn state_update(&self, full_rng: bool) -> ServerMessage {
        let pos = self.rng_positions();
        let rng = |i: usize| {
            let changed = full_rng || self.last_sent_rng.is_none_or(|sent| sent[i] != pos[i]);
            changed.then(|| Box::new(self.boards[i].rng_state()))
        };
        ServerMessage::StateUpdate {
            p1_board: Box::new(self.boards[0].clone()),
            p2_board: Box::new(self.boards[1].clone()),
            p1_rng: rng(0),
            p2_rng: rng(1),
            p1_ack: self.last_seq[0],
            p2_ack: self.last_seq[1],
            tick: self.tick,
            p1_incoming: self.incoming(0),
            p2_incoming: self.incoming(1),
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
        self.tick = 0;
        self.queued_inputs = [Vec::new(), Vec::new()];
        self.garbage_in_flight.clear();
        self.late_inputs = [0; 2];
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
        self.last_sent_rng = None;
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
    fn send_to_members(&self, payload: &[u8], outgoing: &mut Vec<(ConnId, Vec<u8>)>) {
        for m in &self.members {
            if let Some(c) = m.conn {
                outgoing.push((c, payload.to_vec()));
            }
        }
    }

    /// Returns whether the room list changed (the game started).
    fn tick_countdown(&mut self, dt: f32, outgoing: &mut Vec<(ConnId, Vec<u8>)>) -> bool {
        let Phase::CountingDown(t) = &mut self.phase else {
            return false;
        };
        let before = t.ceil() as u8;
        *t -= dt;
        if *t <= 0.0 {
            self.phase = Phase::Playing;
            self.sim.reset_boards(&self.settings);
            match shared::encode(&ServerMessage::GameStart) {
                Ok(payload) => self.send_to_members(&payload, outgoing),
                Err(e) => error!("encode GameStart failed: {e}"),
            }
            return true;
        }
        if t.ceil() as u8 != before {
            for (i, m) in self.members.iter().enumerate() {
                if let Some(c) = m.conn {
                    let msg = ServerMessage::Lobby {
                        info: self.lobby_info_for(i),
                    };
                    match shared::encode(&msg) {
                        Ok(payload) => outgoing.push((c, payload)),
                        Err(e) => error!("encode Lobby failed: {e}"),
                    }
                }
            }
        }
        false
    }

    /// Advances a running game by one step and sends its state. Returns the
    /// match record when this step ended the game.
    fn tick_game(&mut self, do_broadcast: bool, outgoing: &mut Vec<(ConnId, Vec<u8>)>) -> Option<db::MatchRecord> {
        if !matches!(self.phase, Phase::Playing) {
            return None;
        }
        let advanced = !self.sim.paused && !self.sim.finished;
        let mut record = None;
        if advanced {
            self.sim.advance();
            self.sim.record_stats();
            if self.sim.boards.iter().any(|b| b.state == GameState::GameOver) {
                self.sim.finished = true;
                let winner_slot = if self.sim.boards[0].state == GameState::GameOver
                    && self.sim.boards[1].state != GameState::GameOver
                {
                    2u8
                } else {
                    1u8
                };
                let rec = self.match_record(winner_slot);
                info!(
                    "Match terminé room #{} → slot {winner_slot} gagne ({:.0}s, inputs en retard {:?})",
                    self.id, rec.duration_secs, self.sim.late_inputs
                );
                record = Some(rec);
            }
        }
        let just_finished = record.is_some();
        if (do_broadcast && advanced) || just_finished {
            let msg = self.sim.state_update(just_finished);
            self.sim.last_sent_rng = Some(self.sim.rng_positions());
            match shared::encode(&msg) {
                Ok(upd) => self.send_to_members(&upd, outgoing),
                Err(e) => error!("encode StateUpdate failed: {e}"),
            }
        }
        record
    }

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

    fn game_running(&self) -> bool {
        matches!(self.phase, Phase::Playing) && !self.sim.finished
    }

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

    fn forfeit(&mut self, slot: usize) -> Option<db::MatchRecord> {
        if !self.game_running() || self.members.len() != 2 || slot > 1 {
            return None;
        }
        let winner = 1 - slot;
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
    friend_checks: Vec<FriendCheck>,
    checks_in_flight: HashSet<ConnId>,
    last_invite: HashMap<ConnId, Instant>,
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
}

impl Command {
    fn client(&self) -> Option<ConnId> {
        match self {
            Self::Register { .. } | Self::FriendCheckDone { .. } => None,
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
    fn new() -> Self {
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
        }
    }

    fn room_of(&self, conn: ConnId) -> Option<RoomId> {
        self.clients.get(&conn).copied().flatten()
    }

    fn record_forfeit(&mut self, id: RoomId, slot: usize, why: &str) {
        if let Some(rec) = self.rooms.get_mut(&id).and_then(|r| r.forfeit(slot)) {
            info!("Forfait room #{id}: slot {} perd ({why})", slot + 1);
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

    fn request_join(&mut self, conn: ConnId, id: RoomId) {
        if !self.conn_token.contains_key(&conn) {
            return;
        }
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
        if self.rooms.get(&id).is_none_or(|r| r.members.len() >= 2) {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        }
        self.leave_current(conn);
        let user_id = self.conn_user_id.get(&conn).copied();
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

    fn send_snapshot(&mut self, id: RoomId) {
        let msg = match self.rooms.get(&id) {
            Some(room) => room.sim.state_update(true),
            None => return,
        };
        self.send_room_msg(id, &msg);
    }

    fn public_room_list(&self) -> Vec<RoomInfo> {
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

    fn handle(&mut self, cmd: Command) {
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
    }

    fn create_room(&mut self, conn: ConnId, name: &str) {
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
        if let Some(id) = self.room_of(conn) {
            let toggled = self
                .with_room(id, |room| {
                    if room.is_host_conn(conn) {
                        room.phase = match room.phase {
                            Phase::Lobby if room.members.len() >= 2 && room.all_connected() => Phase::CountingDown(3.0),
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

    fn restart(&mut self, conn: ConnId) {
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

    fn tick(&mut self, dt: f32, do_broadcast: bool) {
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

struct TickProfile {
    enabled: bool,
    budget: Duration,
    sum: Duration,
    max: Duration,
    count: u32,
    since_report: Instant,
}

impl TickProfile {
    fn new() -> Self {
        Self {
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
                Ok(n) if n > 0 => info!("Sessions expirées: {n} supprimées"),
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

    warp::serve(routes)
        .incoming(bind_listener((config::SERVER_BIND_ADDRESS, port).into()))
        .run()
        .await;
}

fn bind_listener(addr: SocketAddr) -> tokio::net::TcpListener {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP)).expect("socket()");
    socket.set_tcp_nodelay(true).expect("TCP_NODELAY");
    socket.set_reuse_address(true).expect("SO_REUSEADDR");
    socket.set_nonblocking(true).expect("O_NONBLOCK");
    socket.bind(&addr.into()).expect("bind");
    socket.listen(1024).expect("listen");
    tokio::net::TcpListener::from_std(socket.into()).expect("listener")
}

fn ws_route(
    cmd_tx: mpsc::Sender<Command>,
    pool: db::DbPool,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let conn_counter = Arc::new(AtomicU64::new(1));
    warp::path("ws")
        .and(warp::ws())
        .and(warp::query::<WsQuery>())
        .and(warp::any().map(move || cmd_tx.clone()))
        .and(warp::any().map(move || Arc::clone(&conn_counter)))
        .and(warp::any().map(move || pool.clone()))
        .map(
            |ws: warp::ws::Ws, query: WsQuery, cmd_tx, counter: Arc<AtomicU64>, pool: db::DbPool| {
                let conn = counter.fetch_add(1, Ordering::Relaxed);
                let ws = ws
                    .max_message_size(MAX_CLIENT_MESSAGE)
                    .max_frame_size(MAX_CLIENT_MESSAGE);
                if query.v == Some(shared::PROTOCOL_VERSION) {
                    ws.on_upgrade(move |socket| handle_connection(socket, cmd_tx, conn, pool))
                        .into_response()
                } else {
                    info!(
                        "WS {conn} refusé: protocole {:?}, attendu {}",
                        query.v,
                        shared::PROTOCOL_VERSION
                    );
                    ws.on_upgrade(reject_outdated).into_response()
                }
            },
        )
}

#[derive(serde::Deserialize)]
struct WsQuery {
    v: Option<u32>,
}

// Close code for an outdated client, in the range RFC 6455 leaves to applications
// (426: HTTP's Upgrade Required).
const CLOSE_OUTDATED: u16 = 4426;

async fn reject_outdated(ws: warp::ws::WebSocket) {
    let (mut tx, _rx) = ws.split();
    let _ = tx.send(warp::ws::Message::text(shared::OUTDATED_FRAME)).await;
    let _ = tx
        .send(warp::ws::Message::close_with(CLOSE_OUTDATED, shared::OUTDATED_FRAME))
        .await;
}

async fn handle_connection(ws: warp::ws::WebSocket, cmd_tx: mpsc::Sender<Command>, conn: ConnId, pool: db::DbPool) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    let pong_tx = to_client_tx.clone();
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
        let mut greeted = false;
        loop {
            let result = match tokio::time::timeout(CLIENT_SILENCE_TIMEOUT, user_ws_rx.next()).await {
                Ok(Some(result)) => result,
                Ok(None) => break,
                Err(_) => {
                    warn!("WS {conn} muet depuis {}s, fermeture", CLIENT_SILENCE_TIMEOUT.as_secs());
                    break;
                }
            };
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
                            ClientMessage::Ping { id } => {
                                if let Ok(bytes) = shared::encode(&ServerMessage::Pong { id }) {
                                    let _ = pong_tx.try_send(bytes);
                                }
                                continue;
                            }
                            ClientMessage::Input { kind, seq, tick } => Command::Input { conn, kind, seq, tick },
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
                        if cmd_tx_recv.send(cmd).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    let _ = cmd_tx.send(Command::Unregister { conn }).await;
    info!("WS {conn} fermeture");
}

#[cfg(test)]
mod tests;
