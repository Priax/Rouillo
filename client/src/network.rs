use shared::*;
use crate::state::State;
use ewebsock::{WsEvent, WsMessage};
use crate::config;

pub fn handle_server_messages(state: &mut State) {
    while let Some(event) = state.ws_receiver.try_recv() {
        match event {
            WsEvent::Message(msg) => match msg {
                WsMessage::Text(text) => {
                    if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                       process_message(state, server_msg);
                    }
                }
                _ => {}
            },
            WsEvent::Opened => {
                let join_msg = ClientMessage::Join { name: "Joueur".to_string() };
                if let Ok(json) = serde_json::to_string(&join_msg) { 
                    state.ws_sender.send(WsMessage::Text(json)); 
                }
            },
            _ => {}
        }
    }
}

fn process_message(state: &mut State, msg: ServerMessage) {
    match msg {
        ServerMessage::Welcome { random_seed, player_id } => {
            state.last_server_msg = format!("Je suis Joueur {}", player_id);
            state.my_player_id = Some(player_id);
            state.initial_seed = random_seed;
            state.board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, random_seed);
            state.board.spawn_piece();
            state.other_board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, random_seed);
            state.played_time = 0.0;
            state.game_over_sent = false;
            state.did_i_win = false;
            state.opponent_disconnected = false;
        }
        ServerMessage::GameStart => {
            state.last_server_msg = "Game start".to_string();
            state.waiting_for_opponent = false;
            state.opponent_disconnected = false;
            state.played_time = 0.0;
        }
        ServerMessage::OpponentAction { player_id, col, rot, axis_color_idx, sat_color_idx } => {
            if Some(player_id) != state.my_player_id {
                // state.last_server_msg = format!("Opponent placed: col {}", col);
                update_opponent_board(&mut state.other_board, col, rot, axis_color_idx, sat_color_idx);
            }
        }
        ServerMessage::PlayerEliminated { player_id } => {
            state.last_server_msg = format!("Player {} lost", player_id);
            if Some(player_id) != state.my_player_id {
                state.did_i_win = true;
                state.board.state = GameState::GameOver; 
            }
        }
        ServerMessage::Restart { new_seed } => {
            state.last_server_msg = "Redémarrage de la partie".to_string();
             state.initial_seed = new_seed;
             state.board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, new_seed);
             state.board.spawn_piece();
             state.other_board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, new_seed);
             state.played_time = 0.0;
             state.game_over_sent = false;
             state.did_i_win = false;
             if state.board.state == GameState::Paused { state.board.state = GameState::Playing; }
        }
        ServerMessage::GameStateChange { paused } => {
             state.last_server_msg = format!("Pause: {}", paused);
             state.board.toggle_pause();
        }
        ServerMessage::OpponentDisconnected => {
            state.last_server_msg = "Adversaire déconnecté !".to_string();
            state.opponent_disconnected = true;
            if state.board.state == GameState::Playing {
                state.board.toggle_pause();
            }
        }
        ServerMessage::RequestSnapshot { requester_id } => {
            state.last_server_msg = format!("Envoi snapshot vers J{}", requester_id);
            let msg = ClientMessage::FullGameState {
                my_board: state.board.clone(),
                opponent_board: state.other_board.clone(),
                scores: (state.board.score, state.other_board.score),
                requester_id
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                state.ws_sender.send(WsMessage::Text(json));
            }
            state.opponent_disconnected = false;
        }
        ServerMessage::SyncState { my_board, opponent_board, scores, target_player_id } => {
            if Some(target_player_id) == state.my_player_id {
                state.last_server_msg = "État synchronisé reçu !".to_string();
                state.board = my_board;
                state.other_board = opponent_board;
                state.board.score = scores.0;
                state.other_board.score = scores.1;

                if state.board.active_piece.is_none() && state.board.state == GameState::Playing {
                    state.board.spawn_piece();
                }
                state.waiting_for_opponent = false; 
                state.opponent_disconnected = false;
                if state.board.state != GameState::Paused {
                     state.board.state = GameState::Playing;
                }
            } else {
                state.last_server_msg = "Adversaire resynchronisé".to_string();
                state.opponent_disconnected = false;
                if state.board.state == GameState::Paused {
                    state.board.toggle_pause(); 
                }
            }
        }
    }
}

fn update_opponent_board(board: &mut Board, col: i32, rot: usize, c1: u8, c2: u8) {
    let piece = ActivePuyo { row: 1, col, rotation: rot, axis_type: PuyoType::from_u8(c1), sat_type: PuyoType::from_u8(c2) };
    let mut ghost = piece.clone();
    if board.check_collision(&ghost) { return; }
    while !board.check_collision(&ghost) { ghost.row += 1; }
    ghost.row -= 1; 
    for (r, c) in ghost.get_positions().iter() {
        if *r >= 0 && *r < board.height as i32 && *c >= 0 && *c < board.width as i32 {
            let p_type = if *r == ghost.row && *c == ghost.col { ghost.axis_type } else { ghost.sat_type };
            board.cells[*r as usize][*c as usize] = Some(p_type);
        }
    }
    board.apply_board_gravity();
    loop {
        if board.check_matches() { board.apply_board_gravity(); } else { break; }
    }
}
