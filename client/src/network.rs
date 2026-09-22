use shared::*;

use crate::connection::ConnEvent;
use crate::state::{GameSession, Screen, State};

pub fn handle_server_messages(state: &mut State) {
    let now = crate::connection::now_secs();
    for event in state.conn.poll(now) {
        match event {
            ConnEvent::Opened { recovered } => on_opened(state, recovered),
            ConnEvent::Message(msg) => process_message(state, *msg),
            ConnEvent::Retrying => {
                if !state.screen.needs_connection() {
                    state.conn.disconnect();
                    reset_to_menu(state, "Connexion au serveur perdue.");
                } else {
                    state.notice = "Connexion perdue — reconnexion…".to_string();
                }
            }
            ConnEvent::GaveUp => reset_to_menu(state, "Connexion au serveur perdue."),
        }
    }
}

fn on_opened(state: &mut State, recovered: bool) {
    let hello = ClientMessage::Hello {
        player_id: state.player_id.clone(),
        auth_token: state.auth.as_ref().map(|a| a.token.clone()),
        username: state.auth.as_ref().map(|a| a.username.clone()),
        last_disconnect_reason: state.conn.take_unreported_drop(),
    };
    state.conn.send(&hello);
    if let Some(id) = state.pending_join.take() {
        state.conn.send(&ClientMessage::JoinRoom { id });
    }
    if recovered {
        state.notice.clear();
    }
}

fn reset_to_menu(state: &mut State, notice: &str) {
    state.session = None;
    state.lobby = None;
    state.rooms.clear();
    state.screen = Screen::Menu;
    state.notice = notice.to_string();
}

pub fn input_tick(session: &GameSession) -> u32 {
    session
        .server_tick
        .saturating_add(lead_ticks(session.ping_rtt_ms))
        .saturating_add(session.ticks_since_update)
}

fn lead_ticks(rtt_ms: Option<f32>) -> u32 {
    let one_way = rtt_ms.map_or(0, |ms| (ms / 2.0 / 1000.0 / config::CLIENT_SIM_DT).ceil() as u32);
    one_way
        .saturating_add(config::INPUT_LEAD_MARGIN_TICKS)
        .min(config::MAX_INPUT_LEAD_TICKS)
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
            tick,
        } => {
            if let Some(session) = state.session.as_mut() {
                let (mut my_auth, mut opp_auth, my_ack, my_rng, opp_rng) = match session.my_slot {
                    1 => (*p1_board, *p2_board, p1_ack, p1_rng, p2_rng),
                    2 => (*p2_board, *p1_board, p2_ack, p2_rng, p1_rng),
                    _ => return,
                };

                match my_rng {
                    Some(r) => my_auth.set_rng(*r),
                    None => my_auth.set_rng(session.board.rng_state()),
                }
                match opp_rng {
                    Some(r) => opp_auth.set_rng(*r),
                    None => opp_auth.set_rng(session.other_board.rng_state()),
                }

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
                session.server_tick = tick;
                session.ticks_since_update = 0;
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
                session.sim_accumulator = 0.0;
                session.server_tick = 0;
                session.ticks_since_update = 0;
                session.last_server_msg = "Restart".to_string();
            }
        }
        ServerMessage::OpponentDisconnected => {
            if let Some(session) = state.session.as_mut() {
                session.opponent_disconnected = true;
                session.last_server_msg = "OpponentDisconnected".to_string();
            }
        }
        ServerMessage::Pong { .. } => {}
        ServerMessage::FriendInvitation {
            from_username,
            room_id,
            room_name,
        } => {
            state.pending_invitation = Some((from_username, room_id, room_name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lead_without_a_measurement_is_just_the_margin() {
        assert_eq!(lead_ticks(None), config::INPUT_LEAD_MARGIN_TICKS);
        assert_eq!(lead_ticks(Some(0.0)), config::INPUT_LEAD_MARGIN_TICKS);
    }

    #[test]
    fn lead_covers_half_the_round_trip() {
        let lead = lead_ticks(Some(200.0));
        assert!((7..=9).contains(&lead), "200 ms round trip gave a lead of {lead}");

        assert!(lead_ticks(Some(200.0)) > lead_ticks(Some(20.0)));
    }

    #[test]
    fn lead_is_capped() {
        assert_eq!(lead_ticks(Some(10_000.0)), config::MAX_INPUT_LEAD_TICKS);
        assert_eq!(lead_ticks(Some(f32::INFINITY)), config::MAX_INPUT_LEAD_TICKS);
    }

    fn session_at(server_tick: u32, rtt_ms: f32, since: u32) -> GameSession {
        let mut session = GameSession::new(1);
        session.server_tick = server_tick;
        session.ping_rtt_ms = Some(rtt_ms);
        session.ticks_since_update = since;
        session
    }

    #[test]
    fn the_stamp_is_pinned_to_the_last_update() {
        let stamp = |t, rtt, since| input_tick(&session_at(t, rtt, since));

        assert_eq!(stamp(1_000, 60.0, 0), stamp(1_000, 60.0, 0));
        assert!(stamp(1_100, 60.0, 0) > stamp(1_000, 60.0, 0));
        assert_eq!(stamp(1_000, 60.0, 3), stamp(1_000, 60.0, 0) + 3);
        assert!(stamp(1_000, 20.0, 0) < stamp(1_000, 200.0, 0));
    }

    #[test]
    fn a_nonsense_sample_falls_back_to_the_margin() {
        for bad in [f32::NAN, -1.0, f32::NEG_INFINITY] {
            assert_eq!(lead_ticks(Some(bad)), config::INPUT_LEAD_MARGIN_TICKS, "sample {bad}");
        }
    }
}
