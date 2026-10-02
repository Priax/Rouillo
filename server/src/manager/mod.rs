use std::collections::{HashMap, HashSet};

use shared::{config, InputKind, PausePolicy, RoomId, RoomInfo, RoomSettings, ServerMessage};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::room::{Member, Phase, Room, Series, SeriesResult};
use crate::{db, ConnId, Token};

pub mod friends;
mod identity;
mod presence;
pub mod ranked;
mod rooms;

use friends::FriendCheck;
use ranked::{PendingMatch, QueueEntry, RankedCheck};

pub const COUNTDOWN_SECS: f32 = 3.0;
pub const GRACE: Duration = Duration::from_secs(config::RECONNECT_GRACE_SECS);

const MAX_PLAYER_ID: usize = 64;

const JOIN_UNAVAILABLE: &str = "Room indisponible";
const JOIN_FRIENDS_ONLY: &str = "Cette room est réservée aux amis de l'hôte.";
const JOIN_MAINTENANCE: &str = "Le serveur redémarre, réessayez dans un instant.";
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
    unsaved_series: Vec<db::SeriesRecord>,
    user_conns: HashMap<Uuid, HashSet<ConnId>>,
    conn_session: HashMap<ConnId, Uuid>,
    pub queue: Vec<QueueEntry>,
    pub pending_matches: Vec<PendingMatch>,
    queue_cooldowns: HashMap<Uuid, Instant>,
    ranked_checks: Vec<RankedCheck>,
    ranked_checking: HashSet<ConnId>,
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
        session: Option<Uuid>,
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
    JoinQueue {
        conn: ConnId,
    },
    LeaveQueue {
        conn: ConnId,
    },
    AcceptMatch {
        conn: ConnId,
    },
    RankedCheckDone {
        check: RankedCheck,
        profile: Option<(i32, i64)>,
    },
    Revoke {
        user_id: Uuid,
        keep: Option<Uuid>,
    },
    Shutdown,
}

impl Command {
    fn client(&self) -> Option<ConnId> {
        match self {
            Self::Register { .. }
            | Self::FriendCheckDone { .. }
            | Self::RankedCheckDone { .. }
            | Self::Revoke { .. }
            | Self::Shutdown => None,
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
            | Self::InviteFriend { conn, .. }
            | Self::JoinQueue { conn }
            | Self::LeaveQueue { conn }
            | Self::AcceptMatch { conn } => Some(*conn),
        }
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
            unsaved_series: Vec::new(),
            user_conns: HashMap::new(),
            conn_session: HashMap::new(),
            queue: Vec::new(),
            pending_matches: Vec::new(),
            queue_cooldowns: HashMap::new(),
            ranked_checks: Vec::new(),
            ranked_checking: HashSet::new(),
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
        for entry in std::mem::take(&mut self.queue) {
            self.refuse_queue(entry.conn, JOIN_MAINTENANCE);
        }
        self.cancel_pending_for_maintenance();
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

    pub fn take_unsaved_matches(&mut self) -> Vec<db::MatchRecord> {
        std::mem::take(&mut self.unsaved_matches)
    }

    fn join_failed(&mut self, conn: ConnId, reason: &str) {
        self.deliver_msg(
            conn,
            &ServerMessage::JoinFailed {
                reason: reason.to_string(),
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
        let Some(msgs) = self.rooms.get(&id).map(Room::lobby_payloads) else {
            return;
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
            .filter(|r| !r.settings.friends_only && r.series.is_none())
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
                session,
                last_disconnect_reason,
            } => {
                if let Some(reason) = last_disconnect_reason {
                    warn!("WS {conn} reconnecte après coupure client: {reason}");
                }
                self.set_identity(conn, user_id, username, session);
                self.on_hello(conn, token);
            }
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
            Command::JoinQueue { conn } => self.join_queue(conn),
            Command::LeaveQueue { conn } => self.leave_queue(conn),
            Command::AcceptMatch { conn } => self.accept_match(conn),
            Command::RankedCheckDone { check, profile } => self.ranked_check_done(check, profile),
            Command::Revoke { user_id, keep } => self.revoke(user_id, keep),
            Command::Shutdown => self.begin_shutdown(),
        }
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

    pub fn tick(&mut self, dt: f32, do_broadcast: bool) {
        if self.room_list_dirty && self.last_room_list.elapsed() >= Duration::from_millis(200) {
            self.broadcast_room_list();
            self.room_list_dirty = false;
            self.last_room_list = Instant::now();
        }
        self.expire_grace_periods();

        let mut outgoing: Vec<(ConnId, Vec<u8>)> = Vec::new();
        let mut list_changed = false;
        let mut games_won: Vec<(RoomId, Option<usize>)> = Vec::new();
        for room in self.rooms.values_mut() {
            list_changed |= room.tick_countdown(dt, &mut outgoing);
            if let Some(rec) = room.tick_game(do_broadcast, &mut outgoing) {
                if room.series.is_some() {
                    games_won.push((room.id, rec.winner_slot.map(|w| usize::from(w) - 1)));
                }
                self.unsaved_matches.push(rec);
            }
        }

        for (conn, payload) in outgoing {
            self.deliver(conn, payload);
        }
        if list_changed {
            self.room_list_dirty = true;
        }
        for (id, winner) in games_won {
            self.series_game_won(id, winner);
        }
        self.advance_series();
        self.expire_matches();
        self.matchmake();
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
                self.forfeit_series(id, slot);
                self.remove_member(id, slot);
            }
        }
    }
}
