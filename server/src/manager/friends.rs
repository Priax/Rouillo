use super::*;

pub const INVITE_COOLDOWN: Duration = Duration::from_secs(2);

const INVITE_NOTE_FROM: &str = "Invitation";
const INVITE_SENT: &str = "Envoyée à votre ami.";
pub(crate) const INVITE_OFFLINE: &str = "Votre ami n'est pas connecté.";

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
    /// May `watcher` watch `room`: a friends-only room's host, or the friend
    /// it looked for, must be a friend. `host` is the token the answer is about.
    Watch {
        conn: ConnId,
        room: RoomId,
        host: Token,
        from: Option<RoomId>,
        watcher: Uuid,
        other: Uuid,
    },
}

impl FriendCheck {
    fn conn(&self) -> ConnId {
        match self {
            Self::Join { conn, .. } | Self::Invite { conn, .. } | Self::Watch { conn, .. } => *conn,
        }
    }

    pub fn users(&self) -> (Uuid, Uuid) {
        match self {
            Self::Join { joiner, host_user, .. } => (*joiner, *host_user),
            Self::Invite { inviter, target, .. } => (*inviter, *target),
            Self::Watch { watcher, other, .. } => (*watcher, *other),
        }
    }
}

/// The accepted friends of each signed-in user connected, for the room list.
#[derive(Default)]
pub struct Friends {
    known: HashMap<Uuid, HashSet<Uuid>>,
    /// Users whose friend list is being read, and whether a friendship of
    /// theirs changed meanwhile, which makes the answer stale.
    loading: HashMap<Uuid, bool>,
}

/// What a friend list read from the database is worth.
enum Loaded {
    Fresh,
    /// A friendship changed while it was read: to read again.
    Stale,
    /// Its user left meanwhile.
    Unwanted,
}

impl Friends {
    /// Starts knowing `user`'s friends: true when they are to be read.
    fn start(&mut self, user: Uuid) -> bool {
        let new = !self.known.contains_key(&user);
        if new {
            self.known.insert(user, HashSet::new());
            self.loading.insert(user, false);
        }
        new
    }

    fn loaded(&mut self, user: Uuid, friends: Vec<Uuid>) -> Loaded {
        let stale = self.loading.remove(&user);
        let Some(known) = self.known.get_mut(&user) else {
            return Loaded::Unwanted;
        };
        if stale == Some(true) {
            self.loading.insert(user, false);
            return Loaded::Stale;
        }
        *known = friends.into_iter().collect();
        Loaded::Fresh
    }

    fn changed(&mut self, user: Uuid, other: Uuid, friends: bool) {
        if let Some(stale) = self.loading.get_mut(&user) {
            *stale = true;
        }
        if let Some(known) = self.known.get_mut(&user) {
            if friends {
                known.insert(other);
            } else {
                known.remove(&other);
            }
        }
    }

    /// Whether `viewer` sees what `host` keeps for friends: theirs, or a friend's.
    pub(super) fn sees(&self, viewer: Option<Uuid>, host: Uuid) -> bool {
        viewer.is_some_and(|v| v == host || self.known.get(&v).is_some_and(|f| f.contains(&host)))
    }
}

impl Manager {
    pub(super) fn load_friends(&mut self, user: Uuid) {
        if self.friends.start(user) {
            self.effects.push(Effect::LoadFriends(user));
        }
    }

    pub(super) fn forget_friends(&mut self, user: Uuid) {
        self.friends.known.remove(&user);
    }

    pub(super) fn friends_loaded(&mut self, user: Uuid, friends: Vec<Uuid>) {
        match self.friends.loaded(user, friends) {
            Loaded::Unwanted => {}
            Loaded::Stale => self.effects.push(Effect::LoadFriends(user)),
            Loaded::Fresh => {
                let rooms = room_list(&self.rooms, &self.friends, Some(user));
                self.clients.deliver_user(user, &encode(&ServerMessage::RoomList { rooms }), Client::browsing);
            }
        }
    }

    pub(super) fn friendship_changed(&mut self, (a, b): (Uuid, Uuid), friends: bool) {
        self.friends.changed(a, b, friends);
        self.friends.changed(b, a, friends);
        self.room_list_dirty = true;
    }

    pub(super) fn request_invite(&mut self, conn: ConnId, target: &str) {
        let Some(inviter) = self.clients.user_of(conn) else {
            return;
        };
        let member = |id: &RoomId| self.rooms.get(id).is_some_and(|r| r.slot_of_conn(conn).is_some());
        let Some(room) = self.clients.room_of(conn).filter(|id| self.series(*id).is_none() && member(id)) else {
            return;
        };
        let Ok(target) = Uuid::parse_str(target) else { return };
        if target == inviter {
            return;
        }
        let now = Instant::now();
        if self.clients.get(conn).and_then(|c| c.last_invite).is_some_and(|t| now.duration_since(t) < INVITE_COOLDOWN) {
            return;
        }
        if !self.clients.begin_friend_check(conn) {
            return;
        }
        if let Some(client) = self.clients.get_mut(conn) {
            client.last_invite = Some(now);
        }
        self.effects.push(Effect::CheckFriends(FriendCheck::Invite { conn, room, inviter, target }));
    }

    pub(super) fn finish_invite_check(&mut self, conn: ConnId, room: RoomId, target: Uuid, friends: bool) {
        if !friends || self.clients.room_of(conn) != Some(room) {
            return;
        }
        let Some(room_name) = self.rooms.get(&room).map(|r| r.name.clone()) else {
            return;
        };
        let from_username =
            self.clients.get(conn).and_then(|c| c.username.clone()).unwrap_or_else(|| "Un joueur".to_string());
        let invitation = encode(&ServerMessage::FriendInvitation { from_username, room_id: room, room_name });
        let sent = self.clients.deliver_user(target, &invitation, |_| true);
        self.invite_note(conn, if sent == 0 { INVITE_OFFLINE } else { INVITE_SENT });
    }

    /// Answers the inviter in the room's chat.
    fn invite_note(&mut self, conn: ConnId, text: &str) {
        self.clients.deliver_msg(
            conn,
            &ServerMessage::Chat { from: INVITE_NOTE_FROM.to_string(), text: text.to_string(), spectator: false },
        );
    }

    pub(super) fn friend_check_done(&mut self, check: FriendCheck, friends: bool) {
        if let Some(client) = self.clients.get_mut(check.conn()) {
            client.checking_friends = false;
        }
        match check {
            FriendCheck::Join { conn, room, host, from, .. } => {
                self.finish_join_check(conn, room, &host, from, friends)
            }
            FriendCheck::Invite { conn, room, target, .. } => {
                self.finish_invite_check(conn, room, target, friends);
            }
            FriendCheck::Watch { conn, room, host, from, other, .. } => {
                if self.clients.room_of(conn) == from {
                    self.finish_watch_check(conn, room, &host, other, friends);
                }
            }
        }
    }
}
