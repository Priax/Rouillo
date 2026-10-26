use std::ops::RangeInclusive;

use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::board::Rules;

/// The teams a lobby offers; a player in none plays for themselves.
pub const MAX_TEAMS: u8 = 4;
pub const MAX_ROOM_PLAYERS: u8 = 16;

/// The team of the player in `seat`, from 0, who picked none: one of their
/// own, past those one may pick.
pub const fn own_team(seat: u8) -> u8 {
    MAX_TEAMS + 1 + seat
}
/// The board sizes a room may pick, hidden row aside.
pub const BOARD_COLUMNS: RangeInclusive<u8> = 4..=10;
pub const BOARD_ROWS: RangeInclusive<u8> = 8..=16;

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PausePolicy {
    Everyone,
    HostOnly,
    Nobody,
}

impl PausePolicy {
    const ALL: [Self; 3] = [Self::Everyone, Self::HostOnly, Self::Nobody];

    pub fn allows(self, is_host: bool) -> bool {
        match self {
            Self::Everyone => true,
            Self::HostOnly => is_host,
            Self::Nobody => false,
        }
    }

    fn step(self, dir: i32) -> Self {
        let n = Self::ALL.len();
        let i = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        let next = match dir.signum() {
            1 => (i + 1) % n,
            -1 => (i + n - 1) % n,
            _ => i,
        };
        Self::ALL[next]
    }
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, Debug)]
pub struct RoomSettings {
    pub rules: Rules,
    pub friends_only: bool,
    pub pause: PausePolicy,
    pub max_players: u8,
}

/// One of the `RoomSettings` a host changes in the lobby, in the order the
/// lobby lists them.
#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomSetting {
    StartingLevel,
    Colors,
    Columns,
    Rows,
    FriendsOnly,
    Pause,
    MaxPlayers,
}

impl RoomSetting {
    pub const ALL: [Self; 7] = [
        Self::StartingLevel,
        Self::Colors,
        Self::Columns,
        Self::Rows,
        Self::FriendsOnly,
        Self::Pause,
        Self::MaxPlayers,
    ];
}

impl Default for RoomSettings {
    fn default() -> Self {
        Self {
            rules: Rules::default(),
            friends_only: false,
            pause: PausePolicy::Everyone,
            max_players: 2,
        }
    }
}

impl RoomSettings {
    pub fn adjust(&mut self, setting: RoomSetting, dir: i32) {
        let rules = &mut self.rules;
        match setting {
            RoomSetting::StartingLevel => {
                rules.starting_level = rules.starting_level.saturating_add_signed(dir).clamp(1, 15);
            }
            RoomSetting::Colors => rules.colors = rules.colors.saturating_add_signed(dir).clamp(4, 5),
            RoomSetting::Columns => rules.columns = step_clamped(rules.columns, dir, BOARD_COLUMNS),
            RoomSetting::Rows => rules.rows = step_clamped(rules.rows, dir, BOARD_ROWS),
            RoomSetting::FriendsOnly => self.friends_only = !self.friends_only,
            RoomSetting::Pause => self.pause = self.pause.step(dir),
            RoomSetting::MaxPlayers => self.max_players = step_clamped(self.max_players, dir, 2..=MAX_ROOM_PLAYERS),
        }
    }
}

fn step_clamped(value: u8, dir: i32, range: RangeInclusive<u8>) -> u8 {
    match dir.signum() {
        1 => value.saturating_add(1),
        -1 => value.saturating_sub(1),
        _ => value,
    }
    .clamp(*range.start(), *range.end())
}
