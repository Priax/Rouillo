use shared::{config, Board, GameState, InputKind, RoomSettings};

use crate::cpu::{Cpu, Difficulty};
use crate::state::TurnAnim;

/// A game the hard CPU plays alone, chosen because it fires a 5-chain early.
const SEED: u64 = 86;
/// The tick at which that game's 5-chain starts popping.
const CHAIN_TICK: u32 = 903;
/// How much of the game is shown before the chain.
const LEAD_TICKS: u32 = 15 * 60;
const SPEED: f32 = 0.6;
const RESTART_AFTER: f32 = 4.0;

const STEP: f32 = 1.0 / config::SERVER_TICK_HZ as f32;

pub struct Demo {
    pub board: Board,
    cpu: Cpu,
    pub turn: TurnAnim,
    ticks: u32,
    clock: f32,
    pub caption: Option<(&'static str, f32)>,
    pub chain: Option<(u32, f32)>,
    restart_in: Option<f32>,
}

fn tick(board: &mut Board, cpu: &mut Cpu) -> Option<InputKind> {
    let input = cpu.input(board, 0);
    board.step(input, 0);
    input
}

const fn caption(input: InputKind) -> Option<&'static str> {
    match input {
        InputKind::MoveLeft => Some("Gauche"),
        InputKind::MoveRight => Some("Droite"),
        InputKind::RotateCW => Some("Tourner (horaire)"),
        InputKind::RotateCCW => Some("Tourner (anti-horaire)"),
        InputKind::SoftDropPress => Some("Descendre"),
        InputKind::HardDrop => Some("Poser"),
        InputKind::SoftDropRelease => None,
    }
}

impl Demo {
    pub fn new() -> Self {
        let mut board = Board::for_match(SEED, &RoomSettings::default());
        let mut cpu = Cpu::new(Difficulty::Hard, SEED);
        let start = CHAIN_TICK.saturating_sub(LEAD_TICKS);
        for _ in 0..start {
            tick(&mut board, &mut cpu);
        }
        Self {
            board,
            cpu,
            turn: TurnAnim::default(),
            ticks: start,
            clock: 0.0,
            caption: None,
            chain: None,
            restart_in: None,
        }
    }

    pub fn update(&mut self, dt: f32) {
        if let Some(left) = self.restart_in.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                *self = Self::new();
            }
            return;
        }
        self.clock += dt * SPEED;
        while self.clock >= STEP {
            self.clock -= STEP;
            self.step();
        }
        if let Some(p) = &self.board.active_piece {
            self.turn.update(self.board.piece_id, p.rotation, dt * SPEED);
        }
        if let Some((_, left)) = self.caption.as_mut() {
            *left -= dt;
        }
        if let Some((_, left)) = self.chain.as_mut() {
            *left -= dt;
        }
        self.caption = self.caption.filter(|c| c.1 > 0.0);
        self.chain = self.chain.filter(|c| c.1 > 0.0);
    }

    fn step(&mut self) {
        let before = self.board.chain_count;
        if let Some(text) = tick(&mut self.board, &mut self.cpu).and_then(caption) {
            self.caption = Some((text, 1.2));
        }
        self.ticks += 1;
        let chain = self.board.chain_count;
        if chain > before {
            self.chain = Some((chain, 2.0));
        }
        let settled = self.board.state == GameState::Playing && chain == 0;
        let finished = self.ticks > CHAIN_TICK + 60 && settled;
        if finished || self.board.state == GameState::GameOver {
            self.restart_in = Some(RESTART_AFTER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET_CHAIN: u32 = 5;

    /// The tick at which `seed`'s game first pops a chain of `TARGET_CHAIN`.
    fn chain_tick(seed: u64, limit: u32) -> Option<u32> {
        let mut board = Board::for_match(seed, &RoomSettings::default());
        let mut cpu = Cpu::new(Difficulty::Hard, seed);
        (1..=limit)
            .find(|_| {
                tick(&mut board, &mut cpu);
                board.chain_count >= TARGET_CHAIN || board.state == GameState::GameOver
            })
            .filter(|_| board.chain_count >= TARGET_CHAIN)
    }

    #[test]
    #[ignore = "picks SEED and CHAIN_TICK: cargo test -p client --release demo::tests::pick -- --ignored --nocapture"]
    fn pick() {
        let best = (0..300)
            .filter_map(|seed| chain_tick(seed, 30_000).map(|t| (t, seed)))
            .min();
        println!("fastest {TARGET_CHAIN}-chain: {best:?} (tick, seed)");
    }

    #[test]
    fn the_demo_seed_fires_its_chain_on_time() {
        assert_eq!(chain_tick(SEED, CHAIN_TICK + 1), Some(CHAIN_TICK));
    }

    #[test]
    fn the_demo_shows_the_chain_then_starts_again() {
        let mut demo = Demo::new();
        let mut best = 0;
        for _ in 0..(LEAD_TICKS + 900) * 2 {
            demo.update(STEP / SPEED);
            best = best.max(demo.chain.map_or(0, |c| c.0));
            if demo.restart_in.is_some() {
                break;
            }
        }
        assert!(best >= TARGET_CHAIN, "best chain shown: {best}");
        assert!(demo.restart_in.is_some(), "the demo never looped");
    }
}
