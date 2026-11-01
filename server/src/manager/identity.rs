use super::*;

pub(super) const GUEST_NAME: &str = "Invité";

impl Manager {
    pub(super) fn conns_of_user(&self, user: Uuid) -> Vec<ConnId> {
        self.user_conns.get(&user).into_iter().flatten().copied().collect()
    }

    /// Tells the players of a saved game, those still in a room, that its
    /// replay can be kept.
    pub(super) fn match_saved(&mut self, id: Uuid, users: &[Uuid]) {
        let msg = ServerMessage::MatchSaved { id: id.to_string() };
        for &user in users {
            for conn in self.conns_of_user(user) {
                if self.room_of(conn).is_some() {
                    self.deliver_msg(conn, &msg);
                }
            }
        }
    }

    pub(super) fn display_name(&self, conn: ConnId) -> String {
        self.clients
            .get(&conn)
            .and_then(|c| c.username.clone())
            .unwrap_or_else(|| GUEST_NAME.to_string())
    }

    pub(super) fn set_identity(
        &mut self,
        conn: ConnId,
        user: Option<Uuid>,
        name: Option<String>,
        session: Option<Uuid>,
    ) {
        self.forget_identity(conn);
        let Some(client) = self.clients.get_mut(&conn) else {
            return;
        };
        client.user = user;
        client.username = name;
        client.session = session;
        if let Some(uid) = user {
            self.user_conns.entry(uid).or_default().insert(conn);
            self.load_friends(uid);
        }
    }

    pub(super) fn forget_identity(&mut self, conn: ConnId) {
        let user = self.clients.get_mut(&conn).and_then(|c| {
            c.username = None;
            c.session = None;
            c.user.take()
        });
        if let Some(uid) = user {
            if let Some(conns) = self.user_conns.get_mut(&uid) {
                conns.remove(&conn);
                if conns.is_empty() {
                    self.user_conns.remove(&uid);
                    self.forget_friends(uid);
                }
            }
        }
        self.leave_queue(conn);
    }

    pub(super) fn rename(&mut self, user: Uuid, name: &str) {
        for conn in self.conns_of_user(user) {
            if let Some(client) = self.clients.get_mut(&conn) {
                client.username = Some(name.to_owned());
            }
        }
        let mut renamed = Vec::new();
        for room in self.rooms.values_mut() {
            let seats = room
                .members
                .iter_mut()
                .filter(|m| m.user_id == Some(user))
                .map(|m| &mut m.name);
            let seats = seats.chain(
                room.spectators
                    .iter_mut()
                    .filter(|s| s.user_id == Some(user))
                    .map(|s| &mut s.name),
            );
            let mut any = false;
            for seat in seats {
                name.clone_into(seat);
                any = true;
            }
            if any {
                renamed.push(room.id);
            }
        }
        for id in renamed {
            self.refresh_lobby(id);
        }
        let series = self.rooms.values_mut().filter_map(|r| r.series.as_mut());
        for s in series.filter(|s| s.result.is_none()) {
            for i in (0..2).filter(|&i| s.users[i] == user) {
                s.names[i] = name.to_owned();
            }
        }
    }

    pub(super) fn revoke(&mut self, user: Uuid, keep: Option<Uuid>) {
        for conn in self.conns_of_user(user) {
            if keep.is_some() && self.clients.get(&conn).and_then(|c| c.session) == keep {
                continue;
            }
            self.forget_identity(conn);
            self.deliver_msg(conn, &ServerMessage::SessionRevoked);
        }
    }
}
