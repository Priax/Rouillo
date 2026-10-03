use super::*;

impl Manager {
    pub(super) fn on_hello(&mut self, conn: ConnId, token: Token) {
        if token.is_empty() || token.len() > MAX_PLAYER_ID {
            warn!("WS {conn}: player_id invalide, ignoré");
            return;
        }
        let user = self.conn_user_id.get(&conn).copied();
        let room = match self.room_of_token(&token) {
            Some(id) => {
                let seat = self.rooms[&id].members.iter().position(|m| m.token == token);
                match seat {
                    Some(slot) if self.series_over(id) => {
                        self.detach_ranked(id, slot);
                        None
                    }
                    Some(slot) if self.rooms[&id].members[slot].user_id != user => {
                        if self.rooms[&id].members[slot].conn.is_none() {
                            info!("WS {conn}: autre compte sur ce navigateur, place libérée room #{id}");
                            self.release_seat(id, slot, "un autre compte a repris son navigateur");
                        }
                        None
                    }
                    _ => Some(id),
                }
            }
            None => None,
        };
        self.conn_token.insert(conn, token);
        if let Some(id) = room {
            self.rejoin(conn, id);
        } else {
            self.send_room_list_to(conn);
        }
        if self.closing {
            self.deliver_msg(conn, &ServerMessage::Maintenance);
        }
    }

    pub(super) fn rejoin(&mut self, conn: ConnId, id: RoomId) {
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
            self.forget_conn(old);
        }
        self.sync_after_attach(id, conn);
        info!("Reconnexion room #{id} (conn {conn})");
    }

    pub(super) fn sync_after_attach(&mut self, id: RoomId, conn: ConnId) {
        if self.series(id).is_some() {
            self.attach_ranked(id, conn);
            return;
        }
        let resume = if let Some(room) = self.rooms.get_mut(&id) {
            let playing = matches!(room.phase, Phase::Playing);
            let all = room.all_connected();
            if playing && all {
                room.sim.set_paused(false);
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

    pub(super) fn mark_disconnected(&mut self, conn: ConnId) {
        let Some(id) = self.room_of(conn) else {
            return;
        };
        if self.leave_audience(conn, id) {
            return;
        }
        let (notify, in_game) = if let Some(room) = self.rooms.get_mut(&id) {
            let Some(slot) = room.slot_of_conn(conn) else {
                return;
            };
            room.members[slot].conn = None;
            room.members[slot].disconnect_at = Some(Instant::now());
            if matches!(room.phase, Phase::CountingDown(_)) && room.series.is_none() {
                room.phase = Phase::Lobby;
                self.room_list_dirty = true;
            }
            let playing = matches!(room.phase, Phase::Playing);
            let notify = playing && !room.sim.finished && room.series.is_none();
            if notify {
                room.sim.set_paused(true);
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

    pub(super) fn drop_connection(&mut self, conn: ConnId) {
        self.mark_disconnected(conn);
        self.forget_conn(conn);
    }

    fn forget_conn(&mut self, conn: ConnId) {
        self.clients.remove(&conn);
        self.conn_token.remove(&conn);
        self.forget_identity(conn);
        self.senders.remove(&conn);
        self.checks_in_flight.remove(&conn);
        self.ranked_checking.remove(&conn);
        self.last_invite.remove(&conn);
        self.chat_budget.remove(&conn);
    }

    pub(super) fn leave_current(&mut self, conn: ConnId) {
        self.leave_queue(conn);
        let Some(id) = self.room_of(conn) else {
            return;
        };
        self.clients.insert(conn, None);
        if self.leave_audience(conn, id) {
            return;
        }
        let Some(slot) = self.rooms.get(&id).and_then(|r| r.slot_of_conn(conn)) else {
            return;
        };
        self.release_seat(id, slot, "a quitté la partie");
    }

    pub(super) fn release_seat(&mut self, id: RoomId, slot: usize, why: &str) {
        self.record_forfeit(id, slot, why);
        self.forfeit_series(id, slot);
        self.remove_member(id, slot);
    }

    pub(super) fn remove_member(&mut self, id: RoomId, slot: usize) {
        if self.series(id).is_some() {
            self.detach_ranked(id, slot);
            return;
        }
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
                    room.back_to_lobby();
                }
                false
            }
        } else {
            return;
        };

        if closed {
            self.close_room(id);
        } else {
            self.send_lobby(id);
        }
        self.room_list_dirty = true;
    }
}
