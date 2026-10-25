use bitcode::{Decode, Encode};
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub enum PuyoType {
    Red,
    Blue,
    Yellow,
    Green,
    Purple,
    Garbage,
}

impl PuyoType {
    pub(super) fn random_pair<R: Rng>(rng: &mut R, colors: u32) -> (Self, Self) {
        (Self::random(rng, colors), Self::random(rng, colors))
    }

    fn random<R: Rng>(rng: &mut R, colors: u32) -> Self {
        let n = colors.clamp(1, 5);
        match rng.random_range(0..n) {
            0 => Self::Red,
            1 => Self::Blue,
            2 => Self::Yellow,
            3 => Self::Green,
            _ => Self::Purple,
        }
    }
}

/// Where the satellite sits around the axis.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum Rotation {
    Up,
    Right,
    Down,
    Left,
}

impl Rotation {
    pub const ALL: [Self; 4] = [Self::Up, Self::Right, Self::Down, Self::Left];

    /// The satellite's offset from the axis, in rows then columns.
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Self::Up => (-1, 0),
            Self::Right => (0, 1),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
        }
    }

    pub const fn quarter_turns(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub const fn cw(self) -> Self {
        Self::ALL[(self as usize + 1) % 4]
    }

    #[must_use]
    pub const fn ccw(self) -> Self {
        Self::ALL[(self as usize + 3) % 4]
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        Self::ALL[(self as usize + 2) % 4]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode, Debug)]
pub struct ActivePuyo {
    pub row: i32,
    pub col: i32,
    pub rotation: Rotation,
    pub axis_type: PuyoType,
    pub sat_type: PuyoType,
}

impl ActivePuyo {
    /// The axis, then the satellite.
    pub fn positions(&self) -> [(i32, i32); 2] {
        let (dr, dc) = self.rotation.offset();
        [(self.row, self.col), (self.row + dr, self.col + dc)]
    }

    /// The axis, then the satellite, with their colours.
    pub fn puyos(&self) -> [((i32, i32), PuyoType); 2] {
        let [axis, sat] = self.positions();
        [(axis, self.axis_type), (sat, self.sat_type)]
    }

    pub(super) const fn shifted(self, rows: i32, cols: i32) -> Self {
        Self {
            row: self.row + rows,
            col: self.col + cols,
            ..self
        }
    }
}
