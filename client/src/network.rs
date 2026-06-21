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
            p1_rng,
            p2_rng,
            p1_ack,
            p2_ack,
        } => {
            if let Some(session) = state.session.as_mut() {
                // The server sends both boards every update; pick which one is "us"
                // based on our slot. `my_ack` is the highest input seq the server has
                // already folded into our authoritative board.
                let (mut my_auth, mut opp_auth, my_ack, my_rng, opp_rng) = match session.my_slot {
                    1 => (*p1_board, *p2_board, p1_ack, p1_rng, p2_rng),
                    2 => (*p2_board, *p1_board, p2_ack, p2_rng, p1_rng),
                    _ => return,
                };

                // Boards arrive without their RNG (skipped on the wire). Restore it from the
                // message when the server sent it (it advanced), otherwise carry forward the
                // RNG we already hold for that board — it hasn't changed.
                match my_rng {
                    Some(r) => my_auth.set_rng(*r),
                    None => my_auth.set_rng(session.board.rng_state()),
                }
                match opp_rng {
                    Some(r) => opp_auth.set_rng(*r),
                    None => opp_auth.set_rng(session.other_board.rng_state()),
                }

                // Snapshot the locally *predicted* piece before we overwrite anything.
                // We need it to (a) keep our prediction from snapping backwards and
                // (b) compute a visual smoothing offset between old and new positions.
                let prev_piece = session.predicted_board.active_piece.clone();
                let prev_piece_id = session.predicted_board.piece_id;
                let prev_fall_timer = session.predicted_board.fall_timer;
                let prev_opp_piece = session.other_board.active_piece.clone();
                let prev_opp_piece_id = session.other_board.piece_id;

                // Snapshot fields of the *previous authoritative* board so we can detect
                // edge transitions (new chain, all-clear, incoming garbage, lock) and
                // fire the matching sound effects exactly once below.
                let prev_chain = session.board.chain_count;
                let prev_all_clear = session.board.last_was_all_clear;
                let prev_garbage = session.board.pending_garbage;
                let prev_state = session.board.state;
                // Did the server already process a HardDrop we sent? If so, the lock it
                // produced is "our" hard drop, we suppress the lock sound for it because
                // send_input() already played one locally when the key was pressed.
                let hard_drop_acked = session
                    .pending_inputs
                    .iter()
                    .any(|(seq, kind)| *seq <= my_ack && *kind == InputKind::HardDrop);

                // Adopt the authoritative state, and drop every pending input the server
                // has already acknowledged (seq <= my_ack); only unacked inputs remain to
                // be re-applied on top of the prediction.
                session.board = my_auth.clone();
                session.other_board = opp_auth;
                session.my_ack = my_ack;
                session.pending_inputs.retain(|(seq, _)| *seq > my_ack);

                // RTT = time elapsed since we sent the most recent acked input. We take
                // the highest acked seq so the measurement reflects the freshest round trip.
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

                // Rebuild the predicted board: start from the fresh authoritative state,
                // then replay the inputs the server hasn't seen yet.
                let mut predicted = my_auth;

                // `piece_id` increments on every spawn. If it's unchanged, the server and
                // client are still talking about the *same* falling piece, so we can carry
                // our local prediction forward. If it changed, a new piece spawned and we
                // simply accept the authoritative position (no re-projection).
                let same_piece = predicted.piece_id == prev_piece_id;
                if same_piece {
                    // The server's snapshot is slightly stale (older than our local view by
                    // ~RTT), so its piece sits higher than where we'd already predicted it.
                    // Re-drop the piece down to the row we had locally but never *through*
                    // landed puyos so the piece doesn't visibly snap upward each update.
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
                    // Keep our local gravity accumulator so the next fall step is timed
                    // from where we were, not reset by the server snapshot.
                    predicted.fall_timer = prev_fall_timer;
                }

                // Replay the still-unacked inputs to reach the present predicted state.
                for (_, kind) in session.pending_inputs.iter() {
                    predicted.apply_input(*kind);
                }

                // Any residual gap between the old and reconciled piece position is pushed
                // into a visual offset (in cells) that the renderer eases back to zero each
                // frame, turning a correction into a smooth slide instead of a teleport.
                // Clamped to ±3 cells so a big desync can't fling the sprite off-screen.
                if same_piece {
                    if let (Some(prev), Some(cur)) = (&prev_piece, predicted.active_piece.as_ref()) {
                        let off = &mut session.piece_visual_offset;
                        off.0 = (off.0 + (prev.row - cur.row) as f32).clamp(-3.0, 3.0);
                        off.1 = (off.1 + (prev.col - cur.col) as f32).clamp(-3.0, 3.0);
                    }
                } else {
                    // New piece: nothing to smooth, snap the offset to zero.
                    session.piece_visual_offset = (0.0, 0.0);
                }

                // Same smoothing for the opponent's piece. We don't predict the opponent at
                // all (no inputs to replay), so this purely interpolates between the discrete
                // server snapshots to hide the broadcast cadence.
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

                // Edge-triggered sound effects, driven by transitions in the authoritative
                // board. A piece that just locked moves Playing -> ResolvingMatches; we skip
                // it when it was our own acked hard drop (sound already played locally).
                if prev_state == GameState::Playing
                    && session.board.state == GameState::ResolvingMatches
                    && !hard_drop_acked
                {
                    crate::audio::play_lock();
                }
                // chain_count rising means another link resolved: pop sound + on-screen badge.
                let cc = session.board.chain_count;
                if cc > prev_chain {
                    crate::audio::play_pop(cc);
                    session.chain_display = Some((cc, 2.0));
                }
                // More pending garbage than last frame => the opponent just sent some.
                if session.board.pending_garbage > prev_garbage {
                    crate::audio::play_garbage();
                }
                // Rising edge of the all-clear flag: jingle + timed celebratory overlay.
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
