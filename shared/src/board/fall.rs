use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::grid::Move;
use crate::config::{BOUNCE_FRAMES, CELL_PX, FREE_FALL_ACCEL, FREE_FALL_MAX, FREE_FALL_START, OJAMA_ACCEL, PX_UNITS};
use crate::small;

/// Puyos falling into the cells they already hold on the grid, and how far
/// through it they are. Nuisance falls slower, and cannot pop: the next pair
/// comes as soon as it lands.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct Fall {
    pub frame: u32,
    pub frames: u32,
    pub cells: Vec<CellFall>,
    pub ojama: bool,
}

/// A puyo falling into `(row, col)` from `cells_fallen` rows higher.
#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct CellFall {
    pub row: u8,
    pub col: u8,
    pub cells_fallen: u8,
    pub delay: u8,
}

pub fn fallen_px(frames: u32, start: u32, accel: u32) -> u64 {
    let (mut y, mut v) = (0u64, start);
    for _ in 0..frames {
        y += u64::from(v);
        v = (v + accel).min(FREE_FALL_MAX);
    }
    y
}

pub fn frames_to_fall(cells: u32, start: u32, accel: u32) -> u32 {
    let target = u64::from(cells * CELL_PX) * u64::from(PX_UNITS);
    let (mut y, mut v, mut frames) = (0u64, start, 0);
    while y < target {
        y += u64::from(v);
        v = (v + accel).min(FREE_FALL_MAX);
        frames += 1;
    }
    frames
}

impl Fall {
    pub(super) fn new(cells: Vec<CellFall>, ojama: bool) -> Self {
        let mut fall = Self {
            frame: 0,
            frames: 0,
            cells,
            ojama,
        };
        fall.frames = fall
            .cells
            .iter()
            .map(|c| fall.lands_at(c) + BOUNCE_FRAMES)
            .max()
            .unwrap_or(0);
        fall
    }

    fn start_and_accel(&self, cell: &CellFall) -> (u32, u32) {
        if self.ojama {
            (0, OJAMA_ACCEL[cell.col as usize % OJAMA_ACCEL.len()])
        } else {
            (FREE_FALL_START, FREE_FALL_ACCEL)
        }
    }

    /// The frame `cell` reaches its cell on.
    pub fn lands_at(&self, cell: &CellFall) -> u32 {
        let (start, accel) = self.start_and_accel(cell);
        u32::from(cell.delay) + frames_to_fall(u32::from(cell.cells_fallen), start, accel)
    }

    /// How many cells above its own `cell` is drawn now.
    pub fn height(&self, cell: &CellFall) -> f32 {
        let (start, accel) = self.start_and_accel(cell);
        let moving = self.frame.saturating_sub(u32::from(cell.delay));
        let total = u64::from(u32::from(cell.cells_fallen) * CELL_PX) * u64::from(PX_UNITS);
        let left = total.saturating_sub(fallen_px(moving, start, accel));
        left as f32 / (CELL_PX * PX_UNITS) as f32
    }

    /// How far through its bounce `cell` is, while it bounces.
    pub fn bounce(&self, cell: &CellFall) -> Option<f32> {
        let since = self.frame.checked_sub(self.lands_at(cell))?;
        (since < BOUNCE_FRAMES).then(|| since as f32 / BOUNCE_FRAMES as f32)
    }
}

impl CellFall {
    pub(super) fn new(row: usize, col: usize, cells_fallen: usize) -> Self {
        Self {
            row: small(row),
            col: small(col),
            cells_fallen: small(cells_fallen),
            delay: 0,
        }
    }
}

impl From<Move> for CellFall {
    fn from(m: Move) -> Self {
        Self::new(m.to, m.col, m.to - m.from)
    }
}
