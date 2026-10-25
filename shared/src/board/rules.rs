use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::config::{GRID_HEIGHT, GRID_WIDTH, VISIBLE_ROW_OFFSET};
use crate::small;

/// What a board is played with: its size, without the hidden row, the
/// level it starts at and how many colours its pairs come in.
#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    pub columns: u8,
    pub rows: u8,
    pub starting_level: u32,
    pub colors: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            columns: small(GRID_WIDTH),
            rows: small(GRID_HEIGHT - VISIBLE_ROW_OFFSET),
            starting_level: 1,
            colors: 5,
        }
    }
}

impl Rules {
    /// The grid's height, hidden row included.
    pub fn height(&self) -> usize {
        usize::from(self.rows) + VISIBLE_ROW_OFFSET
    }
}
