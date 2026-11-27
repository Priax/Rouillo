use super::*;

pub(super) const GUEST_NAME: &str = "Invité";

impl Manager {
    /// Tells the players of a saved game, those still in a room, that its
    /// replay can be kept.
    pub(super) fn match_saved(&mut self, id: Uuid, users: &[Uuid]) {
        let msg = encode(&ServerMessage::MatchSaved { id });
        for &user in users {
            self.clients.deliver_user(user, &msg, |c| c.room.is_some());
        }
    }

    pub(super) fn set_identity(
        &mut self,
        conn: ConnId,
        user: Option<Uuid>,
        name: Option<String>,
        session: Option<Uuid>,
    ) {
        self.forget_identity(conn);
        if self.clients.sign_in(conn, user, name, session) {
            if let Some(user) = user {
                self.load_friends(user);
            }
        }
    }

    pub(super) fn forget_identity(&mut self, conn: ConnId) {
        if let Some((user, true)) = self.clients.sign_out(conn) {
            self.forget_friends(user);
        }
        self.leave_queue(conn);
    }

    pub(super) fn rename(&mut self, user: Uuid, name: &str) {
        self.clients.rename(user, name);
        for room in self.rooms.values_mut() {
            let seats = room.members.iter_mut().filter(|m| m.user_id == Some(user)).map(|m| &mut m.name);
            let seats =
                seats.chain(room.spectators.iter_mut().filter(|s| s.user_id == Some(user)).map(|s| &mut s.name));
            let mut any = false;
            for seat in seats {
                name.clone_into(seat);
                any = true;
            }
            if any && !matches!(room.phase, Phase::Playing) {
                self.clients.deliver_many(room.lobby_payloads());
            }
        }
        let series = self.rooms.values_mut().filter_map(|r| r.series.as_mut());
        for s in series.filter(|s| s.result.is_none()) {
            for i in (0..2).filter(|&i| s.users[i] == user) {
                s.names[i] = name.to_owned();
            }
        }
    }

    pub(super) fn revoke(&mut self, user: Uuid, keep: Option<Uuid>) {
        let conns: Vec<ConnId> = self.clients.of_user(user).collect();
        for conn in conns {
            if keep.is_some() && self.clients.get(conn).and_then(|c| c.session) == keep {
                continue;
            }
            self.forget_identity(conn);
            self.clients.deliver_msg(conn, &ServerMessage::SessionRevoked);
        }
    }
}
