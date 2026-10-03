use super::*;

pub(super) const GUEST_NAME: &str = "Invité";

impl Manager {
    pub(super) fn conn_of_user(&self, user: Uuid) -> Option<ConnId> {
        self.user_conns.get(&user).and_then(|conns| conns.iter().min().copied())
    }

    pub(super) fn display_name(&self, conn: ConnId) -> String {
        self.conn_username
            .get(&conn)
            .cloned()
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
        if let Some(uid) = user {
            self.conn_user_id.insert(conn, uid);
            self.user_conns.entry(uid).or_default().insert(conn);
        }
        if let Some(name) = name {
            self.conn_username.insert(conn, name);
        }
        if let Some(session) = session {
            self.conn_session.insert(conn, session);
        }
    }

    pub(super) fn forget_identity(&mut self, conn: ConnId) {
        if let Some(uid) = self.conn_user_id.remove(&conn) {
            if let Some(conns) = self.user_conns.get_mut(&uid) {
                conns.remove(&conn);
                if conns.is_empty() {
                    self.user_conns.remove(&uid);
                }
            }
        }
        self.conn_username.remove(&conn);
        self.conn_session.remove(&conn);
        self.leave_queue(conn);
    }

    pub(super) fn rename(&mut self, user: Uuid, name: &str) {
        let conns: Vec<ConnId> = self.user_conns.get(&user).into_iter().flatten().copied().collect();
        for conn in conns {
            self.conn_username.insert(conn, name.to_owned());
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
        let conns: Vec<ConnId> = self
            .user_conns
            .get(&user)
            .map(|c| c.iter().copied().collect())
            .unwrap_or_default();
        for conn in conns {
            if keep.is_some() && self.conn_session.get(&conn).copied() == keep {
                continue;
            }
            self.forget_identity(conn);
            self.deliver_msg(conn, &ServerMessage::SessionRevoked);
        }
    }
}
