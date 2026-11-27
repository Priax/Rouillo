use std::collections::{HashMap, HashSet};

use bytes::Bytes;
use rayon::prelude::*;
use shared::cpu::{Difficulty, Style};
use shared::{
    config, ClientMessage, InputKind, PausePolicy, RoomId, RoomInfo, RoomSetting, RoomSettings, ServerMessage,
    Targeting,
};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;

use crate::room::{Member, Phase, Room, Series, SeriesResult, Spectator, StateOut};
use crate::{db, encode, ConnId, Token};

mod client;
pub mod friends;
mod identity;
mod presence;
pub mod ranked;
pub(crate) mod rooms;
pub mod social;

pub use client::{Client, Clients};
use friends::{FriendCheck, Friends};
use ranked::{Ranked, RankedCheck};

/// What the rooms one thread stepped and sent leave to record.
#[derive(Default)]
struct RoomsTicked {
    records: Vec<db::MatchRecord>,
    games_won: Vec<(RoomId, Option<usize>)>,
    dead: Vec<ConnId>,
    list_changed: bool,
}

pub const COUNTDOWN_SECS: f32 = 3.0;
pub const GRACE: Duration = Duration::from_secs(config::RECONNECT_GRACE_SECS);

const MAX_PLAYER_ID: usize = 64;

const JOIN_UNAVAILABLE: &str = "Room indisponible";
const JOIN_FRIENDS_ONLY: &str = "Cette room est réservée aux amis de l'hôte.";
const JOIN_MAINTENANCE: &str = "Le serveur redémarre, réessayez dans un instant.";
const JOIN_SAME_ACCOUNT: &str = "Ce compte est déjà dans cette room.";
const WATCH_NOBODY: &str = "Cet ami ne joue pas en ce moment.";
const WATCH_NOT_FRIENDS: &str = "Seuls les amis peuvent regarder cette partie.";

/// Work for the database, done outside the manager, whose answer comes
/// back as a `Command`.
pub enum Effect {
    SaveMatch(db::MatchRecord),
    SaveSeries(db::SeriesRecord),
    CheckRanked(RankedCheck),
    LoadFriends(Uuid),
    CheckFriends(FriendCheck),
}

pub struct Manager {
    pub rooms: HashMap<RoomId, Room>,
    pub clients: Clients,
    pub ranked: Ranked,
    friends: Friends,
    effects: Vec<Effect>,
    next_id: RoomId,
    room_list_dirty: bool,
    last_room_list: Instant,
    closing: bool,
}

pub enum Command {
    Register {
        conn: ConnId,
        sender: mpsc::Sender<Bytes>,
        states: Option<mpsc::Sender<Vec<Bytes>>>,
    },
    Unregister {
        conn: ConnId,
    },
    /// What a connection sent, past its greeting.
    Client {
        conn: ConnId,
        msg: ClientMessage,
    },
    Hello {
        conn: ConnId,
        token: Token,
        user_id: Option<Uuid>,
        username: Option<String>,
        session: Option<Uuid>,
        last_disconnect_reason: Option<String>,
        background: bool,
    },
    MatchSaved {
        id: Uuid,
        users: Vec<Uuid>,
    },
    FriendCheckDone {
        check: FriendCheck,
        friends: bool,
    },
    FriendsLoaded {
        user: Uuid,
        friends: Vec<Uuid>,
    },
    FriendshipChanged {
        users: (Uuid, Uuid),
        friends: bool,
    },
    /// Which of `users` are in a room, for the friend list.
    Playing {
        users: Vec<Uuid>,
        reply: tokio::sync::oneshot::Sender<Vec<Uuid>>,
    },
    RankedCheckDone {
        check: RankedCheck,
        profile: Option<db::RankedProfile>,
    },
    Revoke {
        user_id: Uuid,
        keep: Option<Uuid>,
    },
    Rename {
        user_id: Uuid,
        username: String,
    },
    Shutdown,
}

impl Command {
    fn client(&self) -> Option<ConnId> {
        match self {
            Self::Register { .. }
            | Self::MatchSaved { .. }
            | Self::FriendCheckDone { .. }
            | Self::FriendsLoaded { .. }
            | Self::FriendshipChanged { .. }
            | Self::RankedCheckDone { .. }
            | Self::Revoke { .. }
            | Self::Rename { .. }
            | Self::Playing { .. }
            | Self::Shutdown => None,
            Self::Unregister { conn } | Self::Hello { conn, .. } | Self::Client { conn, .. } => Some(*conn),
        }
    }
}

impl Manager {
    pub fn new() -> Self {
        Self {
            rooms: HashMap::new(),
            clients: Clients::default(),
            ranked: Ranked::default(),
            friends: Friends::default(),
            effects: Vec::new(),
            next_id: 1,
            room_list_dirty: false,
            last_room_list: Instant::now(),
            closing: false,
        }
    }

    /// What is left for the database to do, in the order it came.
    pub fn effects(&mut self) -> std::vec::Drain<'_, Effect> {
        self.effects.drain(..)
    }

    fn begin_shutdown(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        for room in self.rooms.values_mut().filter(|r| matches!(r.phase, Phase::CountingDown(_))) {
            room.phase = Phase::Lobby;
            self.clients.deliver_many(room.lobby_payloads());
            self.room_list_dirty = true;
        }
        let maintenance = encode(&ServerMessage::Maintenance);
        self.clients.broadcast(|_| Some(maintenance.clone()));
        self.refuse_queue_for_maintenance();
        info!("Maintenance: {} partie(s) en cours à laisser finir", self.games_running());
    }

    pub fn closing(&self) -> bool {
        self.closing
    }

    pub fn games_running(&self) -> usize {
        self.rooms.values().filter(|r| r.game_running()).count()
    }

    fn join_failed(&mut self, conn: ConnId, reason: &str) {
        self.clients.deliver_msg(conn, &ServerMessage::JoinFailed { reason: reason.to_string() });
    }

    fn with_room<R>(&mut self, id: RoomId, f: impl FnOnce(&mut Room) -> R) -> Option<R> {
        self.rooms.get_mut(&id).map(f)
    }

    fn send_room_msg(&mut self, id: RoomId, msg: &ServerMessage) {
        if let Some(room) = self.rooms.get(&id) {
            self.clients.deliver_all(room.audience(), &encode(msg));
        }
    }

    fn send_game_start(&mut self, id: RoomId) {
        if let Some(room) = self.rooms.get_mut(&id) {
            self.clients.deliver_many(room.game_start_payloads());
        }
    }

    fn deliver_game_start(&mut self, id: RoomId, conn: ConnId) {
        if let Some(room) = self.rooms.get_mut(&id) {
            room.restart_client(conn);
            self.clients.deliver_msg(conn, &room.game_start_for(conn));
        }
    }

    fn send_lobby(&mut self, id: RoomId) {
        if let Some(room) = self.rooms.get(&id) {
            self.clients.deliver_many(room.lobby_payloads());
        }
    }

    pub fn send_snapshot(&mut self, id: RoomId) {
        if let Some(room) = self.rooms.get(&id) {
            self.clients.deliver_all(room.audience(), &encode(&room.sim.state_update()));
        }
    }

    fn send_room_list_to(&mut self, conn: ConnId) {
        let rooms = room_list(&self.rooms, &self.friends, self.clients.user_of(conn));
        self.clients.deliver_msg(conn, &ServerMessage::RoomList { rooms });
    }

    /// The public list is encoded once; only those who also see a
    /// friends-only room get a list of their own.
    pub(crate) fn broadcast_room_list(&mut self) {
        let Self { rooms, friends, clients, .. } = self;
        let public = encode(&ServerMessage::RoomList { rooms: public_rooms(rooms).map(Room::info).collect() });
        clients.broadcast(|client| {
            if !client.browsing() {
                return None;
            }
            if private_rooms(rooms, friends, client.user).next().is_none() {
                return Some(public.clone());
            }
            Some(encode(&ServerMessage::RoomList { rooms: room_list(rooms, friends, client.user) }))
        });
    }

    fn room_of_token(&self, token: &str) -> Option<RoomId> {
        self.rooms.values().find(|r| r.members.iter().any(|m| m.token == token)).map(|r| r.id)
    }

    fn on_message(&mut self, conn: ConnId, msg: ClientMessage) {
        match msg {
            ClientMessage::Hello { .. } | ClientMessage::Ping { .. } | ClientMessage::Inputs { .. } => {}
            ClientMessage::RequestRoomList => self.send_room_list_to(conn),
            ClientMessage::ListRooms { on } => {
                if let Some(client) = self.clients.get_mut(conn) {
                    client.lists_rooms = on;
                }
                if on {
                    self.send_room_list_to(conn);
                }
            }
            ClientMessage::CreateRoom { name } => self.create_room(conn, &name),
            ClientMessage::JoinRoom { id } => self.request_join(conn, id),
            ClientMessage::LeaveRoom => {
                self.leave_current(conn);
                self.send_room_list_to(conn);
            }
            ClientMessage::SetRoomSetting { setting, dir } => self.set_setting(conn, setting, dir),
            ClientMessage::ToggleCountdown => self.toggle_countdown(conn),
            ClientMessage::ReturnToLobby => self.return_to_lobby(conn),
            ClientMessage::Input { kind, seq, tick } => self.on_input(conn, kind, seq, tick),
            ClientMessage::TogglePause => self.toggle_pause(conn),
            ClientMessage::RequestRestart => self.restart(conn),
            ClientMessage::InviteFriend { user_id } => self.request_invite(conn, &user_id),
            ClientMessage::JoinQueue => self.join_queue(conn),
            ClientMessage::LeaveQueue => self.leave_queue(conn),
            ClientMessage::AcceptMatch => self.accept_match(conn),
            ClientMessage::Chat { text } => self.chat(conn, &text),
            ClientMessage::Spectate { id } => self.spectate(conn, id),
            ClientMessage::WatchFriend { user_id } => self.watch_friend(conn, &user_id),
            ClientMessage::SetTargeting { targeting } => self.set_targeting(conn, targeting),
            ClientMessage::SetTeam { team } => self.set_team(conn, team),
            ClientMessage::AddCpu => self.add_cpu(conn),
            ClientMessage::SetCpu { slot, difficulty, style, team } => {
                self.set_cpu(conn, slot, difficulty, style, team)
            }
            ClientMessage::RemoveCpu { slot } => self.remove_cpu(conn, slot),
            ClientMessage::Follow { slot } => self.follow(conn, slot),
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        if cmd.client().is_some_and(|conn| !self.clients.contains(conn)) {
            return;
        }
        match cmd {
            Command::Register { conn, sender, states } => self.clients.insert(conn, Client::new(sender, states)),
            Command::Hello { conn, token, user_id, username, session, last_disconnect_reason, background } => {
                if let Some(reason) = last_disconnect_reason {
                    warn!("WS {conn} reconnecte après coupure client: {reason}");
                }
                self.set_identity(conn, user_id, username, session);
                self.on_hello(conn, token, background);
            }
            Command::Unregister { conn } => self.drop_connection(conn),
            Command::Client { conn, msg } => self.on_message(conn, msg),
            Command::MatchSaved { id, users } => self.match_saved(id, &users),
            Command::FriendCheckDone { check, friends } => self.friend_check_done(check, friends),
            Command::FriendsLoaded { user, friends } => self.friends_loaded(user, friends),
            Command::FriendshipChanged { users, friends } => self.friendship_changed(users, friends),
            Command::Playing { users, reply } => {
                let _ = reply.send(self.playing(&users));
            }
            Command::RankedCheckDone { check, profile } => self.ranked_check_done(check, profile),
            Command::Revoke { user_id, keep } => self.revoke(user_id, keep),
            Command::Rename { user_id, username } => self.rename(user_id, &username),
            Command::Shutdown => self.begin_shutdown(),
        }
    }

    /// Drops the connections too slow to keep up, and those that dropping
    /// them finds slow in turn.
    pub fn reap_dead(&mut self) {
        while let Some(conn) = self.clients.next_dead() {
            if self.clients.contains(conn) {
                warn!("WS {conn} trop lente, fermeture");
                self.drop_connection(conn);
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

        let clients = &self.clients;
        let ticked: Vec<RoomsTicked> = self
            .rooms
            .par_iter_mut()
            .fold(RoomsTicked::default, |mut out, (_, room)| {
                let (mut outgoing, mut states) = (Vec::new(), Vec::new());
                out.list_changed |= room.tick_countdown(dt, &mut outgoing);
                let split = |c| clients.splits_states(c);
                if let Some(rec) = room.tick_game(do_broadcast, split, &mut states, &mut outgoing) {
                    if room.series.is_some() {
                        out.games_won.push((room.id, rec.winner_slot().map(|w| usize::from(w) - 1)));
                    }
                    out.records.push(rec);
                }
                for (conn, payload) in outgoing {
                    if !clients.try_send(conn, payload) {
                        out.dead.push(conn);
                    }
                }
                for (conn, state) in states {
                    if !clients.try_send_state(conn, state) {
                        out.dead.push(conn);
                    }
                }
                out
            })
            .collect();

        for out in ticked {
            self.clients.mark_dead(out.dead);
            self.room_list_dirty |= out.list_changed;
            self.effects.extend(out.records.into_iter().map(Effect::SaveMatch));
            for (id, winner) in out.games_won {
                self.series_game_won(id, winner);
            }
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
            if let Some(slot) = self.rooms.get(&id).and_then(|r| r.members.iter().position(|m| m.token == token)) {
                info!("Grâce expirée, retrait room #{id}");
                self.release_seat(id, slot, "jamais revenu après sa déconnexion");
            }
        }
    }
}

fn public_rooms(rooms: &HashMap<RoomId, Room>) -> impl Iterator<Item = &Room> {
    rooms.values().filter(|r| !r.settings.friends_only)
}

/// The friends-only rooms `viewer` may join: its own, or a friend's.
fn private_rooms<'a>(
    rooms: &'a HashMap<RoomId, Room>,
    friends: &'a Friends,
    viewer: Option<Uuid>,
) -> impl Iterator<Item = &'a Room> {
    rooms
        .values()
        .filter(move |r| r.settings.friends_only && r.host_user().is_some_and(|host| friends.sees(viewer, host)))
}

fn room_list(rooms: &HashMap<RoomId, Room>, friends: &Friends, viewer: Option<Uuid>) -> Vec<RoomInfo> {
    public_rooms(rooms).chain(private_rooms(rooms, friends, viewer)).map(Room::info).collect()
}

#[cfg(test)]
macro_rules! effects_of {
    ($name:ident, $variant:ident, $ty:ty) => {
        pub fn $name(&mut self) -> Vec<$ty> {
            let picked = self.effects.extract_if(.., |e| matches!(e, Effect::$variant(_)));
            picked.filter_map(|e| if let Effect::$variant(x) = e { Some(x) } else { None }).collect()
        }
    };
}

#[cfg(test)]
impl Manager {
    pub fn public_room_list(&self) -> Vec<RoomInfo> {
        public_rooms(&self.rooms).map(Room::info).collect()
    }

    effects_of!(take_unsaved_matches, SaveMatch, db::MatchRecord);
    effects_of!(take_unsaved_series, SaveSeries, db::SeriesRecord);
    effects_of!(take_ranked_checks, CheckRanked, RankedCheck);
    effects_of!(take_friend_loads, LoadFriends, Uuid);
    effects_of!(take_friend_checks, CheckFriends, FriendCheck);
}
