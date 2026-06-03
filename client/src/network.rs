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
            reset_prediction(state);
        }
        ServerMessage::GameStart => {
            state.last_server_msg = "Game start".to_string();
            state.waiting_for_opponent = false;
            state.opponent_disconnected = false;
        }
        ServerMessage::StateUpdate { p1_board, p2_board, p1_ack, p2_ack } => {
            let (my_auth, opp_auth, my_ack) = match state.my_player_id {
                Some(1) => (p1_board, p2_board, p1_ack),
                Some(2) => (p2_board, p1_board, p2_ack),
                _ => return,
            };
            state.board = my_auth.clone();
            state.other_board = opp_auth;
            state.my_ack = my_ack;
            state.pending_inputs.retain(|(seq, _)| *seq > my_ack);
            state.predicted_board = my_auth;
            for (_, kind) in state.pending_inputs.iter() {
                state.predicted_board.apply_input(*kind);
            }
        }
        ServerMessage::Restart => {
            state.last_server_msg = "Redémarrage de la partie".to_string();
            state.opponent_disconnected = false;
            reset_prediction(state);
        }
        ServerMessage::OpponentDisconnected => {
            state.last_server_msg = "Adversaire déconnecté !".to_string();
            state.opponent_disconnected = true;
        }
    }
}

fn reset_prediction(state: &mut State) {
    state.input_seq = 0;
    state.my_ack = 0;
    state.pending_inputs.clear();
}
