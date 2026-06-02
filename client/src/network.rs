use shared::*;
use crate::state::State;
use ewebsock::{WsEvent, WsMessage};
use crate::config;

pub fn handle_server_messages(state: &mut State) {
    while let Some(event) = state.ws_receiver.try_recv() {
        match event {
            WsEvent::Message(WsMessage::Binary(bytes)) => {
                if let Some(server_msg) = shared::decode::<ServerMessage>(&bytes) {
                    process_message(state, server_msg);
                }
            }
            _ => {}
        }
    }
}

fn process_message(state: &mut State, msg: ServerMessage) {
    match msg {
        ServerMessage::RoomFull => {
            state.room_full = true;
            state.waiting_for_opponent = false;
        }
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
            state.my_piece_seq = 0;
            state.expected_opponent_seq = 0;
            state.resync_pending = false;
        }
        ServerMessage::GameStart => {
            state.last_server_msg = "Game start".to_string();
            state.waiting_for_opponent = false;
            state.opponent_disconnected = false;
            state.my_piece_seq = 0;
            state.expected_opponent_seq = 0;
            state.resync_pending = false;
            if state.board.state == GameState::Paused {
                state.board.toggle_pause();
            }
        }
        ServerMessage::OpponentAction { col, rot, axis_type, sat_type, seq, .. } => {
            if state.expected_opponent_seq > 0 && seq != state.expected_opponent_seq + 1 {
                state.last_server_msg = format!("DESYNC: seq attendu {}, reçu {}", state.expected_opponent_seq + 1, seq);
                if !state.resync_pending {
                    state.resync_pending = true;
                    state.ws_sender.send(WsMessage::Binary(shared::encode(&ClientMessage::RequestSync)));
                }
            }
            state.expected_opponent_seq = seq;
            update_opponent_board(&mut state.other_board, col, rot, axis_type, sat_type);
        }
        ServerMessage::GarbageAttack { attacker_id, amount } => {
            state.last_server_msg = format!("Reçu {} garbage de J{}", amount, attacker_id);
            state.board.pending_garbage += amount;
        }
        ServerMessage::PlayerEliminated { player_id } => {
            state.last_server_msg = format!("Player {} lost", player_id);
            state.did_i_win = true;
            state.board.state = GameState::GameOver;
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
            state.my_piece_seq = 0;
            state.expected_opponent_seq = 0;
            state.resync_pending = false;
        }
        ServerMessage::GameStateChange { paused } => {
            state.last_server_msg = format!("Pause: {}", paused);
            state.board.set_paused(paused);
        }
        ServerMessage::OpponentBoardSync { cells, pending_garbage, score, next_types, next_next_types, .. } => {
            if let Some(c) = cells {
                state.other_board.cells = c;
            }
            state.other_board.pending_garbage = pending_garbage;
            state.other_board.score = score;
            state.other_board.next_types = next_types;
            state.other_board.next_next_types = next_next_types;
        }
        ServerMessage::OpponentDisconnected => {
            state.last_server_msg = "Adversaire déconnecté !".to_string();
            state.opponent_disconnected = true;
            state.board.set_paused(true);
        }
        ServerMessage::RequestSnapshot { requester_id } => {
            if Some(requester_id) != state.my_player_id {
                state.last_server_msg = format!("Envoi snapshot vers J{}", requester_id);
                let msg = ClientMessage::FullGameState {
                    my_board: state.board.clone(),
                    opponent_board: state.other_board.clone(),
                    requester_id,
                    my_piece_seq: state.my_piece_seq,
                };
                state.ws_sender.send(WsMessage::Binary(shared::encode(&msg)));
            }
            state.opponent_disconnected = false;
        }
        ServerMessage::SyncState { my_board, opponent_board, target_player_id, opponent_piece_seq } => {
            if Some(target_player_id) == state.my_player_id {
                state.last_server_msg = "État synchronisé reçu !".to_string();
                state.board = my_board;
                state.other_board = opponent_board;
                state.board.state = GameState::Playing;
                if state.board.active_piece.is_none() {
                    state.board.spawn_piece();
                }
                state.waiting_for_opponent = false;
                state.opponent_disconnected = false;
                state.expected_opponent_seq = opponent_piece_seq;
                state.resync_pending = false;
            } else {
                state.last_server_msg = "Adversaire resynchronisé".to_string();
                state.opponent_disconnected = false;
                state.resync_pending = false;
                state.expected_opponent_seq = 0;
                if state.board.state == GameState::Paused {
                    state.board.toggle_pause();
                }
            }
        }
    }
}

fn update_opponent_board(board: &mut Board, col: i32, rot: u8, axis_type: PuyoType, sat_type: PuyoType) {
    let piece = ActivePuyo { row: 1, col, rotation: rot as usize, axis_type, sat_type };
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
    
    let fell = board.apply_board_gravity();
    
    let mut matched = false;
    loop {
        if board.check_matches().is_some() { 
            matched = true;
            board.apply_board_gravity(); 
        } else { 
            break; 
        }
    }

    if !fell && !matched {
        board.drop_garbage();
        board.apply_board_gravity();
    }
}
