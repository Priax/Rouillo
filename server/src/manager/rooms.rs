use super::*;

pub(crate) fn clean_name(name: &str) -> String {
    super::social::one_line(name, 24).unwrap_or_else(|| "Room".to_string())
}

impl Manager {
    pub(super) fn record_forfeit(&mut self, id: RoomId, slot: usize, why: &str) {
        if let Some(rec) = self.rooms.get_mut(&id).and_then(|r| r.forfeit(slot)) {
            info!("Forfait room #{id}: slot {} perd ({why})", slot + 1);
            self.unsaved_matches.push(rec);
        }
    }

    pub(super) fn request_join(&mut self, conn: ConnId, id: RoomId) {
        if self.token_of(conn).is_none() {
            return;
        }
        if self.rooms.get(&id).is_some_and(|r| r.slot_of_conn(conn).is_some()) {
            return;
        }
        if self.closing {
            self.join_failed(conn, JOIN_MAINTENANCE);
            return;
        }
        let Some(room) = self
            .rooms
            .get(&id)
            .filter(|r| r.members.len() < 2 && r.series.is_none())
        else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if !room.settings.friends_only {
            self.complete_join(conn, id);
            return;
        }
        let host = room.host.clone();
        let (Some(host_user), Some(joiner)) = (room.host_user(), self.user_of(conn)) else {
            self.join_failed(conn, JOIN_FRIENDS_ONLY);
            return;
        };
        if !self.begin_friend_check(conn) {
            return;
        }
        let from = self.room_of(conn);
        self.friend_checks.push(FriendCheck::Join {
            conn,
            room: id,
            host,
            from,
            joiner,
            host_user,
        });
    }

    pub(super) fn complete_join(&mut self, conn: ConnId, id: RoomId) {
        let Some(token) = self.token_of(conn).cloned() else {
            return;
        };
        if self.closing {
            self.join_failed(conn, JOIN_MAINTENANCE);
            return;
        }
        if self
            .rooms
            .get(&id)
            .is_none_or(|r| r.members.len() >= 2 || r.series.is_some())
        {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        }
        let user_id = self.user_of(conn);
        if user_id.is_some() && self.rooms[&id].members.iter().any(|m| m.user_id == user_id) {
            self.join_failed(conn, JOIN_SAME_ACCOUNT);
            return;
        }
        self.leave_current(conn);
        let name = self.display_name(conn);
        if let Some(room) = self.rooms.get_mut(&id) {
            room.members.push(Member::present(token, conn, user_id, name));
        }
        self.set_room(conn, Some(id));
        self.sync_after_attach(id, conn);
    }

    pub(super) fn finish_join_check(
        &mut self,
        conn: ConnId,
        room: RoomId,
        host: &str,
        from: Option<RoomId>,
        friends: bool,
    ) {
        if !self.clients.contains_key(&conn) || self.room_of(conn) != from {
            return;
        }
        let Some(r) = self.rooms.get(&room) else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if r.settings.friends_only {
            if r.host != host {
                self.request_join(conn, room);
                return;
            }
            if !friends {
                self.join_failed(conn, JOIN_FRIENDS_ONLY);
                return;
            }
        }
        self.complete_join(conn, room);
    }

    pub(super) fn create_room(&mut self, conn: ConnId, name: &str) {
        let Some(token) = self.token_of(conn).cloned() else {
            return;
        };
        if self.closing {
            self.join_failed(conn, JOIN_MAINTENANCE);
            return;
        }
        self.leave_current(conn);
        let id = self.next_id;
        self.next_id += 1;
        let user_id = self.user_of(conn);
        let member = Member::present(token, conn, user_id, self.display_name(conn));
        let room = Room::new(id, clean_name(name), vec![member], RoomSettings::default(), None);
        self.rooms.insert(id, room);
        self.set_room(conn, Some(id));
        self.send_lobby(id);
        self.room_list_dirty = true;
        info!("Room #{id} créée (conn {conn})");
    }

    pub(super) fn set_setting(&mut self, conn: ConnId, index: u8, dir: i32) {
        if let Some(id) = self.room_of(conn) {
            let changed = self
                .with_room(id, |room| {
                    if room.series.is_none() && room.is_host_conn(conn) && matches!(room.phase, Phase::Lobby) {
                        room.settings.adjust(index as usize, dir);
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if changed {
                self.send_lobby(id);
            }
        }
    }

    pub(super) fn toggle_countdown(&mut self, conn: ConnId) {
        let closing = self.closing;
        if let Some(id) = self.room_of(conn) {
            let toggled = self
                .with_room(id, |room| {
                    if room.series.is_none() && room.is_host_conn(conn) {
                        room.phase = match room.phase {
                            Phase::Lobby if !closing && room.members.len() >= 2 && room.all_connected() => {
                                Phase::CountingDown(COUNTDOWN_SECS)
                            }
                            Phase::Lobby => Phase::Lobby,
                            Phase::CountingDown(_) => Phase::Lobby,
                            Phase::Playing => Phase::Playing,
                        };
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if toggled {
                self.send_lobby(id);
                self.room_list_dirty = true;
            }
        }
    }

    pub(super) fn return_to_lobby(&mut self, conn: ConnId) {
        let Some(id) = self.room_of(conn) else { return };
        let host_slot = self
            .rooms
            .get(&id)
            .filter(|room| room.series.is_none() && room.is_host_conn(conn))
            .and_then(|room| room.slot_of_conn(conn));
        let Some(slot) = host_slot else { return };
        self.record_forfeit(id, slot, "l'hôte a interrompu la partie");
        if let Some(room) = self.rooms.get_mut(&id) {
            room.back_to_lobby();
        }
        self.send_lobby(id);
        self.room_list_dirty = true;
    }

    pub(super) fn on_input(&mut self, conn: ConnId, kind: InputKind, seq: u32, tick: u32) {
        if let Some(id) = self.room_of(conn) {
            self.with_room(id, |room| {
                let Some(idx) = room.slot_of_conn(conn) else {
                    return;
                };
                if matches!(room.phase, Phase::Playing) && !room.sim.paused && !room.sim.finished {
                    room.sim.queue_input(idx, tick, seq, kind);
                } else {
                    room.sim.last_seq[idx] = seq;
                }
            });
        }
    }

    pub(super) fn toggle_pause(&mut self, conn: ConnId) {
        if let Some(id) = self.room_of(conn) {
            let toggled = self
                .with_room(id, |room| {
                    let allowed = room.slot_of_conn(conn).is_some()
                        && room.game_running()
                        && room.all_connected()
                        && room.settings.pause.allows(room.is_host_conn(conn));
                    if allowed {
                        room.sim.set_paused(!room.sim.paused);
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if toggled {
                self.send_snapshot(id);
            }
        }
    }

    pub(super) fn restart(&mut self, conn: ConnId) {
        if self.closing {
            return;
        }
        if let Some(id) = self.room_of(conn) {
            let restarted = self
                .with_room(id, |room| {
                    if room.series.is_none()
                        && room.slot_of_conn(conn).is_some()
                        && matches!(room.phase, Phase::Playing)
                        && room.sim.finished
                        && room.all_connected()
                    {
                        let now = Instant::now();
                        let in_cooldown = room
                            .sim
                            .last_restart
                            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(2));
                        if in_cooldown {
                            false
                        } else {
                            room.sim.last_restart = Some(now);
                            room.sim.reset_boards(&room.settings);
                            true
                        }
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if restarted {
                self.send_room_msg(id, &ServerMessage::Restart);
            }
        }
    }
}
