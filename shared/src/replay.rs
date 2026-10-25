use bitcode::{Decode, Encode};

use crate::board::{Board, Events, InputKind, Rules};

/// A finished game as the server played it: every board starts from the
/// same seed, and takes the same inputs and nuisance on the same ticks.
#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub seed: u64,
    pub rules: Rules,
    /// How many ticks the game lasted.
    pub ticks: u32,
    pub players: Vec<ReplayPlayer>,
}

/// One slot of a replay, from slot 1.
#[derive(Encode, Decode, Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayPlayer {
    pub name: String,
    pub team: u8,
    /// The inputs the board took, each with the tick it took it on, in order.
    pub inputs: Vec<(u32, InputKind)>,
    /// The nuisance that landed on the board, each with its tick, in order.
    pub garbage: Vec<(u32, u32)>,
    /// The tick after which the player forfeited, if they did.
    pub out_at: Option<u32>,
}

/// A replay being played, a tick at a time.
#[derive(Clone)]
pub struct Playback {
    replay: Replay,
    boards: Vec<Board>,
    tick: u32,
    next_input: Vec<usize>,
    next_garbage: Vec<usize>,
}

impl Replay {
    pub fn play(self) -> Playback {
        let boards = vec![Board::for_match(self.seed, &self.rules); self.players.len()];
        let n = self.players.len();
        let mut playback = Playback {
            replay: self,
            boards,
            tick: 0,
            next_input: vec![0; n],
            next_garbage: vec![0; n],
        };
        playback.forfeits();
        playback
    }
}

impl Playback {
    pub const fn replay(&self) -> &Replay {
        &self.replay
    }

    pub fn boards(&self) -> &[Board] {
        &self.boards
    }

    pub const fn tick(&self) -> u32 {
        self.tick
    }

    pub const fn done(&self) -> bool {
        self.tick >= self.replay.ticks
    }

    /// Plays the next tick on every board: what each board did.
    pub fn step(&mut self) -> Vec<Events> {
        if self.done() {
            return vec![Events::default(); self.boards.len()];
        }
        self.tick += 1;
        let t = self.tick;
        let events = (0..self.boards.len())
            .map(|slot| {
                let player = &self.replay.players[slot];
                let inputs = &player.inputs[self.next_input[slot]..];
                let taken = inputs.iter().take_while(|&&(at, _)| at <= t).count();
                self.next_input[slot] += taken;
                let garbage = &player.garbage[self.next_garbage[slot]..];
                let landed = garbage.iter().take_while(|&&(at, _)| at <= t).count();
                self.next_garbage[slot] += landed;
                let amount = garbage[..landed].iter().map(|&(_, amount)| amount).sum();
                self.boards[slot].step(inputs[..taken].iter().map(|&(_, kind)| kind), amount)
            })
            .collect();
        self.forfeits();
        events
    }

    fn forfeits(&mut self) {
        for (board, player) in self.boards.iter_mut().zip(&self.replay.players) {
            if player.out_at == Some(self.tick) {
                board.lose();
            }
        }
    }
}
