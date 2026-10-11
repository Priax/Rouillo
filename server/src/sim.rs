use rand::RngExt;
use shared::{
    config, Board, GameState, IncomingGarbage, InputKind, PlayerUpdate, RoomSettings, ServerMessage, Targeting,
};
use tokio::time::Instant;
use tracing::warn;

use crate::ws::CLIENT_MSG_BURST;

pub const INPUT_QUEUE_CAP: usize = 2048;

/// How long a random target is kept before another is drawn.
const RANDOM_TARGET_TICKS: u32 = 8 * config::SERVER_TICK_HZ as u32;

const _: () = assert!(INPUT_QUEUE_CAP as f64 > CLIENT_MSG_BURST);

pub struct GarbageDelivery {
    pub at: u32,
    from: usize,
    to: usize,
    amount: u32,
}

pub struct PendingInput {
    at: u32,
    seq: u32,
    kind: InputKind,
}

/// One game: a board per slot, and what travels between them. Every slot
/// starts from the same seed, so all players get the same pieces.
pub struct Sim {
    pub boards: Vec<Board>,
    /// Slots on the same team never attack each other.
    pub teams: Vec<u8>,
    pub targeting: Vec<Targeting>,
    /// Where each slot's nuisance goes now, kept up to date every tick.
    pub targets: Vec<Option<usize>>,
    random_until: Vec<u32>,
    pub tick: u32,
    /// Which of the room's games this is.
    pub game: u32,
    pub queued_inputs: Vec<Vec<PendingInput>>,
    pub garbage_in_flight: Vec<GarbageDelivery>,
    pub late_inputs: Vec<u32>,
    pub paused: bool,
    pub finished: bool,
    pub last_seq: Vec<u32>,
    pub last_restart: Option<Instant>,
    pub start: Instant,
    pub max_chain: Vec<u32>,
    pub total_chains: Vec<u32>,
    pub nuisance_sent: Vec<u32>,
    pub nuisance_received: Vec<u32>,
    /// The tick each slot was knocked out at, for the final ranking.
    pub out_at: Vec<Option<u32>>,
    pub all_clears: Vec<u32>,
    pub pieces_placed: Vec<u32>,
    prev_chain: Vec<u32>,
    prev_all_clear: Vec<bool>,
    prev_piece_id: Vec<u32>,
}

impl Sim {
    /// A game for `teams.len()` slots, each on the team given.
    pub fn new(settings: &RoomSettings, teams: Vec<u8>) -> Self {
        let n = teams.len();
        let board = Board::for_match(rand::rng().random(), settings);
        let piece_id = board.piece_id;
        Self {
            boards: vec![board; n],
            teams,
            targeting: vec![Targeting::default(); n],
            targets: vec![None; n],
            random_until: vec![0; n],
            tick: 0,
            game: 0,
            queued_inputs: (0..n).map(|_| Vec::new()).collect(),
            garbage_in_flight: Vec::new(),
            late_inputs: vec![0; n],
            paused: false,
            finished: false,
            last_seq: vec![0; n],
            last_restart: None,
            start: Instant::now(),
            max_chain: vec![0; n],
            total_chains: vec![0; n],
            nuisance_sent: vec![0; n],
            nuisance_received: vec![0; n],
            out_at: vec![None; n],
            all_clears: vec![0; n],
            pieces_placed: vec![0; n],
            prev_chain: vec![0; n],
            prev_all_clear: vec![false; n],
            prev_piece_id: vec![piece_id; n],
        }
    }

    /// Updates the per-player match statistics after a step.
    pub fn record_stats(&mut self) {
        for i in 0..self.boards.len() {
            let cc = self.boards[i].chain_count;
            if cc > 0 && self.prev_chain[i] == 0 {
                self.total_chains[i] += 1;
            }
            self.max_chain[i] = self.max_chain[i].max(cc);
            self.prev_chain[i] = cc;

            let ac = self.boards[i].last_was_all_clear;
            if ac && !self.prev_all_clear[i] {
                self.all_clears[i] += 1;
            }
            self.prev_all_clear[i] = ac;

            let pid = self.boards[i].piece_id;
            if pid != self.prev_piece_id[i] {
                self.pieces_placed[i] += 1;
                self.prev_piece_id[i] = pid;
            }
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        for board in &mut self.boards {
            board.set_paused(paused);
        }
    }

    pub fn alive(&self, slot: usize) -> bool {
        self.boards[slot].state != GameState::GameOver
    }

    /// The teams with a board still playing.
    pub fn teams_alive(&self) -> Vec<u8> {
        let mut teams: Vec<u8> = (0..self.boards.len())
            .filter(|&s| self.alive(s))
            .map(|s| self.teams[s])
            .collect();
        teams.sort_unstable();
        teams.dedup();
        teams
    }

    /// Takes a slot out of the game, as when its player leaves.
    pub fn eliminate(&mut self, slot: usize) {
        if let Some(board) = self.boards.get_mut(slot) {
            board.state = GameState::GameOver;
            self.out_at[slot].get_or_insert(self.tick);
        }
    }

    pub fn set_targeting(&mut self, slot: usize, targeting: Targeting) {
        if let Some(t) = self.targeting.get_mut(slot) {
            *t = targeting;
            self.random_until[slot] = 0;
        }
        self.update_targets();
    }

    fn opponents(&self, slot: usize) -> Vec<usize> {
        (0..self.boards.len())
            .filter(|&o| self.alive(o) && self.teams[o] != self.teams[slot])
            .collect()
    }

    fn pick_target(&mut self, slot: usize) -> Option<usize> {
        let opponents = self.opponents(slot);
        if opponents.is_empty() {
            return None;
        }
        let random = |sim: &mut Self| {
            let kept = sim.targets[slot].filter(|t| opponents.contains(t) && sim.tick < sim.random_until[slot]);
            kept.unwrap_or_else(|| {
                sim.random_until[slot] = sim.tick + RANDOM_TARGET_TICKS;
                opponents[rand::rng().random_range(0..opponents.len())]
            })
        };
        Some(match self.targeting[slot] {
            Targeting::Manual(t) if opponents.contains(&usize::from(t)) => usize::from(t),
            Targeting::Danger => *opponents
                .iter()
                .max_by_key(|&&o| self.boards[o].danger())
                .unwrap_or(&opponents[0]),
            Targeting::Attackers => match opponents.iter().find(|&&o| self.targets[o] == Some(slot)) {
                Some(&attacker) => attacker,
                None => random(self),
            },
            Targeting::Random | Targeting::Manual(_) => random(self),
        })
    }

    fn update_targets(&mut self) {
        for slot in 0..self.boards.len() {
            self.targets[slot] = if self.alive(slot) { self.pick_target(slot) } else { None };
        }
    }

    pub fn queue_input(&mut self, slot: usize, tick: u32, seq: u32, kind: InputKind) {
        if self.queued_inputs[slot].len() >= INPUT_QUEUE_CAP {
            warn!("File d'inputs pleine (slot {slot}), input {seq} abandonné");
            return;
        }
        if tick < self.tick {
            self.late_inputs[slot] += 1;
        }
        let at = tick.clamp(self.tick, self.tick.saturating_add(config::MAX_INPUT_LEAD_TICKS));
        self.queued_inputs[slot].push(PendingInput { at, seq, kind });
    }

    /// Sends `amount` nuisance from `from` to its current target.
    pub fn send_garbage(&mut self, from: usize, amount: u32, at: u32) {
        if amount == 0 {
            return;
        }
        let Some(to) = self
            .targets
            .get(from)
            .copied()
            .flatten()
            .or_else(|| self.pick_target(from))
        else {
            return;
        };
        self.garbage_in_flight.push(GarbageDelivery { at, from, to, amount });
    }

    fn take_due_garbage(&mut self, slot: usize) -> u32 {
        let now = self.tick;
        let mut landed = 0;
        let mut i = 0;
        while i < self.garbage_in_flight.len() {
            let g = &self.garbage_in_flight[i];
            if g.to == slot && g.at <= now {
                landed += g.amount;
                self.nuisance_sent[g.from] += g.amount;
                self.nuisance_received[slot] += g.amount;
                self.garbage_in_flight.swap_remove(i);
            } else {
                i += 1;
            }
        }
        landed
    }

    fn incoming(&self, slot: usize) -> Vec<IncomingGarbage> {
        self.garbage_in_flight
            .iter()
            .filter(|g| g.to == slot)
            .map(|g| IncomingGarbage {
                at: g.at,
                amount: g.amount,
            })
            .collect()
    }

    fn take_due_inputs(&mut self, slot: usize) -> Vec<InputKind> {
        let now = self.tick;
        let queue = &mut self.queued_inputs[slot];
        let due = queue.iter().position(|p| p.at > now).unwrap_or(queue.len());
        if let Some(last) = queue[..due].last() {
            self.last_seq[slot] = last.seq;
        }
        queue.drain(..due).map(|p| p.kind).collect()
    }

    pub fn advance(&mut self) {
        self.tick += 1;
        self.update_targets();
        let at = self.tick + config::GARBAGE_TRAVEL_TICKS;
        let produced: Vec<u32> = (0..self.boards.len())
            .map(|slot| {
                let inputs = self.take_due_inputs(slot);
                let landed = self.take_due_garbage(slot);
                self.boards[slot].step(inputs, landed)
            })
            .collect();
        for (slot, sent) in produced.into_iter().enumerate() {
            self.send_garbage(slot, sent, at);
            if !self.alive(slot) {
                self.out_at[slot].get_or_insert(self.tick);
            }
        }
    }

    /// How long a slot's team lasted: as long as its last player.
    fn team_lasted(&self, slot: usize) -> u32 {
        (0..self.boards.len())
            .filter(|&s| self.teams[s] == self.teams[slot])
            .map(|s| self.out_at[s].unwrap_or(u32::MAX))
            .max()
            .unwrap_or(0)
    }

    /// Each slot's final place, from 1, shared with its team: the team still
    /// in comes first, then the later a team went out, the better.
    pub fn places(&self) -> Vec<u8> {
        let lasted: Vec<u32> = (0..self.boards.len()).map(|s| self.team_lasted(s)).collect();
        lasted
            .iter()
            .map(|&mine| {
                let mut better: Vec<u32> = lasted.iter().copied().filter(|&t| t > mine).collect();
                better.sort_unstable();
                better.dedup();
                slot_byte(better.len() + 1)
            })
            .collect()
    }

    /// Whether a slot's team won: it is the one team left.
    pub fn won(&self, slot: usize) -> bool {
        self.teams_alive() == [self.teams[slot]]
    }

    pub fn state_update(&self) -> ServerMessage {
        self.state_update_of(0..self.boards.len())
    }

    /// The state of some boards only.
    pub fn state_update_of(&self, slots: impl IntoIterator<Item = usize>) -> ServerMessage {
        let players = slots
            .into_iter()
            .filter(|&i| i < self.boards.len())
            .map(|i| PlayerUpdate {
                slot: slot_byte(i + 1),
                team: self.teams[i],
                board: Box::new(self.boards[i].clone()),
                ack: self.last_seq[i],
                incoming: self.incoming(i),
                target: self.targets[i].map(|t| slot_byte(t + 1)),
            })
            .collect();
        ServerMessage::StateUpdate {
            game: self.game,
            tick: self.tick,
            players,
        }
    }

    pub fn reset_boards(&mut self, s: &RoomSettings, teams: Vec<u8>) {
        let last_restart = self.last_restart;
        let targeting = std::mem::take(&mut self.targeting);
        *self = Self::new(s, teams);
        for (slot, t) in targeting.into_iter().enumerate().take(self.boards.len()) {
            self.targeting[slot] = t;
        }
        self.last_restart = last_restart;
    }
}

/// A slot, team or place as sent on the wire.
pub fn slot_byte(slot: usize) -> u8 {
    u8::try_from(slot).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(teams: &[u8]) -> Sim {
        Sim::new(&RoomSettings::default(), teams.to_vec())
    }

    fn fill(sim: &mut Sim, slot: usize, rows: usize) {
        let h = sim.boards[slot].height;
        for row in &mut sim.boards[slot].cells[h - rows..] {
            row[0] = Some(shared::PuyoType::Red);
        }
    }

    #[test]
    fn nuisance_never_goes_to_a_teammate_or_a_loser() {
        let mut s = sim(&[0, 0, 1, 2]);
        s.eliminate(3);
        for _ in 0..50 {
            s.update_targets();
            assert_eq!(s.targets[0], Some(2), "the only opponent left");
            assert_eq!(s.targets[1], Some(2));
            assert!(matches!(s.targets[2], Some(0 | 1)));
            assert_eq!(s.targets[3], None, "out of the game");
            s.tick += RANDOM_TARGET_TICKS;
        }
    }

    #[test]
    fn danger_aims_at_the_fullest_board() {
        let mut s = sim(&[0, 1, 2]);
        fill(&mut s, 2, 6);
        fill(&mut s, 1, 3);
        s.set_targeting(0, Targeting::Danger);
        assert_eq!(s.targets[0], Some(2));
    }

    #[test]
    fn attackers_answer_whoever_targets_you() {
        let mut s = sim(&[0, 1, 2]);
        s.set_targeting(1, Targeting::Manual(2));
        s.set_targeting(2, Targeting::Manual(0));
        s.set_targeting(0, Targeting::Attackers);
        assert_eq!(s.targets[2], Some(0), "setup");
        s.update_targets();
        assert_eq!(s.targets[0], Some(2));
    }

    #[test]
    fn a_manual_target_falls_back_when_it_cannot_be_hit() {
        let mut s = sim(&[0, 0, 1]);
        s.set_targeting(0, Targeting::Manual(1));
        assert_eq!(s.targets[0], Some(2), "a teammate is never hit");
        s.set_targeting(0, Targeting::Manual(9));
        assert_eq!(s.targets[0], Some(2));
    }

    #[test]
    fn a_random_target_holds_for_a_while() {
        let mut s = sim(&[0, 1, 2, 3, 4]);
        s.update_targets();
        let first = s.targets[0];
        for _ in 0..RANDOM_TARGET_TICKS - 1 {
            s.tick += 1;
            s.update_targets();
            assert_eq!(s.targets[0], first);
        }
    }

    #[test]
    fn landed_nuisance_counts_for_its_sender() {
        let mut s = sim(&[0, 1, 2]);
        s.set_targeting(0, Targeting::Manual(2));
        s.send_garbage(0, 5, s.tick + 1);
        for _ in 0..3 {
            s.advance();
        }
        assert_eq!(s.nuisance_sent, [5, 0, 0]);
        assert_eq!(s.boards[2].pending_garbage, 5);
        assert_eq!(s.boards[1].pending_garbage, 0);
    }

    #[test]
    fn the_game_goes_on_until_one_team_is_left() {
        let mut s = sim(&[0, 1, 1]);
        assert_eq!(s.teams_alive(), [0, 1]);
        s.eliminate(1);
        assert_eq!(s.teams_alive(), [0, 1], "a teammate still plays");
        s.eliminate(2);
        assert_eq!(s.teams_alive(), [0]);
    }

    #[test]
    fn a_team_shares_its_last_players_place_and_win() {
        let mut s = sim(&[0, 0, 1, 2]);
        s.eliminate(0);
        s.tick += 1;
        s.eliminate(3);
        s.tick += 1;
        s.eliminate(2);
        assert_eq!(s.places(), [1, 1, 2, 3]);
        assert_eq!((0..4).map(|i| s.won(i)).collect::<Vec<_>>(), [true, true, false, false]);
    }

    #[test]
    fn the_wire_names_slots_from_one() {
        let mut s = sim(&[0, 1]);
        s.update_targets();
        let ServerMessage::StateUpdate { players, .. } = s.state_update() else {
            unreachable!()
        };
        let seen: Vec<_> = players.iter().map(|p| (p.slot, p.target)).collect();
        assert_eq!(seen, [(1, Some(2)), (2, Some(1))]);
    }
}
