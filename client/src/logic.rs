use notan::prelude::*;
use shared::{self, *};
use crate::state::{GameSession, Settings};
use ewebsock::WsMessage;

pub fn update_game(app: &mut App, session: &mut GameSession, settings: &Settings) -> bool {
    crate::network::handle_server_messages(session);

    let game_over = session.board.state == GameState::GameOver
        || session.other_board.state == GameState::GameOver;

    let can_leave = session.waiting_for_opponent
        || session.opponent_disconnected
        || session.board.state == GameState::Paused
        || game_over;
    if can_leave {
        let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
        let back_btn = crate::menu::back_to_menu_button(ww, wh);
        if back_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::KeyM) {
            return true;
        }
    }

    let delta_time = app.timer.delta_f32();
    let can_play = !session.waiting_for_opponent && !session.opponent_disconnected;
    if !can_play {
        return false;
    }

    handle_global_input(app, session);

    if !game_over && session.predicted_board.state == GameState::Playing {
        handle_game_input(app, session, settings, delta_time);
    } else {
        session.key_timer_left = 0.0;
        session.key_timer_right = 0.0;
        session.key_timer_down = 0.0;
    }

    false
}

fn send_input(session: &mut GameSession, kind: InputKind) {
    session.input_seq += 1;
    let seq = session.input_seq;
    session.predicted_board.apply_input(kind);
    session.pending_inputs.push((seq, kind));
    session.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::Input { kind, seq })));
}

fn handle_global_input(app: &mut App, session: &mut GameSession) {
    let can_restart = session.board.state == GameState::GameOver
        || session.other_board.state == GameState::GameOver
        || session.board.state == GameState::Paused;
    if app.keyboard.was_pressed(KeyCode::KeyR) && can_restart {
        session.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::RequestRestart)));
    }

    if app.keyboard.was_pressed(KeyCode::Escape) {
        session.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::TogglePause)));
    }
}

fn handle_game_input(app: &mut App, session: &mut GameSession, settings: &Settings, delta_time: f32) {
    if app.keyboard.was_pressed(KeyCode::ArrowUp) || app.keyboard.was_pressed(KeyCode::KeyZ) {
        send_input(session, InputKind::RotateCW);
    }
    if app.keyboard.was_pressed(KeyCode::KeyX) || app.keyboard.was_pressed(KeyCode::KeyW) {
        send_input(session, InputKind::RotateCCW);
    }

    if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Enter) {
        send_input(session, InputKind::HardDrop);
        return;
    }

    if app.keyboard.is_down(KeyCode::ArrowLeft) {
        if session.key_timer_left == 0.0 {
            send_input(session, InputKind::MoveLeft);
            session.key_timer_left = 0.0001;
        } else {
            session.key_timer_left += delta_time;
            while session.key_timer_left > settings.das_delay + settings.das_speed {
                send_input(session, InputKind::MoveLeft);
                session.key_timer_left -= settings.das_speed;
            }
        }
    } else {
        session.key_timer_left = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowRight) {
        if session.key_timer_right == 0.0 {
            send_input(session, InputKind::MoveRight);
            session.key_timer_right = 0.0001;
        } else {
            session.key_timer_right += delta_time;
            while session.key_timer_right > settings.das_delay + settings.das_speed {
                send_input(session, InputKind::MoveRight);
                session.key_timer_right -= settings.das_speed;
            }
        }
    } else {
        session.key_timer_right = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowDown) {
        session.key_timer_down += delta_time;
        if session.key_timer_down > settings.soft_drop_speed {
            send_input(session, InputKind::SoftDrop);
            session.key_timer_down = 0.0;
        }
    } else {
        session.key_timer_down = 0.0;
    }
}
