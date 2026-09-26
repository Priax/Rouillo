//! The opponent's board, played back slightly in the past.
//!
//! Updates are sent every tick but do not arrive every frame: the network
//! bunches them up and spreads them out. Drawing the newest one as it lands
//! makes the opponent's piece stutter along. Instead, updates are kept with
//! the server tick they describe, and the board is drawn at a render tick that
//! trails the newest by a small delay, sized from how unevenly updates arrive.
//! The falling piece is placed between the two updates around that tick.

use std::collections::VecDeque;

use shared::{config, Board};

/// Never less than this behind the newest update, even on a perfect link:
/// one tick of slack for the frame and the update to be out of step.
const MIN_DELAY_TICKS: f64 = 2.0;
/// Never more: past this, the opponent looks laggy rather than stuttery.
const MAX_DELAY_TICKS: f64 = 12.0;
/// Arrivals remembered to measure jitter; 2 s of updates.
const JITTER_WINDOW: usize = 120;
/// Updates kept at most; the render tick only needs the two around it.
const MAX_SNAPSHOTS: usize = 120;
/// How much faster or slower than real time playback may run while it
/// catches up with its target, and how hard it leans towards it.
const MAX_RATE_CHANGE: f64 = 0.25;
const STEER_GAIN: f64 = 0.05;
/// Off by more than this, playback jumps instead of catching up.
const SNAP_TICKS: f64 = 30.0;

const TICK_HZ: f64 = config::SERVER_TICK_HZ as f64;

struct Snapshot {
    tick: u32,
    board: Board,
}

#[derive(Default)]
pub struct OpponentView {
    snapshots: VecDeque<Snapshot>,
    /// For each recent update, its arrival time in ticks minus its tick. On
    /// a steady link this is constant; its spread is the jitter.
    arrivals: VecDeque<f64>,
    render_tick: Option<f64>,
}

impl OpponentView {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Takes in the opponent's board as of `tick`, received at `now_secs`.
    pub fn push(&mut self, tick: u32, board: Board, now_secs: f64) {
        match self.snapshots.back() {
            // A new game started without us seeing the restart: start over.
            Some(last) if tick < last.tick => self.clear(),
            // Same tick sent again (a snapshot after a pause, a rejoin): the
            // newer word wins, the timing sample says nothing new.
            Some(last) if tick == last.tick => {
                if let Some(last) = self.snapshots.back_mut() {
                    last.board = board;
                }
                return;
            }
            _ => {}
        }
        self.arrivals.push_back(now_secs * TICK_HZ - tick as f64);
        if self.arrivals.len() > JITTER_WINDOW {
            self.arrivals.pop_front();
        }
        self.snapshots.push_back(Snapshot { tick, board });
        if self.snapshots.len() > MAX_SNAPSHOTS {
            self.snapshots.pop_front();
        }
        if self.render_tick.is_none() {
            self.render_tick = Some(tick as f64 - self.delay_ticks());
        }
    }

    /// How far behind the newest update to play: the spread of recent
    /// arrivals, so that the update after the render tick has almost always
    /// arrived already, plus one tick of slack.
    pub fn delay_ticks(&self) -> f64 {
        let (min, max) = self
            .arrivals
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &a| {
                (lo.min(a), hi.max(a))
            });
        let jitter = if min.is_finite() { max - min } else { 0.0 };
        (jitter.ceil() + 1.0).clamp(MIN_DELAY_TICKS, MAX_DELAY_TICKS)
    }

    /// Moves playback on by `dt` seconds, a little faster or slower than real
    /// time so that it settles `delay_ticks` behind the newest update. It
    /// never plays past the newest one: when updates stop (a pause), the
    /// board holds on the last one known.
    pub fn advance(&mut self, dt: f32) {
        let (Some(rt), Some(newest)) = (self.render_tick, self.snapshots.back()) else {
            return;
        };
        let newest = newest.tick as f64;
        let target = newest - self.delay_ticks();
        let error = target - rt;
        let next = if error.abs() > SNAP_TICKS {
            target
        } else {
            let rate = 1.0 + (error * STEER_GAIN).clamp(-MAX_RATE_CHANGE, MAX_RATE_CHANGE);
            rt + dt as f64 * TICK_HZ * rate
        };
        let next = next.min(newest);
        self.render_tick = Some(next);

        // Keep one snapshot at or before the render tick, drop the rest.
        while self.snapshots.len() > 2 && self.snapshots[1].tick as f64 <= next {
            self.snapshots.pop_front();
        }
    }

    /// The board to draw and the offset, in cells, to draw its falling piece
    /// at: the last update at or before the render tick, with the piece moved
    /// part of the way to where the next update has it. Only a piece that is
    /// the same in both is moved; a new piece or a lock shows as it is.
    pub fn frame(&self) -> Option<(&Board, (f32, f32))> {
        let rt = self.render_tick?;
        let i = self.snapshots.iter().rposition(|s| s.tick as f64 <= rt).unwrap_or(0);
        let a = self.snapshots.get(i)?;
        let offset = self
            .snapshots
            .get(i + 1)
            .filter(|b| b.board.piece_id == a.board.piece_id && b.tick > a.tick)
            .and_then(|b| {
                let (pa, pb) = (a.board.active_piece.as_ref()?, b.board.active_piece.as_ref()?);
                let frac = ((rt - a.tick as f64) / (b.tick - a.tick) as f64).clamp(0.0, 1.0) as f32;
                Some((frac * (pb.row - pa.row) as f32, frac * (pb.col - pa.col) as f32))
            })
            .unwrap_or((0.0, 0.0));
        Some((&a.board, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// A board whose piece sits at column `col`, piece number `id`.
    fn board_at(col: i32, id: u32) -> Board {
        let mut b = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 1, 1, 5);
        b.spawn_piece();
        b.piece_id = id;
        b.active_piece.as_mut().expect("a piece").col = col;
        b
    }

    fn col_of(view: &OpponentView) -> f32 {
        let (board, off) = view.frame().expect("a frame");
        board.active_piece.as_ref().expect("a piece").col as f32 + off.1
    }

    /// Feeds one update per tick, each arriving `lateness(tick)` ticks after
    /// it was sent, and advances playback one frame per tick. Returns the
    /// render tick after every frame.
    fn play(view: &mut OpponentView, ticks: u32, lateness: impl Fn(u32) -> f64) -> Vec<f64> {
        let mut arrivals: Vec<(f64, u32)> = (1..=ticks).map(|t| (t as f64 + lateness(t), t)).collect();
        arrivals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut next = 0;
        let mut seen = 0;
        let mut out = Vec::new();
        for frame in 1..=ticks {
            let now = frame as f64;
            while next < arrivals.len() && arrivals[next].0 <= now {
                let t = arrivals[next].1;
                // Out-of-order arrivals are dropped, like an older tick is.
                if t > seen {
                    view.push(t, board_at(0, 1), now / TICK_HZ);
                    seen = t;
                }
                next += 1;
            }
            view.advance(DT);
            if let Some(rt) = view.render_tick {
                out.push(rt);
            }
        }
        out
    }

    #[test]
    fn the_piece_is_drawn_between_the_updates_around_the_render_tick() {
        let mut view = OpponentView::default();
        view.push(10, board_at(1, 7), 0.0);
        view.push(14, board_at(3, 7), 0.0);
        view.render_tick = Some(12.0);
        assert!((col_of(&view) - 2.0).abs() < 1e-5, "drawn at {}", col_of(&view));

        view.render_tick = Some(10.0);
        assert!((col_of(&view) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_new_piece_is_not_slid_from_the_old_one() {
        let mut view = OpponentView::default();
        view.push(10, board_at(5, 7), 0.0);
        view.push(11, board_at(2, 8), 0.0);
        view.render_tick = Some(10.5);
        assert_eq!(col_of(&view), 5.0, "the old piece must stay put until it is gone");
    }

    #[test]
    fn playback_never_runs_past_the_newest_update() {
        let mut view = OpponentView::default();
        view.push(10, board_at(0, 1), 0.0);
        for _ in 0..600 {
            view.advance(DT);
        }
        assert_eq!(view.render_tick, Some(10.0));
    }

    #[test]
    fn a_steady_link_settles_at_the_minimum_delay_and_real_time() {
        let mut view = OpponentView::default();
        let rts = play(&mut view, 600, |_| 3.0);
        assert_eq!(view.delay_ticks(), MIN_DELAY_TICKS);
        let tail = &rts[rts.len() - 60..];
        for w in tail.windows(2) {
            assert!((w[1] - w[0] - 1.0).abs() < 1e-6, "playback is not at real time: {w:?}");
        }
    }

    #[test]
    fn jitter_buys_a_longer_delay_and_keeps_playback_smooth() {
        let mut view = OpponentView::default();
        // Now and then the link stalls for 8 ticks and the held updates
        // arrive together; the rest of the time it is steady. Steering alone
        // cannot hide that: the average barely moves, the gap is real.
        let rts = play(&mut view, 600, |t| 2.0 + (8 - (t % 37) as i64).max(0) as f64);
        let delay = view.delay_ticks();
        assert!(delay >= 8.0, "a delay of {delay} ticks cannot bridge the gaps");

        // Once settled, the render tick moves forward every frame, never
        // stalling on a gap nor skipping ahead.
        let tail = &rts[rts.len() - 120..];
        for w in tail.windows(2) {
            let step = w[1] - w[0];
            assert!(step > 0.7 && step < 1.3, "playback stuttered: {w:?}");
        }
    }

    #[test]
    fn playback_that_fell_far_behind_jumps_to_its_target() {
        let mut view = OpponentView::default();
        view.push(10, board_at(0, 1), 0.0);
        view.push(500, board_at(0, 1), 490.0 / TICK_HZ);
        view.advance(DT);
        let rt = view.render_tick.expect("playing");
        assert!(rt > 450.0, "crawling towards the present from {rt}");
    }

    #[test]
    fn a_tick_going_backwards_starts_over() {
        let mut view = OpponentView::default();
        view.push(900, board_at(0, 1), 15.0);
        view.push(1, board_at(4, 1), 16.0);
        assert_eq!(view.snapshots.len(), 1);
        assert_eq!(view.snapshots[0].tick, 1);
    }

    #[test]
    fn the_same_tick_again_replaces_the_board() {
        let mut view = OpponentView::default();
        view.push(10, board_at(0, 1), 0.0);
        view.push(10, board_at(4, 1), 5.0);
        assert_eq!(view.snapshots.len(), 1);
        assert_eq!(col_of(&view), 4.0);
        assert_eq!(view.arrivals.len(), 1, "a resend is not a timing sample");
    }
}
