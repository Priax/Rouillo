use shared::*;
use crate::state::State;
use ewebsock::{WsEvent, WsMessage};

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
        ServerMessage::Welcome { player_id } => {
            state.last_server_msg = format!("Je suis Joueur {}", player_id);
            state.my_player_id = Some(player_id);
            state.waiting_for_opponent = true;
            state.opponent_disconnected = false;
        }
        ServerMessage::GameStart => {
            state.last_server_msg = "Game start".to_string();
            state.waiting_for_opponent = false;
            state.opponent_disconnected = false;
        }
        ServerMessage::StateUpdate { p1_board, p2_board } => {
            match state.my_player_id {
                Some(1) => { state.board = p1_board; state.other_board = p2_board; }
                Some(2) => { state.board = p2_board; state.other_board = p1_board; }
                _ => {}
            }
        }
        ServerMessage::Restart => {
            state.last_server_msg = "Redémarrage de la partie".to_string();
            state.opponent_disconnected = false;
        }
        ServerMessage::OpponentDisconnected => {
            state.last_server_msg = "Adversaire déconnecté !".to_string();
            state.opponent_disconnected = true;
        }
    }
}
