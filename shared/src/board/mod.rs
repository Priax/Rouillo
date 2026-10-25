use std::hash::{Hash, Hasher};

use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

mod fall;
mod grid;
mod pair;
mod piece;
mod rng;
mod rules;
mod settle;
#[cfg(test)]
mod tests;
mod thumb;

pub use fall::{fallen_px, frames_to_fall, CellFall, Fall};
pub use grid::Grid;
pub use pair::PairState;
pub use piece::{ActivePuyo, PuyoType, Rotation};
use rng::{BoardRng, Stream};
pub use rules::Rules;
pub use settle::{group_bonus, link_score};

use crate::config::{self, LEVEL_FRAMES, MARGIN_LEVELS, MARGIN_STEPS, TARGET_POINTS, VISIBLE_ROW_OFFSET};
use crate::hash::Fnv;

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    MoveLeft,
    MoveRight,
    RotateCW,
    RotateCCW,
    SoftDropPress,
    SoftDropRelease,
    HardDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StampedInput {
    pub tick: u32,
    pub kind: InputKind,
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingGarbage {
    pub at: u32,
    pub amount: u32,
}

/// What a board is doing, as the rest of the game sees it.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum GameState {
    Playing,
    ResolvingMatches,
    DroppingGarbage,
    GameOver,
    Paused,
}

/// What an input or a tick did, for whoever reacts to it: sounds,
/// statistics, lessons. A replay does not report them again.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Events {
    /// Nuisance sent to the opponents.
    pub sent: u32,
    /// The pair landed and is out of the player's hands.
    pub locked: bool,
    /// A chain link started popping: its number, from 1.
    pub link: Option<u32>,
    pub all_clear: bool,
    /// Nuisance started falling onto the board.
    pub ojama_dropped: bool,
    /// A new pair appeared.
    pub spawned: bool,
    pub topped_out: bool,
}

impl Events {
    fn merge(&mut self, other: Self) {
        self.sent += other.sent;
        self.locked |= other.locked;
        self.link = other.link.or(self.link);
        self.all_clear |= other.all_clear;
        self.ojama_dropped |= other.ojama_dropped;
        self.spawned |= other.spawned;
        self.topped_out |= other.topped_out;
    }
}

/// Where a board is in its turn, with what only that step needs.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum Phase {
    /// Built, before its first pair: nothing moves until `spawn_piece`.
    Waiting,
    /// The player steers a pair.
    Pair(PairState),
    Falling(Fall),
    /// Groups flashing before they vanish.
    Popping {
        frame: u32,
        cells: Vec<(u8, u8)>,
    },
    /// Everything at rest: the next tick pops, drops nuisance or serves a pair.
    Settled,
    GameOver,
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Debug, Hash)]
pub struct Board {
    cells: Grid,
    start_level: u32,
    colors: u32,
    phase: Phase,
    paused: bool,
    pub piece_id: u32,
    pub next_types: (PuyoType, PuyoType),
    pub next_next_types: (PuyoType, PuyoType),
    pub score: u32,
    pub pending_garbage: u32,
    pub nuisance_points: u32,
    pub chain_count: u32,
    pub last_was_all_clear: bool,
    all_clear_bonus: bool,
    match_frames: u32,
    pub soft_dropping: bool,
    rng: BoardRng,
}

impl Board {
    /// A board waiting for its first pair.
    pub fn new(seed: u64, rules: &Rules) -> Self {
        let mut rng = BoardRng::from_seed(seed);
        let colors = rules.colors;
        let (n1, n2) = rng.draw(Stream::Pieces, |r| {
            (PuyoType::random_pair(r, colors), PuyoType::random_pair(r, colors))
        });
        let mut cells = Grid::new(usize::from(rules.columns), rules.height());
        let rows = config::starting_garbage_rows(rules.starting_level) as usize;
        for row in cells.rows_mut().rev().take(rows) {
            row.fill(Some(PuyoType::Garbage));
        }
        Self {
            cells,
            start_level: rules.starting_level,
            colors,
            phase: Phase::Waiting,
            paused: false,
            piece_id: 0,
            next_types: n1,
            next_next_types: n2,
            score: 0,
            pending_garbage: 0,
            nuisance_points: 0,
            chain_count: 0,
            last_was_all_clear: false,
            all_clear_bonus: false,
            match_frames: 0,
            soft_dropping: false,
            rng,
        }
    }

    /// A board with its first pair in play.
    pub fn for_match(seed: u64, rules: &Rules) -> Self {
        let mut board = Self::new(seed, rules);
        board.spawn_piece();
        board
    }

    pub const fn width(&self) -> usize {
        self.cells.width()
    }

    pub const fn height(&self) -> usize {
        self.cells.height()
    }

    pub const fn cells(&self) -> &Grid {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut Grid {
        &mut self.cells
    }

    pub const fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn state(&self) -> GameState {
        match self.phase {
            Phase::GameOver => GameState::GameOver,
            _ if self.paused => GameState::Paused,
            Phase::Waiting | Phase::Pair(_) => GameState::Playing,
            Phase::Falling(Fall { ojama: true, .. }) => GameState::DroppingGarbage,
            Phase::Falling(_) | Phase::Popping { .. } | Phase::Settled => GameState::ResolvingMatches,
        }
    }

    /// The pair the player steers, if one is in play.
    pub const fn pair(&self) -> Option<&PairState> {
        match &self.phase {
            Phase::Pair(pair) => Some(pair),
            _ => None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn pair_mut(&mut self) -> Option<&mut PairState> {
        match &mut self.phase {
            Phase::Pair(pair) => Some(pair),
            _ => None,
        }
    }

    pub const fn piece(&self) -> Option<&ActivePuyo> {
        match self.pair() {
            Some(pair) => Some(&pair.piece),
            None => None,
        }
    }

    pub fn piece_mut(&mut self) -> Option<&mut ActivePuyo> {
        match &mut self.phase {
            Phase::Pair(pair) => Some(&mut pair.piece),
            _ => None,
        }
    }

    /// Puts `piece` in play, as if it had just appeared there.
    pub fn set_piece(&mut self, piece: ActivePuyo) {
        self.phase = Phase::Pair(PairState::new(piece));
    }

    /// Takes the pair out of play: the board waits for the next one.
    pub fn take_piece(&mut self) -> Option<ActivePuyo> {
        let piece = self.piece().copied();
        if piece.is_some() {
            self.phase = Phase::Waiting;
        }
        piece
    }

    /// Lets the stack resolve from the next tick, as it does once a pair has
    /// landed: pops, then nuisance, then the next pair.
    #[cfg(any(test, feature = "test-support"))]
    pub fn settle(&mut self) {
        self.phase = Phase::Settled;
    }

    /// Drops the nuisance waiting now, as the end of a turn does.
    #[cfg(any(test, feature = "test-support"))]
    pub fn drop_pending_garbage(&mut self) {
        self.drop_garbage(&mut Events::default());
    }

    /// Ends this board's game, as topping out or a forfeit does.
    pub fn lose(&mut self) {
        self.phase = Phase::GameOver;
    }

    /// The third column on the standard six.
    pub const fn spawn_col(&self) -> usize {
        self.width().saturating_sub(1) / 2
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn target_points(&self) -> u32 {
        let steps = self.level().saturating_sub(MARGIN_LEVELS).min(MARGIN_STEPS);
        let target = u64::from(TARGET_POINTS) * 3u64.pow(steps) / 4u64.pow(steps);
        u32::try_from(target.max(1)).unwrap_or(u32::MAX)
    }

    pub fn level(&self) -> u32 {
        self.start_level + self.match_frames / LEVEL_FRAMES
    }

    /// How close to losing: the tallest column, then how many cells are taken.
    pub fn danger(&self) -> (usize, usize) {
        let peak = (0..self.width())
            .map(|c| self.cells.column_height(c))
            .max()
            .unwrap_or(0);
        let filled = self.cells.iter().filter(|c| c.is_some()).count();
        (peak, filled)
    }

    /// Whether a board that came in a message holds together: a grid of
    /// its own size, with room to spawn, and a phase that stays on it.
    pub fn is_well_formed(&self) -> bool {
        let on_grid = |r: u8, c: u8| self.cells.checked(i32::from(r), i32::from(c)).is_some();
        self.cells.is_well_formed()
            && self.height() > VISIBLE_ROW_OFFSET
            && match &self.phase {
                Phase::Pair(pair) => pair
                    .piece
                    .positions()
                    .into_iter()
                    .all(|(r, c)| self.cells.checked(r, c).is_some()),
                Phase::Falling(fall) => fall.cells.iter().all(|f| on_grid(f.row, f.col)),
                Phase::Popping { cells, .. } => cells.iter().all(|&(r, c)| on_grid(r, c)),
                Phase::Waiting | Phase::Settled | Phase::GameOver => true,
            }
    }

    pub fn apply_input(&mut self, input: InputKind) -> Events {
        let mut events = Events::default();
        match input {
            InputKind::SoftDropPress => self.soft_dropping = true,
            InputKind::SoftDropRelease => self.soft_dropping = false,
            _ => {}
        }
        if self.paused {
            return events;
        }
        match input {
            InputKind::MoveLeft => self.move_piece(-1),
            InputKind::MoveRight => self.move_piece(1),
            InputKind::RotateCW => self.rotate_piece(Rotation::cw, &mut events),
            InputKind::RotateCCW => self.rotate_piece(Rotation::ccw, &mut events),
            InputKind::HardDrop => self.hard_drop(&mut events),
            InputKind::SoftDropPress | InputKind::SoftDropRelease => {}
        }
        events
    }

    /// A digest of the whole state, the same on every platform: two boards
    /// with the same digest play on identically.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv::new();
        self.hash(&mut h);
        h.finish()
    }

    pub fn tick(&mut self) -> Events {
        let mut events = Events::default();
        if self.paused || self.phase == Phase::GameOver {
            return events;
        }
        self.match_frames += 1;
        match self.phase {
            Phase::Pair(_) => self.tick_pair(&mut events),
            Phase::Falling(_) | Phase::Popping { .. } | Phase::Settled => self.tick_settle(&mut events),
            Phase::Waiting | Phase::GameOver => {}
        }
        events
    }

    /// The inputs, then a tick, then the nuisance landing on this tick.
    pub fn step(&mut self, inputs: impl IntoIterator<Item = InputKind>, landed_garbage: u32) -> Events {
        let mut events = Events::default();
        for kind in inputs {
            events.merge(self.apply_input(kind));
        }
        events.merge(self.tick());
        self.pending_garbage += landed_garbage;
        events
    }

    #[must_use]
    pub fn replay(&self, from: u32, to: u32, inputs: &[StampedInput], incoming: &[IncomingGarbage]) -> Self {
        let mut board = self.clone();
        let mut next = 0;
        let mut t = from;
        while t < to {
            t += 1;
            let start = next;
            while next < inputs.len() && inputs[next].tick <= t {
                next += 1;
            }
            let landed = incoming.iter().filter(|g| g.at == t).map(|g| g.amount).sum();
            board.step(inputs[start..next].iter().map(|i| i.kind), landed);
        }
        for input in &inputs[next..] {
            board.apply_input(input.kind);
        }
        board
    }
}
