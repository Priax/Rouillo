use notan::prelude::*;
use shared::*;

use crate::connection::Connection;
use crate::state::{GameSession, Settings};

pub fn update_game(
    app: &mut App,
    session: &mut GameSession,
    settings: &Settings,
    conn: &mut Connection,
    is_host: bool,
) {
    if !conn.is_live() {
        return;
    }

    let game_over = session.board.state == GameState::GameOver || session.other_board.state == GameState::GameOver;
    let paused = session.board.state == GameState::Paused;

    if paused || game_over || session.opponent_disconnected {
        let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
        if crate::rooms::leave_room_button(ww, wh).clicked(app) {
            conn.send(&ClientMessage::LeaveRoom);
            return;
        }
        if is_host && crate::rooms::back_to_lobby_button(ww, wh).clicked(app) {
            conn.send(&ClientMessage::ReturnToLobby);
            return;
        }
    }

    if session.opponent_disconnected {
        return;
    }

    handle_global_input(app, session, conn);

    let dt = app.timer.delta_f32();
    if !game_over && !paused {
        if session.predicted_board.state == GameState::Playing {
            handle_game_input(app, session, settings, conn, dt);
        } else {
            release_keys(session);
        }
        step_simulation(session, dt);
    } else {
        session.sim_accumulator = 0.0;
        release_keys(session);
    }

    session.opponent_view.advance(dt);

    let off = &mut session.piece_visual_offset;
    let decay = (-PIECE_SMOOTH_RATE * dt).exp();
    off.0 *= decay;
    off.1 *= decay;
    if off.0.abs() < 0.001 {
        off.0 = 0.0;
    }
    if off.1.abs() < 0.001 {
        off.1 = 0.0;
    }

    if let Some((_, ref mut t)) = session.chain_display {
        *t -= dt;
        if *t <= 0.0 {
            session.chain_display = None;
        }
    }
    if session.all_clear_timer > 0.0 {
        session.all_clear_timer -= dt;
    }
}

const PIECE_SMOOTH_RATE: f32 = 22.0;

fn release_keys(session: &mut GameSession) {
    session.key_timer_left = 0.0;
    session.key_timer_right = 0.0;
    session.key_timer_down = 0.0;
}

fn step_simulation(session: &mut GameSession, dt: f32) {
    if !session.synced {
        return;
    }
    session.sim_accumulator += dt;
    let mut steps = 0;
    while session.sim_accumulator >= config::CLIENT_SIM_DT && steps < config::MAX_SIM_STEPS_PER_FRAME {
        session.sim_accumulator -= config::CLIENT_SIM_DT;
        steps += 1;
        // Clock steering spends one step at a time, never a burst: a skipped
        // or doubled 1/60 s is invisible where a jump of several is not.
        match session.clock_correction.signum() {
            -1 => {
                session.clock_correction += 1;
                continue;
            }
            1 => {
                session.clock_correction -= 1;
                advance_one(session);
            }
            _ => {}
        }
        advance_one(session);
    }
    if steps == config::MAX_SIM_STEPS_PER_FRAME {
        session.sim_accumulator = 0.0;
    }
}

/// One tick of the local board, exactly as the server will run it. Inputs are
/// not passed: they were applied the moment they were pressed, stamped for
/// this very tick, which is the same thing `Board::step` would do first.
pub(crate) fn advance_one(session: &mut GameSession) {
    session.local_tick = session.local_tick.wrapping_add(1);
    let now = session.local_tick;
    let landed = session.incoming.iter().filter(|g| g.at == now).map(|g| g.amount).sum();
    let was_playing = session.predicted_board.state == GameState::Playing;
    session.predicted_board.step([], landed, config::CLIENT_SIM_DT);
    if was_playing && session.predicted_board.state == GameState::ResolvingMatches {
        crate::audio::play_lock();
    }
    announce_events(session);
}

/// Plays what the local board just did. Keyed on (piece, chain step) rather
/// than on a transition, because a replay may reach the same moment again.
pub fn announce_events(session: &mut GameSession) {
    let board = &session.predicted_board;
    let chain = (board.piece_id, board.chain_count);
    if board.chain_count > 0 && chain > session.announced_chain {
        session.announced_chain = chain;
        crate::audio::play_pop(board.chain_count);
        session.chain_display = Some((board.chain_count, 2.0));
    }
    let board = &session.predicted_board;
    if board.last_was_all_clear && board.piece_id != session.announced_all_clear {
        session.announced_all_clear = board.piece_id;
        crate::audio::play_all_clear();
        session.all_clear_timer = 3.0;
    }
}

fn send_input(session: &mut GameSession, conn: &mut Connection, kind: InputKind) {
    match kind {
        InputKind::MoveLeft | InputKind::MoveRight => crate::audio::play_move(),
        InputKind::RotateCW | InputKind::RotateCCW => crate::audio::play_rotate(),
        InputKind::HardDrop => crate::audio::play_lock(),
        InputKind::SoftDrop => {}
    }
    session.input_seq += 1;
    let seq = session.input_seq;
    let tick = session.local_tick.wrapping_add(1);
    session.predicted_board.apply_input(kind);
    session.pending_inputs.push((seq, StampedInput { tick, kind }));
    session.sent_at.push((seq, session.clock));
    conn.send(&ClientMessage::Input { kind, seq, tick });
}

fn handle_global_input(app: &mut App, session: &mut GameSession, conn: &mut Connection) {
    let can_restart = session.board.state == GameState::GameOver || session.other_board.state == GameState::GameOver;
    if app.keyboard.was_pressed(KeyCode::KeyR) && can_restart {
        conn.send(&ClientMessage::RequestRestart);
    }

    if app.keyboard.was_pressed(KeyCode::Escape) {
        conn.send(&ClientMessage::TogglePause);
    }

    let _ = session;
}

fn handle_game_input(
    app: &mut App,
    session: &mut GameSession,
    settings: &Settings,
    conn: &mut Connection,
    delta_time: f32,
) {
    if app.keyboard.was_pressed(KeyCode::ArrowUp) || app.keyboard.was_pressed(KeyCode::KeyZ) {
        send_input(session, conn, InputKind::RotateCW);
    }
    if app.keyboard.was_pressed(KeyCode::KeyX) || app.keyboard.was_pressed(KeyCode::KeyW) {
        send_input(session, conn, InputKind::RotateCCW);
    }

    if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Enter) {
        send_input(session, conn, InputKind::HardDrop);
        return;
    }

    if app.keyboard.is_down(KeyCode::ArrowLeft) {
        if session.key_timer_left == 0.0 {
            send_input(session, conn, InputKind::MoveLeft);
            session.key_timer_left = 0.0001;
        } else {
            session.key_timer_left += delta_time;
            while session.key_timer_left > settings.das_delay + settings.das_speed {
                send_input(session, conn, InputKind::MoveLeft);
                session.key_timer_left -= settings.das_speed;
            }
        }
    } else {
        session.key_timer_left = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowRight) {
        if session.key_timer_right == 0.0 {
            send_input(session, conn, InputKind::MoveRight);
            session.key_timer_right = 0.0001;
        } else {
            session.key_timer_right += delta_time;
            while session.key_timer_right > settings.das_delay + settings.das_speed {
                send_input(session, conn, InputKind::MoveRight);
                session.key_timer_right -= settings.das_speed;
            }
        }
    } else {
        session.key_timer_right = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowDown) {
        session.key_timer_down += delta_time;
        if session.key_timer_down > settings.soft_drop_speed {
            send_input(session, conn, InputKind::SoftDrop);
            session.key_timer_down = 0.0;
        }
    } else {
        session.key_timer_down = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synced_session() -> GameSession {
        let mut session = GameSession::new(1);
        session.predicted_board.spawn_piece();
        session.synced = true;
        session
    }

    fn rows_fallen(frame_dts: &[f32], secs: f32) -> i32 {
        let mut session = synced_session();
        let start = session.predicted_board.active_piece.as_ref().expect("a piece").row;

        let mut elapsed = 0.0;
        for dt in frame_dts.iter().cycle() {
            if elapsed >= secs {
                break;
            }
            step_simulation(&mut session, *dt);
            elapsed += dt;
        }
        session.predicted_board.active_piece.as_ref().expect("a piece").row - start
    }

    #[test]
    fn prediction_is_independent_of_the_frame_rate() {
        let secs = 3.0;
        let at_60 = rows_fallen(&[1.0 / 60.0], secs);
        assert!(at_60 > 0, "the piece never fell, the test proves nothing");

        for (name, dts) in [
            ("144 Hz", vec![1.0 / 144.0]),
            ("30 Hz", vec![1.0 / 30.0]),
            ("jittery", vec![1.0 / 61.0, 1.0 / 58.0, 1.0 / 144.0, 1.0 / 45.0]),
        ] {
            let rows = rows_fallen(&dts, secs);
            assert!(
                (rows - at_60).abs() <= 1,
                "{name} fell {rows} rows where 60 Hz fell {at_60}"
            );
        }
    }

    #[test]
    fn a_long_stall_does_not_replay_as_a_burst() {
        let mut session = synced_session();
        let start = session.predicted_board.active_piece.as_ref().expect("a piece").row;

        step_simulation(&mut session, 30.0);

        let fell = session.predicted_board.active_piece.as_ref().expect("a piece").row - start;
        assert!(
            fell <= config::MAX_SIM_STEPS_PER_FRAME as i32,
            "{fell} rows in one frame, at most {} allowed",
            config::MAX_SIM_STEPS_PER_FRAME
        );
        assert_eq!(session.sim_accumulator, 0.0, "the backlog was kept to be replayed");
    }

    fn run_steps(session: &mut GameSession, n: u32) {
        for _ in 0..n {
            step_simulation(session, config::CLIENT_SIM_DT);
        }
    }

    #[test]
    fn the_board_waits_for_the_first_update() {
        let mut session = GameSession::new(1);
        run_steps(&mut session, 10);
        assert_eq!(session.local_tick, 0);
    }

    #[test]
    fn each_step_advances_the_local_clock_by_one_tick() {
        let mut session = synced_session();
        run_steps(&mut session, 10);
        assert_eq!(session.local_tick, 10);
    }

    #[test]
    fn a_correction_is_spent_one_tick_per_step() {
        let mut session = synced_session();
        session.clock_correction = -3;
        run_steps(&mut session, 10);
        assert_eq!(session.local_tick, 7, "held back");
        assert_eq!(session.clock_correction, 0);

        let mut session = synced_session();
        session.clock_correction = 3;
        run_steps(&mut session, 10);
        assert_eq!(session.local_tick, 13, "caught up");
        assert_eq!(session.clock_correction, 0);
    }

    #[test]
    fn a_chain_step_is_announced_once_however_often_it_is_replayed() {
        let mut session = synced_session();
        session.predicted_board.chain_count = 2;
        announce_events(&mut session);
        assert_eq!(session.chain_display.map(|(c, _)| c), Some(2));

        session.chain_display = None;
        announce_events(&mut session);
        assert_eq!(session.chain_display, None, "announced twice");

        session.predicted_board.chain_count = 3;
        announce_events(&mut session);
        assert_eq!(
            session.chain_display.map(|(c, _)| c),
            Some(3),
            "the next step was missed"
        );
    }

    #[test]
    fn landing_garbage_moves_from_travelling_to_queued() {
        let mut session = synced_session();
        session.incoming = vec![IncomingGarbage { at: 3, amount: 5 }];
        assert_eq!(session.my_nuisance(), 5);
        run_steps(&mut session, 2);
        assert_eq!(session.predicted_board.pending_garbage, 0);
        run_steps(&mut session, 1);
        assert_eq!(session.predicted_board.pending_garbage, 5);
        assert_eq!(session.my_nuisance(), 5, "counted twice or lost on landing");
    }
}
