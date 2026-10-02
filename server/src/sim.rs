use rand::RngExt;
use shared::{config, Board, IncomingGarbage, InputKind, RngPosition, RoomSettings, ServerMessage};
use tokio::time::Instant;
use tracing::warn;

use crate::ws::CLIENT_MSG_BURST;

pub const INPUT_QUEUE_CAP: usize = 2048;

const _: () = assert!(INPUT_QUEUE_CAP as f64 > CLIENT_MSG_BURST);

pub struct GarbageDelivery {
    pub at: u32,
    slot: usize,
    amount: u32,
}

pub struct PendingInput {
    at: u32,
    seq: u32,
    kind: InputKind,
}

pub struct Sim {
    pub boards: [Board; 2],
    pub tick: u32,
    pub queued_inputs: [Vec<PendingInput>; 2],
    pub garbage_in_flight: Vec<GarbageDelivery>,
    pub late_inputs: [u32; 2],
    pub paused: bool,
    pub finished: bool,
    pub last_seq: [u32; 2],
    pub last_restart: Option<Instant>,
    pub start: Instant,
    pub max_chain: [u32; 2],
    pub total_chains: [u32; 2],
    pub nuisance_sent: [u32; 2],
    pub all_clears: [u32; 2],
    pub pieces_placed: [u32; 2],
    prev_chain: [u32; 2],
    prev_all_clear: [bool; 2],
    prev_piece_id: [u32; 2],
    pub last_sent_rng: Option<[RngPosition; 2]>,
}

impl Sim {
    /// Updates the per-player match statistics after a step.
    pub fn record_stats(&mut self) {
        for i in 0..2 {
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

    pub fn new(settings: &RoomSettings) -> Self {
        let board = Board::for_match(rand::rng().random(), settings);
        let piece_id = board.piece_id;
        Self {
            boards: [board.clone(), board],
            tick: 0,
            queued_inputs: [Vec::new(), Vec::new()],
            garbage_in_flight: Vec::new(),
            late_inputs: [0; 2],
            paused: false,
            finished: false,
            last_seq: [0; 2],
            last_restart: None,
            start: Instant::now(),
            max_chain: [0; 2],
            total_chains: [0; 2],
            nuisance_sent: [0; 2],
            all_clears: [0; 2],
            pieces_placed: [0; 2],
            prev_chain: [0; 2],
            prev_all_clear: [false; 2],
            prev_piece_id: [piece_id; 2],
            last_sent_rng: None,
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        for board in &mut self.boards {
            board.set_paused(paused);
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

    pub fn send_garbage(&mut self, from: usize, amount: u32, at: u32) {
        debug_assert!(from < 2, "slot {from} does not exist");
        if amount == 0 {
            return;
        }
        self.garbage_in_flight.push(GarbageDelivery {
            at,
            slot: 1 - from,
            amount,
        });
    }

    fn take_due_garbage(&mut self, slot: usize) -> u32 {
        let now = self.tick;
        let mut landed = 0;
        let mut i = 0;
        while i < self.garbage_in_flight.len() {
            let g = &self.garbage_in_flight[i];
            if g.slot == slot && g.at <= now {
                landed += g.amount;
                self.nuisance_sent[1 - slot] += g.amount;
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
            .filter(|g| g.slot == slot)
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
        let at = self.tick + config::GARBAGE_TRAVEL_TICKS;
        let mut produced = [0; 2];
        for (slot, sent) in produced.iter_mut().enumerate() {
            let inputs = self.take_due_inputs(slot);
            let landed = self.take_due_garbage(slot);
            *sent = self.boards[slot].step(inputs, landed);
        }
        for (slot, sent) in produced.into_iter().enumerate() {
            self.send_garbage(slot, sent, at);
        }
    }

    pub fn rng_positions(&self) -> [RngPosition; 2] {
        [self.boards[0].rng_position(), self.boards[1].rng_position()]
    }

    pub fn state_update(&self, full_rng: bool) -> ServerMessage {
        let pos = self.rng_positions();
        let rng = |i: usize| {
            let changed = full_rng || self.last_sent_rng.is_none_or(|sent| sent[i] != pos[i]);
            changed.then(|| Box::new(self.boards[i].rng_state()))
        };
        ServerMessage::StateUpdate {
            p1_board: Box::new(self.boards[0].clone()),
            p2_board: Box::new(self.boards[1].clone()),
            p1_rng: rng(0),
            p2_rng: rng(1),
            p1_ack: self.last_seq[0],
            p2_ack: self.last_seq[1],
            tick: self.tick,
            p1_incoming: self.incoming(0),
            p2_incoming: self.incoming(1),
        }
    }

    pub fn reset_boards(&mut self, s: &RoomSettings) {
        let last_restart = self.last_restart;
        *self = Self::new(s);
        self.last_restart = last_restart;
    }
}
