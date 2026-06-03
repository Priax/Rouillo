use notan::prelude::*;
use shared::{self, *};
use crate::state::State;
use ewebsock::WsMessage;
use crate::config;

pub fn update(app: &mut App, state: &mut State) {
    crate::network::handle_server_messages(state);

    let delta_time = app.timer.delta_f32();
    let can_play = !state.waiting_for_opponent && !state.opponent_disconnected;
    if !can_play {
        return;
    }

    handle_global_input(app, state);

    if state.board.state == GameState::Playing {
        handle_game_input(app, state, delta_time);
    } else {
        state.key_timer_left = 0.0;
        state.key_timer_right = 0.0;
        state.key_timer_down = 0.0;
    }
}

fn send_input(state: &mut State, kind: InputKind) {
    state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::Input { kind })));
}

fn handle_global_input(app: &mut App, state: &mut State) {
    let can_restart = state.board.state == GameState::GameOver
        || state.other_board.state == GameState::GameOver
        || state.board.state == GameState::Paused;
    if app.keyboard.was_pressed(KeyCode::KeyR) && can_restart {
        state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::RequestRestart)));
    }

    if app.keyboard.was_pressed(KeyCode::Escape) {
        state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::TogglePause)));
    }
}

fn handle_game_input(app: &mut App, state: &mut State, delta_time: f32) {
    if app.keyboard.was_pressed(KeyCode::ArrowUp) || app.keyboard.was_pressed(KeyCode::KeyZ) {
        send_input(state, InputKind::RotateCW);
    }
    if app.keyboard.was_pressed(KeyCode::KeyX) || app.keyboard.was_pressed(KeyCode::KeyW) {
        send_input(state, InputKind::RotateCCW);
    }

    if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Enter) {
        send_input(state, InputKind::HardDrop);
        return;
    }

    if app.keyboard.is_down(KeyCode::ArrowLeft) {
        if state.key_timer_left == 0.0 {
            send_input(state, InputKind::MoveLeft);
            state.key_timer_left = 0.0001;
        } else {
            state.key_timer_left += delta_time;
            while state.key_timer_left > config::DAS_DELAY + config::DAS_SPEED {
                send_input(state, InputKind::MoveLeft);
                state.key_timer_left -= config::DAS_SPEED;
            }
        }
    } else {
        state.key_timer_left = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowRight) {
        if state.key_timer_right == 0.0 {
            send_input(state, InputKind::MoveRight);
            state.key_timer_right = 0.0001;
        } else {
            state.key_timer_right += delta_time;
            while state.key_timer_right > config::DAS_DELAY + config::DAS_SPEED {
                send_input(state, InputKind::MoveRight);
                state.key_timer_right -= config::DAS_SPEED;
            }
        }
    } else {
        state.key_timer_right = 0.0;
    }

    if app.keyboard.is_down(KeyCode::ArrowDown) {
        state.key_timer_down += delta_time;
        if state.key_timer_down > config::SOFT_DROP_SPEED {
            send_input(state, InputKind::SoftDrop);
            state.key_timer_down = 0.0;
        }
    } else {
        state.key_timer_down = 0.0;
    }
}
