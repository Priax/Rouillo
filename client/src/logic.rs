use notan::prelude::*;
use shared::{self, *};
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
    if app.keyboard.was_pressed(KeyCode::KeyR) && (state.board.state == GameState::GameOver || state.board.state == GameState::Paused) {
        state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::RequestRestart)));
    }

    if app.keyboard.was_pressed(KeyCode::Escape) {
        state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::TogglePause)));
    }

    if state.board.state == GameState::GameOver && !state.game_over_sent && !state.did_i_win {
        state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::GameOver)));
        state.game_over_sent = true;
    }
}

fn handle_game_input(app: &mut App, state: &mut State, delta_time: f32, time_now: f32) {
    if app.keyboard.was_pressed(KeyCode::ArrowUp) || app.keyboard.was_pressed(KeyCode::KeyZ) { state.board.rotate_piece(1); }
    if app.keyboard.was_pressed(KeyCode::KeyX) || app.keyboard.was_pressed(KeyCode::KeyW) { state.board.rotate_piece(3); }

    let piece_locked_now = if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Enter) {
        if let Some(piece) = state.board.active_piece.clone() {
            let (col, rot, axis_type, sat_type) = (piece.col, piece.rotation as u8, piece.axis_type, piece.sat_type);
            state.board.hard_drop();
            state.my_piece_seq += 1;
            state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::PieceLocked {
                col, rot, axis_type, sat_type, seq: state.my_piece_seq,
            })));
        }
        state.last_fall_time = time_now;
        true
    } else { false };

    if !piece_locked_now {
        if app.keyboard.is_down(KeyCode::ArrowLeft) {
            if state.key_timer_left == 0.0 {
                state.board.move_piece(-1);
                state.key_timer_left = 0.0001;
            } else {
                state.key_timer_left += delta_time;
                while state.key_timer_left > config::DAS_DELAY + config::DAS_SPEED {
                    state.board.move_piece(-1);
                    state.key_timer_left -= config::DAS_SPEED;
                }
            }
        } else {
            state.key_timer_left = 0.0;
        }

        if app.keyboard.is_down(KeyCode::ArrowRight) {
            if state.key_timer_right == 0.0 {
                state.board.move_piece(1);
                state.key_timer_right = 0.0001;
            } else {
                state.key_timer_right += delta_time;
                while state.key_timer_right > config::DAS_DELAY + config::DAS_SPEED {
                    state.board.move_piece(1);
                    state.key_timer_right -= config::DAS_SPEED;
                }
            }
        } else {
            state.key_timer_right = 0.0;
        }

        if app.keyboard.is_down(KeyCode::ArrowDown) {
            state.key_timer_down += delta_time;
            if state.key_timer_down > config::SOFT_DROP_SPEED {
                state.board.force_drop();
                state.last_fall_time = time_now;
                state.key_timer_down = 0.0;
            }
        } else {
            state.key_timer_down = 0.0;
        }
    }
}

fn update_physics(state: &mut State, delta_time: f32, time_now: f32, current_interval: f64) {
    let prev_board_state = state.board.state;
    match state.board.state {
        GameState::Playing => {
            let pending_lock_info = if let Some(piece) = &state.board.active_piece {
                Some((piece.col, piece.rotation as u8, piece.axis_type, piece.sat_type))
            } else { None };

            let locked = state.board.update_logic(delta_time);
            if locked {
                if let Some((col, rot, axis_type, sat_type)) = pending_lock_info {
                    state.my_piece_seq += 1;
                    state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::PieceLocked {
                        col, rot, axis_type, sat_type, seq: state.my_piece_seq,
                    })));
                }
                state.last_fall_time = time_now;
            } else {
                if !state.board.is_touching_ground && (time_now - state.last_fall_time > current_interval as f32) {
                    state.board.force_drop();
                    state.last_fall_time = time_now;
                }
            }
        },
        GameState::ResolvingMatches => {
            if time_now - state.last_resolve_time > 0.10 { 
                let garbage_sent = state.board.resolve_step(); 

                if garbage_sent > 0 {
                    state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::GarbageSent { amount: garbage_sent })));
                    state.last_server_msg = format!("Attaque ! {} blocs", garbage_sent);
                }

                state.last_resolve_time = time_now; 
            }
        },
        GameState::DroppingGarbage => {
            state.garbage_delay_timer += delta_time;

            if state.garbage_delay_timer > 0.5 {
                state.board.drop_garbage();
                state.board.apply_board_gravity();

                if state.board.state != GameState::GameOver {
                    state.board.state = GameState::Playing;
                    state.board.spawn_piece(); // pose GameOver en interne si la colonne de spawn est bloquée
                    state.board.lock_timer = 0.0;
                    state.board.total_ground_timer = 0.0;
                    state.last_fall_time = time_now;
                }

                state.garbage_delay_timer = 0.0;
            }
        },
        _ => {}
    }

    if prev_board_state != GameState::Playing && state.board.state == GameState::Playing {
        // Les cellules ne sont envoyées qu'après un DroppingGarbage pour corriger les divergences
        // de placement aléatoire. Après une résolution de chaîne, OpponentAction suffit.
        let cells = if prev_board_state == GameState::DroppingGarbage {
            Some(state.board.cells.clone())
        } else {
            None
        };
        state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::BoardSync {
            cells,
            pending_garbage: state.board.pending_garbage,
            score: state.board.score,
            next_types: state.board.next_types,
            next_next_types: state.board.next_next_types,
        })));
    }
}
