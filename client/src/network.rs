use shared::*;
use crate::state::GameSession;
use ewebsock::{WsEvent, WsMessage};

pub fn handle_server_messages(session: &mut GameSession) {
    while let Some(event) = session.ws_receiver.try_recv() {
        match event {
            WsEvent::Message(WsMessage::Binary(bytes)) => {
                if let Some(server_msg) = shared::decode::<ServerMessage>(&bytes) {
                    process_message(session, server_msg);
                }
            }
            _ => {}
        }
    }
}

fn process_message(session: &mut GameSession, msg: ServerMessage) {
    match msg {
        ServerMessage::RoomFull => {
            session.room_full = true;
            session.waiting_for_opponent = false;
        }
        ServerMessage::Welcome { player_id } => {
            session.last_server_msg = format!("Je suis Joueur {}", player_id);
            session.my_player_id = Some(player_id);
            session.waiting_for_opponent = true;
            session.opponent_disconnected = false;
            reset_prediction(session);
        }
        ServerMessage::GameStart => {
            session.last_server_msg = "Game start".to_string();
            session.waiting_for_opponent = false;
            session.opponent_disconnected = false;
        }
        ServerMessage::StateUpdate { p1_board, p2_board, p1_ack, p2_ack } => {
            let (my_auth, opp_auth, my_ack) = match session.my_player_id {
                Some(1) => (p1_board, p2_board, p1_ack),
                Some(2) => (p2_board, p1_board, p2_ack),
                _ => return,
            };
            session.board = my_auth.clone();
            session.other_board = opp_auth;
            session.my_ack = my_ack;
            session.pending_inputs.retain(|(seq, _)| *seq > my_ack);
            session.predicted_board = my_auth;
            for (_, kind) in session.pending_inputs.iter() {
                session.predicted_board.apply_input(*kind);
            }
        }
        ServerMessage::Restart => {
            session.last_server_msg = "Redémarrage de la partie".to_string();
            session.opponent_disconnected = false;
            reset_prediction(session);
        }
        ServerMessage::OpponentDisconnected => {
            session.last_server_msg = "Adversaire déconnecté !".to_string();
            session.opponent_disconnected = true;
        }
    }
}

fn reset_prediction(session: &mut GameSession) {
    session.input_seq = 0;
    session.my_ack = 0;
    session.pending_inputs.clear();
}
