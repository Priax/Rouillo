use super::*;

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

impl Manager {
    pub fn take_friend_checks(&mut self) -> Vec<FriendCheck> {
        std::mem::take(&mut self.friend_checks)
    }

    pub fn take_friend_loads(&mut self) -> Vec<Uuid> {
        std::mem::take(&mut self.friend_loads)
    }

    pub(super) fn load_friends(&mut self, user: Uuid) {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.friends.entry(user) {
            entry.insert(HashSet::new());
            self.friends_loading.insert(user, false);
            self.friend_loads.push(user);
        }
    }

    pub(super) fn forget_friends(&mut self, user: Uuid) {
        self.friends.remove(&user);
    }

    pub(super) fn friends_loaded(&mut self, user: Uuid, friends: Vec<Uuid>) {
        let stale = self.friends_loading.remove(&user);
        let Some(known) = self.friends.get_mut(&user) else {
            return;
        };
        if stale == Some(true) {
            self.friends_loading.insert(user, false);
            self.friend_loads.push(user);
            return;
        }
        *known = friends.into_iter().collect();
        let browsing: Vec<ConnId> = self
            .user_conns
            .get(&user)
            .into_iter()
            .flatten()
            .copied()
            .filter(|c| self.clients.get(c).is_some_and(|c| c.room.is_none()))
            .collect();
        for conn in browsing {
            self.send_room_list_to(conn);
        }
    }

    pub(super) fn friendship_changed(&mut self, (a, b): (Uuid, Uuid), friends: bool) {
        for (user, other) in [(a, b), (b, a)] {
            if let Some(stale) = self.friends_loading.get_mut(&user) {
                *stale = true;
            }
            if let Some(known) = self.friends.get_mut(&user) {
                if friends {
                    known.insert(other);
                } else {
                    known.remove(&other);
                }
            }
        }
        self.room_list_dirty = true;
    }

    pub(super) fn request_invite(&mut self, conn: ConnId, target: &str) {
        let Some(inviter) = self.user_of(conn) else {
            return;
        };
        let member = |id: &RoomId| self.rooms.get(id).is_some_and(|r| r.slot_of_conn(conn).is_some());
        let Some(room) = self.room_of(conn).filter(|id| self.series(*id).is_none() && member(id)) else {
            return;
        };
        let Ok(target) = Uuid::parse_str(target) else { return };
        if target == inviter || self.conn_of_user(target).is_none() {
            return;
        }
        let now = Instant::now();
        if self
            .clients
            .get(&conn)
            .and_then(|c| c.last_invite)
            .is_some_and(|t| now.duration_since(t) < INVITE_COOLDOWN)
        {
            return;
        }
        if !self.begin_friend_check(conn) {
            return;
        }
        if let Some(client) = self.clients.get_mut(&conn) {
            client.last_invite = Some(now);
        }
        self.friend_checks.push(FriendCheck::Invite {
            conn,
            room,
            inviter,
            target,
        });
    }

    pub(super) fn finish_invite_check(&mut self, conn: ConnId, room: RoomId, target: Uuid, friends: bool) {
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
            .clients
            .get(&conn)
            .and_then(|c| c.username.clone())
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

    pub(super) fn friend_check_done(&mut self, check: FriendCheck, friends: bool) {
        if let Some(client) = self.clients.get_mut(&check.conn()) {
            client.checking_friends = false;
        }
        match check {
            FriendCheck::Join {
                conn, room, host, from, ..
            } => self.finish_join_check(conn, room, &host, from, friends),
            FriendCheck::Invite { conn, room, target, .. } => {
                self.finish_invite_check(conn, room, target, friends);
            }
            FriendCheck::Watch {
                conn,
                room,
                host,
                from,
                other,
                ..
            } => {
                if self.room_of(conn) == from {
                    self.finish_watch_check(conn, room, &host, other, friends);
                }
            }
        }
    }
}
