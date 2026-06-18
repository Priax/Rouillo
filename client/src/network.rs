use ewebsock::{WsEvent, WsMessage};
use shared::*;

use crate::state::{GameSession, Screen, State};

pub fn handle_server_messages(state: &mut State) {
    let mut msgs = Vec::new();
    let mut lost = false;
    let player_id = state.player_id.clone();
    let auth_token = state.auth.as_ref().map(|a| a.token.clone());
    let username = state.auth.as_ref().map(|a| a.username.clone());
    let pending_join = state.pending_join.take();
    if let Some(net) = state.net.as_mut() {
        while let Some(event) = net.ws_receiver.try_recv() {
            match event {
                WsEvent::Opened => {
                    net.send(&ClientMessage::Hello {
                        player_id: player_id.clone(),
                        auth_token: auth_token.clone(),
                        username: username.clone(),
                    });
                    if let Some(id) = pending_join {
                        net.send(&ClientMessage::JoinRoom { id });
                    }
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
            let mut session = GameSession::new(slot);
            session.last_server_msg = "GameStart".to_string();
            state.session = Some(session);
            state.screen = Screen::Game;
        }
        ServerMessage::StateUpdate {
            p1_board,
            p2_board,
            p1_ack,
            p2_ack,
        } => {
            if let Some(session) = state.session.as_mut() {
                let (my_auth, opp_auth, my_ack) = match session.my_slot {
                    1 => (*p1_board, *p2_board, p1_ack),
                    2 => (*p2_board, *p1_board, p2_ack),
                    _ => return,
                };
                let prev_piece = session.predicted_board.active_piece.clone();
                let prev_piece_id = session.predicted_board.piece_id;
                let prev_fall_timer = session.predicted_board.fall_timer;
                let prev_opp_piece = session.other_board.active_piece.clone();
                let prev_opp_piece_id = session.other_board.piece_id;

                let prev_chain = session.board.chain_count;
                let prev_all_clear = session.board.last_was_all_clear;
                let prev_garbage = session.board.pending_garbage;
                let prev_state = session.board.state;
                let hard_drop_acked = session
                    .pending_inputs
                    .iter()
                    .any(|(seq, kind)| *seq <= my_ack && *kind == InputKind::HardDrop);

                session.board = my_auth.clone();
                session.other_board = opp_auth;
                session.my_ack = my_ack;
                session.pending_inputs.retain(|(seq, _)| *seq > my_ack);

                if let Some(&(_, sent)) = session
                    .sent_at
                    .iter()
                    .filter(|(seq, _)| *seq <= my_ack)
                    .max_by_key(|(seq, _)| *seq)
                {
                    session.last_rtt_ms = ((session.clock - sent) * 1000.0) as f32;
                }
                session.sent_at.retain(|(seq, _)| *seq > my_ack);

                session.last_server_msg = format!(
                    "StateUpdate ack={} pending={} seq={}",
                    my_ack,
                    session.pending_inputs.len(),
                    session.input_seq,
                );

                let mut predicted = my_auth;

                let same_piece = predicted.piece_id == prev_piece_id;
                if same_piece {
                    if let (Some(prev), Some(cur)) = (&prev_piece, predicted.active_piece.clone()) {
                        if prev.row > cur.row {
                            let mut p = cur;
                            while p.row < prev.row {
                                let mut next = p.clone();
                                next.row += 1;
                                if predicted.check_collision(&next) {
                                    break;
                                }
                                p.row += 1;
                            }
                            predicted.active_piece = Some(p);
                        }
                    }
                    predicted.fall_timer = prev_fall_timer;
                }

                for (_, kind) in session.pending_inputs.iter() {
                    predicted.apply_input(*kind);
                }

                if same_piece {
                    if let (Some(prev), Some(cur)) = (&prev_piece, predicted.active_piece.as_ref()) {
                        let off = &mut session.piece_visual_offset;
                        off.0 = (off.0 + (prev.row - cur.row) as f32).clamp(-3.0, 3.0);
                        off.1 = (off.1 + (prev.col - cur.col) as f32).clamp(-3.0, 3.0);
                    }
                } else {
                    session.piece_visual_offset = (0.0, 0.0);
                }

                if session.other_board.piece_id == prev_opp_piece_id {
                    if let (Some(prev), Some(cur)) = (&prev_opp_piece, session.other_board.active_piece.as_ref()) {
                        let off = &mut session.opponent_piece_offset;
                        off.0 = (off.0 + (prev.row - cur.row) as f32).clamp(-3.0, 3.0);
                        off.1 = (off.1 + (prev.col - cur.col) as f32).clamp(-3.0, 3.0);
                    }
                } else {
                    session.opponent_piece_offset = (0.0, 0.0);
                }

                session.predicted_board = predicted;

                if prev_state == GameState::Playing
                    && session.board.state == GameState::ResolvingMatches
                    && !hard_drop_acked
                {
                    crate::audio::play_lock();
                }
                let cc = session.board.chain_count;
                if cc > prev_chain {
                    crate::audio::play_pop(cc);
                    session.chain_display = Some((cc, 2.0));
                }
                if session.board.pending_garbage > prev_garbage {
                    crate::audio::play_garbage();
                }
                if session.board.last_was_all_clear && !prev_all_clear {
                    crate::audio::play_all_clear();
                    session.all_clear_timer = 3.0;
                }
            }
        }
        ServerMessage::Restart => {
            if let Some(session) = state.session.as_mut() {
                session.opponent_disconnected = false;
                session.input_seq = 0;
                session.my_ack = 0;
                session.pending_inputs.clear();
                session.sent_at.clear();
                session.chain_display = None;
                session.all_clear_timer = 0.0;
                session.piece_visual_offset = (0.0, 0.0);
                session.opponent_piece_offset = (0.0, 0.0);
                session.last_server_msg = "Restart".to_string();
            }
        }
        ServerMessage::OpponentDisconnected => {
            if let Some(session) = state.session.as_mut() {
                session.opponent_disconnected = true;
                session.last_server_msg = "OpponentDisconnected".to_string();
            }
        }
        ServerMessage::FriendInvitation {
            from_username,
            room_id,
            room_name,
        } => {
            state.pending_invitation = Some((from_username, room_id, room_name));
        }
    }
}
