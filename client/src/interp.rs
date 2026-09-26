use std::collections::VecDeque;

use shared::{config, Board};

const MIN_DELAY_TICKS: f64 = 2.0;
const MAX_DELAY_TICKS: f64 = 12.0;
const JITTER_WINDOW: usize = 120;
const MAX_SNAPSHOTS: usize = 120;
const MAX_RATE_CHANGE: f64 = 0.25;
const STEER_GAIN: f64 = 0.05;
const SNAP_TICKS: f64 = 30.0;

const TICK_HZ: f64 = config::SERVER_TICK_HZ as f64;

struct Snapshot {
    tick: u32,
    board: Board,
}

#[derive(Default)]
pub struct OpponentView {
    snapshots: VecDeque<Snapshot>,
    arrivals: VecDeque<f64>,
    render_tick: Option<f64>,
}

impl OpponentView {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn push(&mut self, tick: u32, board: Board, now_secs: f64) {
        match self.snapshots.back() {
            Some(last) if tick < last.tick => self.clear(),
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

        while self.snapshots.len() > 2 && self.snapshots[1].tick as f64 <= next {
            self.snapshots.pop_front();
        }
    }

    pub fn frame(&self) -> Option<(&Board, (f32, f32))> {
        let rt = self.render_tick?;
        let i = self.snapshots.iter().rposition(|s| s.tick as f64 <= rt).unwrap_or(0);
        let a = self.snapshots.get(i)?;
        let Some(pa) = a.board.active_piece.as_ref() else {
            return Some((&a.board, (0.0, 0.0)));
        };
        let from = (pa.row as f32 + crate::draw::fall_step(&a.board), pa.col as f32);
        let (row, col) = self
            .snapshots
            .get(i + 1)
            .filter(|b| b.board.piece_id == a.board.piece_id && b.tick > a.tick)
            .and_then(|b| {
                let pb = b.board.active_piece.as_ref()?;
                let to = (pb.row as f32 + crate::draw::fall_step(&b.board), pb.col as f32);
                let frac = ((rt - a.tick as f64) / (b.tick - a.tick) as f64).clamp(0.0, 1.0) as f32;
                Some((from.0 + frac * (to.0 - from.0), from.1 + frac * (to.1 - from.1)))
            })
            .unwrap_or(from);
        Some((&a.board, (row - pa.row as f32, col - pa.col as f32)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

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

    fn row_of(view: &OpponentView) -> f32 {
        let (board, off) = view.frame().expect("a frame");
        board.active_piece.as_ref().expect("a piece").row as f32 + off.0
    }

    #[test]
    fn a_drop_between_two_updates_is_not_counted_twice() {
        let mut before = board_at(2, 7);
        before.active_piece.as_mut().expect("a piece").row = 3;
        before.fall_timer = config::BASE_FALL_INTERVAL as f32 * 0.99;
        let mut after = board_at(2, 7);
        after.active_piece.as_mut().expect("a piece").row = 4;
        after.fall_timer = 0.0;

        let mut view = OpponentView::default();
        view.push(10, before, 0.0);
        view.push(11, after, 0.0);
        let at = |view: &mut OpponentView, rt: f64| {
            view.render_tick = Some(rt);
            row_of(view)
        };
        assert_eq!(at(&mut view, 10.0), 3.5);
        let mid = at(&mut view, 10.5);
        assert!((3.5..=4.0).contains(&mid), "drawn at row {mid}");
        assert_eq!(at(&mut view, 11.0), 4.0);
    }

    #[test]
    fn a_falling_piece_is_drawn_in_half_cell_notches() {
        let mut b = board_at(2, 1);
        let interval = config::BASE_FALL_INTERVAL as f32;
        for (timer, drawn) in [(0.0, 0.0), (0.49, 0.0), (0.5, 0.5), (0.99, 0.5)] {
            b.fall_timer = interval * timer;
            assert_eq!(crate::draw::fall_step(&b), drawn, "at {timer} of the interval");
        }
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
        let rts = play(&mut view, 600, |t| 2.0 + (8 - (t % 37) as i64).max(0) as f64);
        let delay = view.delay_ticks();
        assert!(delay >= 8.0, "a delay of {delay} ticks cannot bridge the gaps");

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
