use notan::prelude::*;
use shared::*;
use crate::state::State;
use ewebsock::WsMessage;
use crate::config;

pub fn update(app: &mut App, state: &mut State) {
    crate::network::handle_server_messages(state);

    let time_now = app.timer.elapsed_f32();
    let delta_time = app.timer.delta_f32();
    let can_play = !state.waiting_for_opponent && !state.opponent_disconnected;

    if can_play {
        if state.board.state == GameState::Playing || state.board.state == GameState::ResolvingMatches {
            state.played_time += delta_time;
        }

        let level = 1 + (state.played_time / 15.0) as u32;
        let speed_decrease = (level as f64 - 1.0) * 0.05;
        let current_interval = if speed_decrease >= (0.8 - 0.1) { 0.1 } else { 0.8 - speed_decrease };

        handle_global_input(app, state);

        if state.board.state == GameState::Playing {
            handle_game_input(app, state, delta_time, time_now);
        }

        update_physics(state, delta_time, time_now, current_interval);
    }
}

fn handle_global_input(app: &mut App, state: &mut State) {
    if app.keyboard.was_pressed(KeyCode::R) && (state.board.state == GameState::GameOver || state.board.state == GameState::Paused) {
        let msg = ClientMessage::RequestRestart;
        if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); }
    }
    
    if app.keyboard.was_pressed(KeyCode::Escape) { 
        let msg = ClientMessage::TogglePause;
        if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); }
    }

    if state.board.state == GameState::GameOver && !state.game_over_sent && !state.did_i_win {
        let msg = ClientMessage::GameOver;
        if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); }
        state.game_over_sent = true;
    }
}

fn handle_game_input(app: &mut App, state: &mut State, delta_time: f32, time_now: f32) {
    if app.keyboard.was_pressed(KeyCode::Up) || app.keyboard.was_pressed(KeyCode::Z) { state.board.rotate_piece(1); }
    if app.keyboard.was_pressed(KeyCode::X) || app.keyboard.was_pressed(KeyCode::W) { state.board.rotate_piece(3); }

    let piece_locked_now = if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Return) {
        if let Some(piece) = &state.board.active_piece {
            let action_msg = ClientMessage::PieceLocked { 
                col: piece.col, rot: piece.rotation, 
                axis_color_idx: piece.axis_type.to_u8(), sat_color_idx: piece.sat_type.to_u8() 
            };
            if let Ok(json) = serde_json::to_string(&action_msg) { state.ws_sender.send(WsMessage::Text(json)); }
        }
        state.board.hard_drop();
        state.last_fall_time = time_now;
        true
    } else { false };

    if !piece_locked_now {
        if app.keyboard.is_down(KeyCode::Left) {
            if state.key_timer_left == 0.0 { state.board.move_piece(-1); state.key_timer_left = 0.0001; }
            else {
                state.key_timer_left += delta_time;
                if state.key_timer_left > config::DAS_DELAY { while state.key_timer_left > config::DAS_DELAY + config::DAS_SPEED { state.board.move_piece(-1); state.key_timer_left -= config::DAS_SPEED; } }
            }
        } else { state.key_timer_left = 0.0; }

        if app.keyboard.is_down(KeyCode::Right) {
            if state.key_timer_right == 0.0 { state.board.move_piece(1); state.key_timer_right = 0.0001; }
            else {
                state.key_timer_right += delta_time;
                if state.key_timer_right > config::DAS_DELAY { while state.key_timer_right > config::DAS_DELAY + config::DAS_SPEED { state.board.move_piece(1); state.key_timer_right -= config::DAS_SPEED; } }
            }
        } else { state.key_timer_right = 0.0; }

        if app.keyboard.is_down(KeyCode::Down) {
            state.key_timer_down += delta_time;
            if state.key_timer_down > config::SOFT_DROP_SPEED {
                state.board.force_drop();
                state.last_fall_time = time_now;
                state.key_timer_down = 0.0;
            }
        } else { state.key_timer_down = 0.0; }
    }
}

fn update_physics(state: &mut State, delta_time: f32, time_now: f32, current_interval: f64) {
    match state.board.state {
        GameState::Playing => {
            let pending_lock_msg = if let Some(piece) = &state.board.active_piece {
                Some(ClientMessage::PieceLocked { 
                    col: piece.col, rot: piece.rotation, axis_color_idx: piece.axis_type.to_u8(), sat_color_idx: piece.sat_type.to_u8() 
                })
            } else { None };

            let locked = state.board.update_logic(delta_time);
            if locked {
                if let Some(msg) = pending_lock_msg { if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); } }
                state.last_fall_time = time_now;
            } else {
                if !state.board.is_touching_ground && (time_now - state.last_fall_time > current_interval as f32) {
                    state.board.force_drop();
                    state.last_fall_time = time_now;
                }
            }
        },
        GameState::ResolvingMatches => {
            if time_now - state.last_resolve_time > 0.15 { state.board.resolve_step(); state.last_resolve_time = time_now; }
        },
        _ => {}
    }
}
