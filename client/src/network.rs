use shared::*;
use crate::state::{State, Screen, GameSession};
use ewebsock::{WsEvent, WsMessage};

pub fn handle_server_messages(state: &mut State) {
    let mut msgs = Vec::new();
    let mut lost = false;
    let player_id = state.player_id.clone();
    if let Some(net) = state.net.as_mut() {
        while let Some(event) = net.ws_receiver.try_recv() {
            match event {
                WsEvent::Opened => {
                    net.send(&ClientMessage::Hello { player_id: player_id.clone() });
                }
                WsEvent::Message(WsMessage::Binary(bytes)) => {
                    if let Some(m) = shared::decode::<ServerMessage>(&bytes) {
                        msgs.push(m);
                    }
                }
                WsEvent::Closed | WsEvent::Error(_) => {
                    lost = true;
                }
                _ => {}
            }
        }
    }
    if lost {
        state.net = None;
        state.session = None;
        state.lobby = None;
        state.rooms.clear();
        state.screen = Screen::Menu;
        state.notice = "Connexion au serveur perdue.".to_string();
        return;
    }
    for m in msgs {
        process_message(state, m);
    }
}

fn process_message(state: &mut State, msg: ServerMessage) {
    match msg {
        ServerMessage::RoomList { rooms } => {
            state.rooms = rooms;
            if matches!(state.screen, Screen::RoomLobby | Screen::Game) {
                state.screen = Screen::RoomBrowser;
                state.session = None;
                state.lobby = None;
            }
        }
        ServerMessage::Lobby { info } => {
            state.lobby = Some(info);
            state.session = None;
            state.screen = Screen::RoomLobby;
        }
        ServerMessage::JoinFailed { reason } => {
            state.notice = reason;
        }
        ServerMessage::GameStart => {
            let slot = state.lobby.as_ref().map(|l| l.your_slot).unwrap_or(1);
            state.session = Some(GameSession::new(slot));
            state.screen = Screen::Game;
        }
        ServerMessage::StateUpdate { p1_board, p2_board, p1_ack, p2_ack } => {
            if let Some(session) = state.session.as_mut() {
                let (my_auth, opp_auth, my_ack) = match session.my_slot {
                    1 => (p1_board, p2_board, p1_ack),
                    2 => (p2_board, p1_board, p2_ack),
                    _ => return,
                };
                let prev_row = session.predicted_board.active_piece.as_ref().map(|p| p.row);

                session.board = my_auth.clone();
                session.other_board = opp_auth;
                session.my_ack = my_ack;
                session.pending_inputs.retain(|(seq, _)| *seq > my_ack);

                let mut predicted = my_auth;
                for (_, kind) in session.pending_inputs.iter() {
                    predicted.apply_input(*kind);
                }
                if let (Some(prev), Some(piece)) = (prev_row, predicted.active_piece.clone()) {
                    if prev == piece.row + 1 {
                        let mut lower = piece;
                        lower.row += 1;
                        if !predicted.check_collision(&lower) {
                            if let Some(p) = predicted.active_piece.as_mut() {
                                p.row += 1;
                            }
                        }
                    }
                }
                session.predicted_board = predicted;
            }
        }
        ServerMessage::Restart => {
            if let Some(session) = state.session.as_mut() {
                session.opponent_disconnected = false;
                session.input_seq = 0;
                session.my_ack = 0;
                session.pending_inputs.clear();
            }
        }
        ServerMessage::OpponentDisconnected => {
            if let Some(session) = state.session.as_mut() {
                session.opponent_disconnected = true;
            }
        }
    }
}
