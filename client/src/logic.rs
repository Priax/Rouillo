use notan::prelude::*;
use shared::*;
use crate::state::{GameSession, Settings, Net};

pub fn update_game(app: &mut App, session: &mut GameSession, settings: &Settings, net: &mut Net, is_host: bool) {
    let game_over = session.board.state == GameState::GameOver
        || session.other_board.state == GameState::GameOver;
    let paused = session.board.state == GameState::Paused;

    if paused || game_over || session.opponent_disconnected {
        let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
        if crate::rooms::leave_room_button(ww, wh).clicked(app) {
            net.send(&ClientMessage::LeaveRoom);
            return;
        }
        if is_host && crate::rooms::back_to_lobby_button(ww, wh).clicked(app) {
            net.send(&ClientMessage::ReturnToLobby);
            return;
        }
    }

    if session.opponent_disconnected {
        return;
    }

    handle_global_input(app, session, net);

    let dt = app.timer.delta_f32();
    if !game_over && session.predicted_board.state == GameState::Playing {
        handle_game_input(app, session, settings, net, dt);
        let row_before = session.predicted_board.active_piece.as_ref().map(|p| p.row);
        session.predicted_board.predict_fall(dt);
        if let (Some(before), Some(after)) =
            (row_before, session.predicted_board.active_piece.as_ref().map(|p| p.row))
        {
            if after > before {
                session.piece_visual_offset.0 -= (after - before) as f32;
            }
        }
    } else {
        session.key_timer_left = 0.0;
        session.key_timer_right = 0.0;
        session.key_timer_down = 0.0;
    }

    let decay = (-PIECE_SMOOTH_RATE * dt).exp();
    session.piece_visual_offset.0 *= decay;
    session.piece_visual_offset.1 *= decay;
    if session.piece_visual_offset.0.abs() < 0.001 { session.piece_visual_offset.0 = 0.0; }
    if session.piece_visual_offset.1.abs() < 0.001 { session.piece_visual_offset.1 = 0.0; }
}

const PIECE_SMOOTH_RATE: f32 = 22.0;

fn send_input(session: &mut GameSession, net: &mut Net, kind: InputKind) {
    session.input_seq += 1;
    let seq = session.input_seq;
    session.predicted_board.apply_input(kind);
    session.pending_inputs.push((seq, kind));
    net.send(&ClientMessage::Input { kind, seq });
}

fn handle_global_input(app: &mut App, session: &mut GameSession, net: &mut Net) {
    let can_restart = session.board.state == GameState::GameOver
        || session.other_board.state == GameState::GameOver
        || session.board.state == GameState::Paused;
    if app.keyboard.was_pressed(KeyCode::KeyR) && can_restart {
        net.send(&ClientMessage::RequestRestart);
    }

    if app.keyboard.was_pressed(KeyCode::Escape) {
        net.send(&ClientMessage::TogglePause);
    }

    let _ = session;
}

fn handle_game_input(app: &mut App, session: &mut GameSession, settings: &Settings, net: &mut Net, delta_time: f32) {
    if app.keyboard.was_pressed(KeyCode::ArrowUp) || app.keyboard.was_pressed(KeyCode::KeyZ) {
        send_input(session, net, InputKind::RotateCW);
    }
    if app.keyboard.was_pressed(KeyCode::KeyX) || app.keyboard.was_pressed(KeyCode::KeyW) {
        send_input(session, net, InputKind::RotateCCW);
    }

    if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Enter) {
        send_input(session, net, InputKind::HardDrop);
        return;
    }

    if app.keyboard.is_down(KeyCode::ArrowLeft) {
        if session.key_timer_left == 0.0 {
            send_input(session, net, InputKind::MoveLeft);
            session.key_timer_left = 0.0001;
        } else {
            session.key_timer_left += delta_time;
            while session.key_timer_left > settings.das_delay + settings.das_speed {
                send_input(session, net, InputKind::MoveLeft);
                session.key_timer_left -= settings.das_speed;
            }
        }
    } else {
        session.key_timer_left = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowRight) {
        if session.key_timer_right == 0.0 {
            send_input(session, net, InputKind::MoveRight);
            session.key_timer_right = 0.0001;
        } else {
            session.key_timer_right += delta_time;
            while session.key_timer_right > settings.das_delay + settings.das_speed {
                send_input(session, net, InputKind::MoveRight);
                session.key_timer_right -= settings.das_speed;
            }
        }
    } else {
        session.key_timer_right = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowDown) {
        session.key_timer_down += delta_time;
        if session.key_timer_down > settings.soft_drop_speed {
            send_input(session, net, InputKind::SoftDrop);
            session.key_timer_down = 0.0;
        }
    } else {
        session.key_timer_down = 0.0;
    }
}
