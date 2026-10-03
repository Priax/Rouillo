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

    pub(super) fn request_invite(&mut self, conn: ConnId, target: &str) {
        let Some(inviter) = self.conn_user_id.get(&conn).copied() else {
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

    pub(super) fn friend_check_done(&mut self, check: FriendCheck, friends: bool) {
        self.checks_in_flight.remove(&check.conn());
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
