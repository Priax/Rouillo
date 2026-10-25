use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::{ActivePuyo, Board, CellFall, Events, Fall, Grid, Phase, PuyoType, Rotation, Stream};
use crate::config::{
    CELL_UNITS, FALL_FRAMES_PER_CELL, GRACE_FRAMES, HALF_CELL_UNITS, LEVELS_PER_SPEEDUP, MAX_PUSH_BACKS,
    MIN_FALL_FRAMES_PER_CELL, SOFT_DROP_UNITS, SPLIT_DELAY_AXIS, SPLIT_DELAY_SATELLITE, VISIBLE_ROW_OFFSET,
};
use crate::small;

/// The pair in play, and how far it is through its fall and its grace.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct PairState {
    pub piece: ActivePuyo,
    /// How far below its row the pair has fallen, in `CELL_UNITS` a cell.
    pub fall_offset: u32,
    ground_frames: u32,
    push_backs: u32,
}

impl PairState {
    pub(super) const fn new(piece: ActivePuyo) -> Self {
        Self {
            piece,
            fall_offset: 0,
            ground_frames: 0,
            push_backs: 0,
        }
    }

    /// How long the pair has rested on something, out of `GRACE_FRAMES`.
    pub const fn ground_frames(&self) -> u32 {
        self.ground_frames
    }
}

/// Whether `piece` may stand there: past half a cell of its fall, the row
/// below must be free too.
fn fits(grid: &Grid, piece: &ActivePuyo, fall_offset: u32) -> bool {
    !grid.collides(piece) && (fall_offset < HALF_CELL_UNITS || !grid.rests(piece))
}

impl Board {
    pub fn spawn_piece(&mut self) {
        self.spawn(&mut Events::default());
    }

    pub(super) fn spawn(&mut self, events: &mut Events) {
        if self.cells[(VISIBLE_ROW_OFFSET, self.spawn_col())].is_some() {
            self.phase = Phase::GameOver;
            events.topped_out = true;
            return;
        }
        let (axis_type, sat_type) = self.next_types;
        self.next_types = self.next_next_types;
        let colors = self.colors;
        self.next_next_types = self.rng.draw(Stream::Pieces, |r| PuyoType::random_pair(r, colors));
        let piece = ActivePuyo {
            row: 1,
            col: small(self.spawn_col()),
            rotation: Rotation::Up,
            axis_type,
            sat_type,
        };
        if self.cells.collides(&piece) {
            self.phase = Phase::GameOver;
            events.topped_out = true;
        } else {
            self.piece_id = self.piece_id.wrapping_add(1);
            self.chain_count = 0;
            self.set_piece(piece);
            events.spawned = true;
        }
    }

    /// Where the pair would land, dropped now.
    pub fn ghost_piece(&self) -> Option<ActivePuyo> {
        let mut ghost = *self.piece()?;
        while !self.cells.rests(&ghost) {
            ghost.row += 1;
        }
        Some(ghost)
    }

    pub fn can_fall(&self) -> bool {
        self.piece().is_some_and(|p| !self.cells.rests(p))
    }

    pub fn fall_progress(&self) -> f32 {
        match self.pair() {
            Some(pair) if self.can_fall() => pair.fall_offset as f32 / CELL_UNITS as f32,
            _ => 0.0,
        }
    }

    pub(super) fn fall_frames_per_cell(&self) -> u32 {
        FALL_FRAMES_PER_CELL
            .saturating_sub(self.level().saturating_sub(1) / LEVELS_PER_SPEEDUP)
            .max(MIN_FALL_FRAMES_PER_CELL)
    }

    pub(super) fn move_piece(&mut self, cols: i32) {
        let Phase::Pair(pair) = &mut self.phase else {
            return;
        };
        let moved = pair.piece.shifted(0, cols);
        if fits(&self.cells, &moved, pair.fall_offset) {
            pair.piece = moved;
        }
    }

    pub(super) fn rotate_piece(&mut self, turn: fn(Rotation) -> Rotation, events: &mut Events) {
        let Phase::Pair(pair) = &mut self.phase else {
            return;
        };
        let piece = pair.piece;
        let rotation = turn(piece.rotation);
        let (row, col) = (piece.row, piece.col);
        let (sat_row, sat_col) = piece.positions()[1];

        let candidates = [
            (row, col, rotation, true, false),                            // in place
            (row, col - 1, rotation, rotation == Rotation::Right, false), // off a wall on the right
            (row, col + 1, rotation, rotation == Rotation::Left, false),  // off a wall on the left
            (row - 1, col, rotation, rotation == Rotation::Down, true),   // off the floor
            (sat_row, sat_col, piece.rotation.opposite(), true, false),   // pivot on satellite
        ];
        for (row, col, rotation, applies, push_back) in candidates {
            let turned = ActivePuyo {
                row,
                col,
                rotation,
                ..piece
            };
            if !applies || !fits(&self.cells, &turned, pair.fall_offset) {
                continue;
            }
            if push_back {
                if pair.push_backs >= MAX_PUSH_BACKS {
                    self.lock_piece(events);
                    return;
                }
                pair.push_backs += 1;
                pair.fall_offset = 0;
            }
            pair.piece = turned;
            return;
        }
    }

    pub(super) fn hard_drop(&mut self, events: &mut Events) {
        let Phase::Pair(pair) = &mut self.phase else {
            return;
        };
        let mut cells = 0;
        while !self.cells.rests(&pair.piece) {
            pair.piece.row += 1;
            cells += 1;
        }
        self.add_drop_bonus(cells);
        self.lock_piece(events);
    }

    fn add_drop_bonus(&mut self, points: u32) {
        self.score = self.score.saturating_add(points);
        self.nuisance_points = self.nuisance_points.saturating_add(points);
    }

    pub(super) fn tick_pair(&mut self, events: &mut Events) {
        let speed = if self.soft_dropping {
            SOFT_DROP_UNITS
        } else {
            CELL_UNITS / self.fall_frames_per_cell()
        };
        let Phase::Pair(pair) = &mut self.phase else {
            return;
        };
        let mut dropped = 0;
        if !self.cells.rests(&pair.piece) {
            pair.fall_offset += speed;
            while pair.fall_offset >= CELL_UNITS && !self.cells.rests(&pair.piece) {
                pair.fall_offset -= CELL_UNITS;
                pair.piece.row += 1;
                if self.soft_dropping {
                    dropped += 1;
                }
            }
        }
        let lock = if self.cells.rests(&pair.piece) {
            pair.fall_offset = 0;
            pair.ground_frames += 1;
            self.soft_dropping || pair.ground_frames > GRACE_FRAMES
        } else {
            false
        };
        self.add_drop_bonus(dropped);
        if lock {
            self.lock_piece(events);
        }
    }

    fn lock_piece(&mut self, events: &mut Events) {
        self.last_was_all_clear = false;
        let Some(piece) = self.piece().copied() else {
            return;
        };
        let mut placed = Vec::new();
        for (((r, c), puyo), delay) in piece.puyos().into_iter().zip([SPLIT_DELAY_AXIS, SPLIT_DELAY_SATELLITE]) {
            if let Some(at) = self.cells.checked(r, c) {
                self.cells[at] = Some(puyo);
                placed.push((at, delay));
            }
        }
        let moved = self.cells.collapse();
        let falls = placed
            .into_iter()
            .map(|((r, c), delay)| {
                let fall = moved
                    .iter()
                    .find(|m| (m.from, m.col) == (r, c))
                    .map_or_else(|| CellFall::new(r, c, 0), |&m| m.into());
                CellFall { delay, ..fall }
            })
            .collect();
        self.phase = Phase::Falling(Fall::new(falls, false));
        events.locked = true;
    }
}
