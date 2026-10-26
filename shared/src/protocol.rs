use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::board::{Board, IncomingGarbage, InputKind, Phase, PuyoType};
use crate::room::{RoomSetting, RoomSettings};

pub const PROTOCOL_VERSION: u32 = 12;

pub const MAX_CHAT_CHARS: usize = 200;
pub const MAX_SPECTATORS: usize = 20;
pub const MAX_USERNAME_CHARS: usize = 24;

/// The most inputs one `Inputs` message carries.
pub const MAX_RESENT_INPUTS: usize = 16;

pub const OUTDATED_FRAME: &str = "outdated";

pub type RoomId = u32;

pub fn valid_username(name: &str) -> bool {
    (3..=MAX_USERNAME_CHARS).contains(&name.chars().count()) && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Who a player's nuisance goes to, among the opponents still playing. The
/// player picks it and may change it during the game.
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Targeting {
    #[default]
    Random,
    /// Those targeting this player, or else a random opponent.
    Attackers,
    /// The opponent whose board is fullest, to finish them off.
    Danger,
    /// A slot of the player's choosing, from 1.
    Manual(u8),
}

impl Targeting {
    /// Who `slot` aims at among `opponents`, by index as `Manual` holds one,
    /// or `None` for whoever chance picks. `targets` is where each index
    /// aims now, `danger` how near a board is to topping out.
    pub fn aim(
        self,
        slot: usize,
        opponents: &[usize],
        targets: &[Option<usize>],
        danger: impl Fn(usize) -> (usize, usize),
    ) -> Option<usize> {
        match self {
            Self::Manual(t) if opponents.contains(&usize::from(t)) => Some(usize::from(t)),
            Self::Danger => opponents.iter().copied().max_by_key(|&o| danger(o)),
            Self::Attackers => opponents.iter().copied().find(|&o| targets[o] == Some(slot)),
            Self::Random | Self::Manual(_) => None,
        }
    }
}

/// One player's part of a state update.
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct PlayerUpdate {
    pub slot: u8,
    pub team: u8,
    pub board: std::sync::Arc<Board>,
    pub ack: u32,
    pub incoming: Vec<IncomingGarbage>,
    /// The slot this player's nuisance goes to now.
    pub target: Option<u8>,
}

/// A board of a big room as it is drawn small, beside the one watched
/// large: without the random state, score or next pairs. The stack is left
/// out while it has not changed since it was last sent.
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct Thumb {
    pub slot: u8,
    pub team: u8,
    pub target: Option<u8>,
    pub incoming: Vec<IncomingGarbage>,
    pub width: u8,
    pub height: u8,
    /// The stack row by row, `width` cells a row.
    pub cells: Option<Vec<Option<PuyoType>>>,
    pub piece_id: u32,
    pub phase: Phase,
    pub paused: bool,
    pub pending_garbage: u32,
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Debug)]
pub struct RoomInfo {
    pub id: RoomId,
    pub name: String,
    pub players: u8,
    pub max: u8,
    pub in_game: bool,
    pub friends_only: bool,
    pub spectators: u8,
    pub ranked: bool,
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Debug)]
pub struct LobbyInfo {
    pub id: RoomId,
    pub name: String,
    pub settings: RoomSettings,
    pub players: u8,
    pub connected: u8,
    pub your_slot: u8,
    pub is_host: bool,
    pub countdown: Option<u8>,
    pub ranked: Option<RankedInfo>,
    pub names: Vec<String>,
    /// Each player's team, in the order of `names`.
    pub teams: Vec<Option<u8>>,
    pub spectators: u8,
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct RankedInfo {
    pub opponent: String,
    pub opponent_elo: i32,
    pub opponent_avatar: Option<String>,
    pub wins: [u8; 2],
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub enum ClientMessage {
    Hello {
        player_id: String,
        auth_token: Option<String>,
        last_disconnect_reason: Option<String>,
    },
    Input {
        kind: InputKind,
        seq: u32,
        tick: u32,
    },
    TogglePause,
    RequestRestart,
    RequestRoomList,
    CreateRoom {
        name: String,
    },
    JoinRoom {
        id: RoomId,
    },
    LeaveRoom,
    SetRoomSetting {
        setting: RoomSetting,
        dir: i32,
    },
    ToggleCountdown,
    ReturnToLobby,
    InviteFriend {
        user_id: String,
    },
    Ping {
        id: u32,
    },
    JoinQueue,
    LeaveQueue,
    AcceptMatch,
    Chat {
        text: String,
    },
    Spectate {
        id: RoomId,
    },
    WatchFriend {
        user_id: String,
    },
    SetTargeting {
        targeting: Targeting,
    },
    SetTeam {
        team: Option<u8>,
    },
    /// The inputs not acknowledged yet, sent again in a datagram: one lost
    /// on the stream need not wait for its retransmission.
    Inputs {
        inputs: Vec<SentInput>,
    },
    /// The board to watch large, from 1, for one in a big room without a
    /// board of their own: the others come small.
    Follow {
        slot: u8,
    },
}

/// An input as the client numbered and stamped it.
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentInput {
    pub seq: u32,
    pub tick: u32,
    pub kind: InputKind,
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub enum ServerMessage {
    /// Also restarts a finished game. Slots may change between games.
    GameStart {
        /// Counts the room's games: updates of another one are stale.
        game: u32,
        your_slot: u8,
        /// Each game slot's player, from slot 1.
        names: Vec<String>,
    },
    StateUpdate {
        game: u32,
        tick: u32,
        players: Vec<PlayerUpdate>,
    },
    OpponentDisconnected,
    RoomList {
        rooms: Vec<RoomInfo>,
    },
    Lobby {
        info: LobbyInfo,
    },
    JoinFailed {
        reason: String,
    },
    FriendInvitation {
        from_username: String,
        room_id: RoomId,
        room_name: String,
    },
    Pong {
        id: u32,
    },
    Maintenance,
    QueueRefused {
        reason: String,
    },
    MatchFound {
        opponent: String,
        opponent_elo: i32,
        opponent_avatar: Option<String>,
        secs: u32,
    },
    MatchCancelled {
        requeued: bool,
    },
    QueueCooldown {
        secs: u32,
    },
    SeriesScore {
        wins: [u8; 2],
    },
    SeriesOver {
        winner_slot: Option<u8>,
        elo_change: i32,
    },
    SessionRevoked,
    Chat {
        from: String,
        text: String,
        spectator: bool,
    },
    Spectators {
        count: u8,
    },
    /// Another connection with the same player id took this one's seat:
    /// the game was opened in another tab or window.
    OpenedElsewhere,
    /// The boards of a big room, small, for those in it who watch another
    /// one large: their own, or the one they follow.
    Thumbs {
        game: u32,
        tick: u32,
        thumbs: Vec<Thumb>,
    },
}
