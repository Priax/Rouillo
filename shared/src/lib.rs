#![warn(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless
)]

mod board;
mod codec;
pub mod config;
pub mod cpu;
mod hash;
mod protocol;
mod replay;
mod room;

pub use board::{
    fallen_px, frames_to_fall, group_bonus, link_score, ActivePuyo, Board, CellFall, Events, Fall, GameState, Grid,
    IncomingGarbage, InputKind, PairState, Phase, PuyoType, Rotation, Rules, StampedInput,
};
pub use codec::{decode, decode_bounded, encode, frame_header, frame_len, FRAME_HEADER, MAX_DECODED_SIZE};
pub use hash::fnv1a;
pub use protocol::{
    valid_username, ClientMessage, LobbyInfo, PlayerUpdate, RankedInfo, RoomId, RoomInfo, SentInput, ServerMessage,
    Targeting, Thumb, MAX_CHAT_CHARS, MAX_RESENT_INPUTS, MAX_SPECTATORS, MAX_USERNAME_CHARS, OUTDATED_FRAME,
    PROTOCOL_VERSION,
};
pub use replay::{Playback, Replay, ReplayPlayer};
pub use room::{PausePolicy, RoomSetting, RoomSettings, BOARD_COLUMNS, BOARD_ROWS, MAX_ROOM_PLAYERS, MAX_TEAMS};

// Grid coordinates and cell counts are tiny (the grid is 6 by 13), so they fit
// any integer type; this checks it rather than truncating silently, since a
// wrong value here would desync the two players.
fn small<T: TryFrom<usize>>(v: usize) -> T {
    T::try_from(v).unwrap_or_else(|_| panic!("{v} is not a grid-sized value"))
}

#[cfg(test)]
mod tests;
