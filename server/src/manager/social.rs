use super::*;

const CHAT_BURST: f32 = 5.0;
const CHAT_REFILL_SECS: f32 = 1.5;

/// The message as shown to others: one line of printable text, bounded.
pub fn clean_chat(text: &str) -> Option<String> {
    let line: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect();
    let line: String = line.trim().chars().take(shared::MAX_CHAT_CHARS).collect();
    (!line.is_empty()).then_some(line)
}

impl Manager {
    pub(super) fn chat(&mut self, conn: ConnId, text: &str) {
        let Some(id) = self.room_of(conn) else { return };
        let Some(text) = clean_chat(text) else { return };
        let now = Instant::now();
        let (tokens, last) = self.chat_budget.entry(conn).or_insert((CHAT_BURST, now));
        *tokens = (*tokens + now.duration_since(*last).as_secs_f32() / CHAT_REFILL_SECS).min(CHAT_BURST);
        *last = now;
        if *tokens < 1.0 {
            return;
        }
        *tokens -= 1.0;
        let spectator = self.rooms.get(&id).is_some_and(|r| r.spectator_slot(conn).is_some());
        let msg = ServerMessage::Chat {
            from: self.display_name(conn),
            text,
            spectator,
        };
        self.send_room_msg(id, &msg);
    }

    pub(super) fn spectate(&mut self, conn: ConnId, id: RoomId) {
        if !self.conn_token.contains_key(&conn) || self.room_of(conn) == Some(id) {
            return;
        }
        let Some(room) = self.rooms.get(&id) else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if !room.settings.friends_only {
            self.enter_audience(conn, id);
            return;
        }
        let host = room.host.clone();
        let host_user = room.members.iter().find(|m| m.token == host).and_then(|m| m.user_id);
        let watcher = self.conn_user_id.get(&conn).copied();
        match (watcher, host_user) {
            (Some(watcher), Some(other)) if watcher == other => self.enter_audience(conn, id),
            (Some(watcher), Some(other)) => self.check_watch(conn, id, host, watcher, other),
            _ => self.join_failed(conn, JOIN_FRIENDS_ONLY),
        }
    }

    /// Watches the game a friend is in, wherever it is.
    pub(super) fn watch_friend(&mut self, conn: ConnId, friend: &str) {
        let (Some(watcher), Ok(friend)) = (self.conn_user_id.get(&conn).copied(), Uuid::parse_str(friend)) else {
            return;
        };
        let found = self
            .rooms
            .values()
            .find(|r| r.members.iter().any(|m| m.user_id == Some(friend) && m.conn.is_some()));
        let Some((id, host)) = found.map(|r| (r.id, r.host.clone())) else {
            self.join_failed(conn, WATCH_NOBODY);
            return;
        };
        self.check_watch(conn, id, host, watcher, friend);
    }

    fn check_watch(&mut self, conn: ConnId, room: RoomId, host: Token, watcher: Uuid, other: Uuid) {
        if !self.checks_in_flight.insert(conn) {
            return;
        }
        let from = self.room_of(conn);
        self.friend_checks.push(FriendCheck::Watch {
            conn,
            room,
            host,
            from,
            watcher,
            other,
        });
    }

    /// The answer to a `FriendCheck::Watch` about `other`. A friends-only
    /// room also needs the friendship of its current host, asked next.
    pub(super) fn finish_watch_check(&mut self, conn: ConnId, id: RoomId, host: &str, other: Uuid, friends: bool) {
        if !self.senders.contains_key(&conn) {
            return;
        }
        let Some(room) = self.rooms.get(&id) else {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        };
        if !friends {
            self.join_failed(conn, WATCH_NOT_FRIENDS);
            return;
        }
        let host_user = room
            .members
            .iter()
            .find(|m| m.token == room.host)
            .and_then(|m| m.user_id);
        if room.settings.friends_only && (room.host != host || host_user != Some(other)) {
            self.spectate(conn, id);
        } else {
            self.enter_audience(conn, id);
        }
    }

    fn enter_audience(&mut self, conn: ConnId, id: RoomId) {
        if self.room_of(conn) == Some(id) {
            return;
        }
        if self
            .rooms
            .get(&id)
            .is_none_or(|r| r.spectators.len() >= shared::MAX_SPECTATORS)
        {
            self.join_failed(conn, JOIN_UNAVAILABLE);
            return;
        }
        self.leave_current(conn);
        let spectator = Spectator {
            conn,
            user_id: self.conn_user_id.get(&conn).copied(),
            name: self.display_name(conn),
        };
        let Some(room) = self.rooms.get_mut(&id) else { return };
        room.spectators.push(spectator);
        let playing = matches!(room.phase, Phase::Playing);
        let snapshot = playing.then(|| room.sim.state_update(true));
        let lobby = room.spectator_info();
        self.clients.insert(conn, Some(id));
        self.refresh_lobby(id);
        if playing {
            self.deliver_msg(conn, &ServerMessage::Lobby { info: lobby });
        }
        if let Some(snapshot) = snapshot {
            self.deliver_msg(conn, &ServerMessage::GameStart);
            self.deliver_msg(conn, &snapshot);
        }
        self.room_list_dirty = true;
        info!("WS {conn} regarde la room #{id}");
    }

    /// Takes a spectator out of the room. Returns false for a player.
    pub(super) fn leave_audience(&mut self, conn: ConnId, id: RoomId) -> bool {
        let Some(room) = self.rooms.get_mut(&id) else {
            return false;
        };
        let Some(i) = room.spectator_slot(conn) else {
            return false;
        };
        room.spectators.remove(i);
        self.clients.insert(conn, None);
        self.refresh_lobby(id);
        self.room_list_dirty = true;
        true
    }

    /// Sends the lobby again after a change only the lobby shows, such as who
    /// watches: in a game, a Lobby message would take everyone out of it.
    pub(super) fn refresh_lobby(&mut self, id: RoomId) {
        if self.rooms.get(&id).is_some_and(|r| !matches!(r.phase, Phase::Playing)) {
            self.send_lobby(id);
        }
    }

    /// Removes a room, sending its spectators back to the room list.
    pub(super) fn close_room(&mut self, id: RoomId) {
        let Some(room) = self.rooms.remove(&id) else { return };
        for s in room.spectators {
            self.clients.insert(s.conn, None);
            self.send_room_list_to(s.conn);
        }
    }
}
