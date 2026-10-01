use super::*;

impl Manager {
    pub(super) fn conn_of_user(&self, user: Uuid) -> Option<ConnId> {
        self.user_conns.get(&user).and_then(|conns| conns.iter().min().copied())
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
        self.queue.retain(|e| e.conn != conn);
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
