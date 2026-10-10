use super::*;

/// What the manager knows about one connection, all dropped with it.
pub struct Client {
    sender: mpsc::Sender<Vec<u8>>,
    pub(super) room: Option<RoomId>,
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
    pub(super) const fn new(sender: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            sender,
            room: None,
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

    pub(super) fn send(&self, payload: Vec<u8>) -> bool {
        self.sender.try_send(payload).is_ok()
    }
}

impl Manager {
    pub(super) fn token_of(&self, conn: ConnId) -> Option<&Token> {
        self.clients.get(&conn)?.token.as_ref()
    }

    pub(super) fn user_of(&self, conn: ConnId) -> Option<Uuid> {
        self.clients.get(&conn)?.user
    }

    pub(super) fn set_room(&mut self, conn: ConnId, room: Option<RoomId>) {
        if let Some(client) = self.clients.get_mut(&conn) {
            client.room = room;
        }
    }

    /// Marks a friendship lookup as pending for `conn`; false when one already
    /// is, or when the connection is gone.
    pub(super) fn begin_friend_check(&mut self, conn: ConnId) -> bool {
        match self.clients.get_mut(&conn) {
            Some(client) if !client.checking_friends => {
                client.checking_friends = true;
                true
            }
            _ => false,
        }
    }
}
