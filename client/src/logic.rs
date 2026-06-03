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

    // Fin de partie : le gagnant garde un board `Playing` gelé côté serveur ; sans ce garde, sa pièce
    // prédite bougerait derrière l'écran de victoire. On bloque donc les inputs dès qu'un board tombe.
    let game_over = state.board.state == GameState::GameOver
        || state.other_board.state == GameState::GameOver;

    // On se base sur le board prédit (ce qu'on contrôle localement) : dès qu'on a prédit un
    // verrouillage (hard-drop), on cesse d'émettre des inputs jusqu'au spawn de la pièce suivante.
    if !game_over && state.predicted_board.state == GameState::Playing {
        handle_game_input(app, state, delta_time);
    } else {
        state.key_timer_left = 0.0;
        state.key_timer_right = 0.0;
        state.key_timer_down = 0.0;
    }
}

// Prédiction : on applique l'input localement tout de suite (contrôle instantané), on le bufferise
// pour la réconciliation, et on l'envoie au serveur avec son numéro de séquence.
fn send_input(state: &mut State, kind: InputKind) {
    state.input_seq += 1;
    let seq = state.input_seq;
    state.predicted_board.apply_input(kind);
    state.pending_inputs.push((seq, kind));
    state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::Input { kind, seq })));
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
