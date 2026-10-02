use notan::prelude::*;
use shared::{config, ClientMessage, GameState, InputKind, StampedInput};

use crate::connection::Connection;
use crate::state::{GameSession, Settings};
use crate::ui::Ui;

#[derive(Clone, Copy)]
pub struct Online {
    pub is_host: bool,
    pub ranked: bool,
    pub ended: bool,
}

pub fn update_game(
    app: &mut App,
    ui: &Ui,
    session: &mut GameSession,
    settings: Settings,
    conn: &mut Connection,
    online: Online,
) -> bool {
    if !conn.is_live() {
        return false;
    }

    let game_over = session.decided() || online.ended;
    let paused = session.board.state == GameState::Paused;
    let is_host = online.is_host;

    if paused || game_over || session.opponent_disconnected || session.quit_menu {
        let with_back = is_host && !online.ranked;
        let (leave_row, back_row) = crate::draw::exit_rows(ui.view(), with_back);
        if ui.bar_clicked(leave_row) {
            conn.send(&ClientMessage::LeaveRoom);
            return true;
        }
        if with_back && ui.bar_clicked(back_row) {
            conn.send(&ClientMessage::ReturnToLobby);
            return false;
        }
    }

    if session.opponent_disconnected {
        return false;
    }

    handle_global_input(app, session, conn, online.ranked);

    let dt = app.timer.delta_f32();
    if !game_over && !paused && !session.quit_menu {
        read_controls(app, session, settings, Some(conn), dt);
        step_simulation(session, dt);
    } else {
        hold(session);
    }

    animate(session, dt);
    false
}

pub fn animate(session: &mut GameSession, dt: f32) {
    session.opponent_view.advance(dt);
    if let Some(p) = &session.predicted_board.active_piece {
        session.my_turn.update(session.predicted_board.piece_id, p.rotation, dt);
    }
    let shown = session
        .opponent_view
        .frame()
        .map_or(&session.other_board, |(board, _)| board);
    if let Some(p) = &shown.active_piece {
        let (id, rotation) = (shown.piece_id, p.rotation);
        session.opp_turn.update(id, rotation, dt);
    }

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

pub fn read_controls(
    app: &App,
    session: &mut GameSession,
    settings: Settings,
    mut conn: Option<&mut Connection>,
    dt: f32,
) {
    handle_soft_drop_key(app, session, conn.as_deref_mut());
    if session.predicted_board.state == GameState::Playing {
        handle_game_input(app, session, settings, conn, dt);
    } else {
        release_keys(session);
    }
}

pub fn hold(session: &mut GameSession) {
    session.sim_accumulator = 0.0;
    release_keys(session);
}

fn release_keys(session: &mut GameSession) {
    session.key_timer_left = 0.0;
    session.key_timer_right = 0.0;
}

fn step_simulation(session: &mut GameSession, dt: f32) {
    if !session.synced {
        return;
    }
    for _ in 0..take_steps(session, dt) {
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
}

pub fn take_steps(session: &mut GameSession, dt: f32) -> u32 {
    session.sim_accumulator += dt;
    let mut steps = 0;
    while session.sim_accumulator >= config::CLIENT_SIM_DT && steps < config::MAX_SIM_STEPS_PER_FRAME {
        session.sim_accumulator -= config::CLIENT_SIM_DT;
        steps += 1;
    }
    if steps == config::MAX_SIM_STEPS_PER_FRAME {
        session.sim_accumulator = 0.0;
    }
    steps
}

pub(crate) fn advance_one(session: &mut GameSession) -> u32 {
    session.local_tick = session.local_tick.wrapping_add(1);
    let now = session.local_tick;
    let landed = session.incoming.iter().filter(|g| g.at == now).map(|g| g.amount).sum();
    let was_playing = session.predicted_board.state == GameState::Playing;
    let sent = session.predicted_board.step([], landed);
    if was_playing && session.predicted_board.state == GameState::ResolvingMatches {
        crate::audio::play_lock();
    }
    announce_events(session);
    sent
}

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

fn send_input(session: &mut GameSession, conn: Option<&mut Connection>, kind: InputKind) -> bool {
    let before = session.predicted_board.active_piece.clone();
    session.predicted_board.apply_input(kind);
    let moved = session.predicted_board.active_piece != before;
    match kind {
        InputKind::RotateCW | InputKind::RotateCCW if moved => crate::audio::play_rotate(),
        InputKind::HardDrop => crate::audio::play_lock(),
        _ => {}
    }
    if let Some(conn) = conn {
        session.input_seq += 1;
        let seq = session.input_seq;
        let tick = session.local_tick.wrapping_add(1);
        session.pending_inputs.push((seq, StampedInput { tick, kind }));
        session.sent_at.push((seq, session.clock));
        conn.send(&ClientMessage::Input { kind, seq, tick });
    }
    moved
}

fn handle_global_input(app: &App, session: &mut GameSession, conn: &mut Connection, ranked: bool) {
    if !ranked && app.keyboard.was_pressed(KeyCode::KeyR) && session.decided() {
        conn.send(&ClientMessage::RequestRestart);
    }

    if app.keyboard.was_pressed(KeyCode::Escape) {
        if ranked {
            session.quit_menu = !session.quit_menu && !session.decided();
        } else {
            conn.send(&ClientMessage::TogglePause);
        }
    }
}

fn handle_game_input(
    app: &App,
    session: &mut GameSession,
    settings: Settings,
    mut conn: Option<&mut Connection>,
    delta_time: f32,
) {
    if app.keyboard.was_pressed(KeyCode::ArrowUp) || app.keyboard.was_pressed(KeyCode::KeyZ) {
        send_input(session, conn.as_deref_mut(), InputKind::RotateCW);
    }
    if app.keyboard.was_pressed(KeyCode::KeyX) || app.keyboard.was_pressed(KeyCode::KeyW) {
        send_input(session, conn.as_deref_mut(), InputKind::RotateCCW);
    }

    if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Enter) {
        send_input(session, conn, InputKind::HardDrop);
        return;
    }

    for (key, kind) in [
        (KeyCode::ArrowLeft, InputKind::MoveLeft),
        (KeyCode::ArrowRight, InputKind::MoveRight),
    ] {
        let timer = if kind == InputKind::MoveLeft {
            &mut session.key_timer_left
        } else {
            &mut session.key_timer_right
        };
        let first = *timer == 0.0;
        let moves = autorepeat(timer, app.keyboard.is_down(key), delta_time, settings);
        for i in 0..moves {
            if send_input(session, conn.as_deref_mut(), kind) && i == 0 && first {
                crate::audio::play_move();
            }
        }
    }
}

fn autorepeat(timer: &mut f32, held: bool, dt: f32, settings: Settings) -> u32 {
    if !held {
        *timer = 0.0;
        return 0;
    }
    if *timer == 0.0 {
        *timer = f32::MIN_POSITIVE;
        return 1;
    }
    *timer += dt;
    let mut moves = 0;
    while *timer + 1e-4 >= settings.das_delay && moves < config::GRID_WIDTH as u32 {
        *timer -= settings.das_speed;
        moves += 1;
    }
    if moves == config::GRID_WIDTH as u32 {
        *timer = settings.das_delay;
    }
    moves
}

fn handle_soft_drop_key(app: &App, session: &mut GameSession, conn: Option<&mut Connection>) {
    let down = app.keyboard.is_down(KeyCode::ArrowDown);
    if down != session.soft_drop_held {
        session.soft_drop_held = down;
        let kind = if down {
            InputKind::SoftDropPress
        } else {
            InputKind::SoftDropRelease
        };
        send_input(session, conn, kind);
    }
}

#[cfg(test)]
mod tests {
    use shared::IncomingGarbage;

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

    #[test]
    fn the_satellite_turns_the_short_way_in_seven_frames() {
        let mut turn = crate::state::TurnAnim::default();
        turn.update(1, 3, 0.0);
        for _ in 0..7 {
            turn.update(1, 0, 1.0 / 60.0);
        }
        let (dr, dc) = turn.satellite();
        assert!((dr + 1.0).abs() < 1e-4 && dc.abs() < 1e-4, "not pointing up: {dr} {dc}");

        turn.update(1, 1, 3.0 / 60.0);
        let (_, dc) = turn.satellite();
        assert!(dc > 0.1 && dc < 0.95, "mid-turn should be part way right: {dc}");
    }

    #[test]
    fn a_new_pair_shows_its_rotation_at_once() {
        let mut turn = crate::state::TurnAnim::default();
        turn.update(1, 0, 0.0);
        turn.update(2, 2, 1.0 / 60.0);
        let (dr, _) = turn.satellite();
        assert!((dr - 1.0).abs() < 1e-4);
    }

    fn moves_over(frames: u32, dt: f32) -> Vec<u32> {
        let settings = Settings::default();
        let mut timer = 0.0;
        (0..frames)
            .map(|_| autorepeat(&mut timer, true, dt, settings))
            .collect()
    }

    #[test]
    fn a_held_key_moves_at_once_then_repeats_after_the_delay() {
        let moves = moves_over(14, 1.0 / 60.0);
        assert_eq!(moves[0], 1, "the press itself moves");
        let first_repeat = moves.iter().skip(1).position(|&m| m > 0).map(|i| i + 1);
        assert_eq!(first_repeat, Some(8), "Tsu repeats after 8 frames");
        assert_eq!(moves[8..].iter().sum::<u32>(), 3, "then every 2 frames: 8, 10, 12");
    }

    #[test]
    fn a_stall_does_not_unload_a_burst_of_moves() {
        let settings = Settings::default();
        let mut timer = 0.0;
        autorepeat(&mut timer, true, 1.0 / 60.0, settings);
        let burst = autorepeat(&mut timer, true, 5.0, settings);
        assert_eq!(burst, config::GRID_WIDTH as u32);
        let after: u32 = (0..10)
            .map(|_| autorepeat(&mut timer, true, 1.0 / 60.0, settings))
            .sum();
        assert!(after <= 6, "{after} moves in the 10 frames after the stall");
    }

    #[test]
    fn releasing_the_key_resets_it() {
        let settings = Settings::default();
        let mut timer = 0.0;
        autorepeat(&mut timer, true, 1.0 / 60.0, settings);
        assert_eq!(autorepeat(&mut timer, false, 1.0 / 60.0, settings), 0);
        assert_eq!(
            autorepeat(&mut timer, true, 1.0 / 60.0, settings),
            1,
            "a new press moves at once"
        );
    }
}
