use super::*;

/// What the manager knows about one connection, all dropped with it.
pub struct Client {
    sender: mpsc::Sender<Bytes>,
    /// Game states may go apart, where each one may be lost: only the
    /// latest matters.
    states: Option<mpsc::Sender<Vec<Bytes>>>,
    pub(super) room: Option<RoomId>,
    /// It wants the room list pushed: not while it shows something else.
    pub(super) lists_rooms: bool,
    /// The player id sent in Hello: it follows a browser across reconnections.
    pub token: Option<Token>,
    pub(super) user: Option<Uuid>,
    pub(super) username: Option<String>,
    pub(super) session: Option<Uuid>,
    /// A friendship lookup for it is on its way to the database.
    pub(super) checking_friends: bool,
    pub(super) checking_ranked: bool,
    pub last_invite: Option<Instant>,
    pub(super) chat_budget: Option<(f32, Instant)>,
}

impl Client {
    pub(super) const fn new(sender: mpsc::Sender<Bytes>, states: Option<mpsc::Sender<Vec<Bytes>>>) -> Self {
        Self {
            sender,
            states,
            room: None,
            lists_rooms: true,
            token: None,
            user: None,
            username: None,
            session: None,
            checking_friends: false,
            checking_ranked: false,
            last_invite: None,
            chat_budget: None,
        }
    }

    fn send(&self, payload: Bytes) -> bool {
        self.sender.try_send(payload).is_ok()
    }

    /// Whether game states go to it board by board, in datagrams.
    pub(super) const fn splits_states(&self) -> bool {
        self.states.is_some()
    }

    /// A game state. Board by board, one that finds the way full is
    /// dropped: the next one replaces it.
    fn send_state(&self, out: StateOut) -> bool {
        match (out, &self.states) {
            (StateOut::Boards(boards), Some(states)) => {
                !matches!(states.try_send(boards), Err(mpsc::error::TrySendError::Closed(_)))
            }
            (StateOut::Whole(payload), _) => self.send(payload),
            (StateOut::Boards(_), None) => true,
        }
    }

    pub(super) const fn browsing(&self) -> bool {
        self.room.is_none() && self.lists_rooms
    }
}

/// The connections, by id and by signed-in account, and those found too
/// slow to keep up, to drop.
#[derive(Default)]
pub struct Clients {
    map: HashMap<ConnId, Client>,
    by_user: HashMap<Uuid, HashSet<ConnId>>,
    dead: Vec<ConnId>,
}

impl Clients {
    pub(super) fn insert(&mut self, conn: ConnId, client: Client) {
        self.map.insert(conn, client);
    }

    pub(super) fn remove(&mut self, conn: ConnId) {
        self.map.remove(&conn);
    }

    pub fn contains(&self, conn: ConnId) -> bool {
        self.map.contains_key(&conn)
    }

    pub fn get(&self, conn: ConnId) -> Option<&Client> {
        self.map.get(&conn)
    }

    pub fn get_mut(&mut self, conn: ConnId) -> Option<&mut Client> {
        self.map.get_mut(&conn)
    }

    pub fn room_of(&self, conn: ConnId) -> Option<RoomId> {
        self.get(conn)?.room
    }

    pub(super) fn token_of(&self, conn: ConnId) -> Option<&Token> {
        self.get(conn)?.token.as_ref()
    }

    pub(super) fn user_of(&self, conn: ConnId) -> Option<Uuid> {
        self.get(conn)?.user
    }

    pub(super) fn display_name(&self, conn: ConnId) -> String {
        self.get(conn).and_then(|c| c.username.clone()).unwrap_or_else(|| identity::GUEST_NAME.to_string())
    }

    pub(super) fn set_room(&mut self, conn: ConnId, room: Option<RoomId>) {
        if let Some(client) = self.get_mut(conn) {
            client.room = room;
        }
    }

    /// Marks a friendship lookup as pending for `conn`; false when one already
    /// is, or when the connection is gone.
    pub(super) fn begin_friend_check(&mut self, conn: ConnId) -> bool {
        match self.get_mut(conn) {
            Some(client) if !client.checking_friends => {
                client.checking_friends = true;
                true
            }
            _ => false,
        }
    }

    /// The connections of `user`.
    pub(super) fn of_user(&self, user: Uuid) -> impl Iterator<Item = ConnId> + '_ {
        self.by_user.get(&user).into_iter().flatten().copied()
    }

    /// Who `conn` is, a guest without `user`. False when it is gone.
    pub(super) fn sign_in(
        &mut self,
        conn: ConnId,
        user: Option<Uuid>,
        name: Option<String>,
        session: Option<Uuid>,
    ) -> bool {
        let Some(client) = self.map.get_mut(&conn) else {
            return false;
        };
        client.user = user;
        client.username = name;
        client.session = session;
        if let Some(user) = user {
            self.by_user.entry(user).or_default().insert(conn);
        }
        true
    }

    /// Unties `conn` from its account, if any: the account, and whether it
    /// was its last connection.
    pub(super) fn sign_out(&mut self, conn: ConnId) -> Option<(Uuid, bool)> {
        let client = self.map.get_mut(&conn)?;
        client.username = None;
        client.session = None;
        let user = client.user.take()?;
        let conns = self.by_user.get_mut(&user)?;
        conns.remove(&conn);
        let last = conns.is_empty();
        if last {
            self.by_user.remove(&user);
        }
        Some((user, last))
    }

    pub(super) fn rename(&mut self, user: Uuid, name: &str) {
        for conn in self.by_user.get(&user).into_iter().flatten() {
            if let Some(client) = self.map.get_mut(conn) {
                client.username = Some(name.to_owned());
            }
        }
    }

    /// Sends to `conn`, noting it dead when its way is full or closed.
    pub(super) fn deliver(&mut self, conn: ConnId, payload: Bytes) {
        if !self.try_send(conn, payload) {
            self.dead.push(conn);
        }
    }

    pub(super) fn deliver_msg(&mut self, conn: ConnId, msg: &ServerMessage) {
        self.deliver(conn, encode(msg));
    }

    /// Sends each its own message.
    pub(super) fn deliver_many(&mut self, payloads: impl IntoIterator<Item = (ConnId, Bytes)>) {
        for (conn, payload) in payloads {
            self.deliver(conn, payload);
        }
    }

    pub(super) fn deliver_all(&mut self, conns: impl IntoIterator<Item = ConnId>, payload: &Bytes) {
        for conn in conns {
            self.deliver(conn, payload.clone());
        }
    }

    /// Sends to each connection of `user` that `wanted` picks: how many.
    pub(super) fn deliver_user(&mut self, user: Uuid, payload: &Bytes, wanted: impl Fn(&Client) -> bool) -> usize {
        let Self { map, by_user, dead } = self;
        let mut sent = 0;
        for &conn in by_user.get(&user).into_iter().flatten() {
            let Some(client) = map.get(&conn).filter(|c| wanted(c)) else { continue };
            if !client.send(payload.clone()) {
                dead.push(conn);
            }
            sent += 1;
        }
        sent
    }

    /// Sends what `payload` makes for each connection, to those it makes one for.
    pub(super) fn broadcast(&mut self, mut payload: impl FnMut(&Client) -> Option<Bytes>) {
        let Self { map, dead, .. } = self;
        for (&conn, client) in map.iter() {
            if payload(client).is_some_and(|p| !client.send(p)) {
                dead.push(conn);
            }
        }
    }

    /// From the rooms' threads: false when `conn` is to be dropped.
    pub(super) fn try_send(&self, conn: ConnId, payload: Bytes) -> bool {
        self.get(conn).is_none_or(|c| c.send(payload))
    }

    pub(super) fn try_send_state(&self, conn: ConnId, state: StateOut) -> bool {
        self.get(conn).is_none_or(|c| c.send_state(state))
    }

    pub(super) fn splits_states(&self, conn: ConnId) -> bool {
        self.get(conn).is_some_and(Client::splits_states)
    }

    pub(super) fn mark_dead(&mut self, conns: impl IntoIterator<Item = ConnId>) {
        self.dead.extend(conns);
    }

    pub(super) fn next_dead(&mut self) -> Option<ConnId> {
        self.dead.pop()
    }
}
