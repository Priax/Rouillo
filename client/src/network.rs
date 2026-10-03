use shared::{config, Board, ClientMessage, IncomingGarbage, ServerMessage, StampedInput};

use crate::connection::ConnEvent;
use crate::state::{GameSession, Screen, State};
use crate::ui::Status;

pub fn handle_server_messages(state: &mut State) {
    let now = crate::connection::now_secs();
    for event in state.conn.poll(now) {
        match event {
            ConnEvent::Opened { recovered } => on_opened(state, recovered),
            ConnEvent::Message(msg) => process_message(state, *msg),
            ConnEvent::Retrying => {
                if state.screen.needs_connection() {
                    state.notice = Status::info("Connexion perdue, reconnexion…");
                } else {
                    state.conn.disconnect();
                    reset_to_menu(state, Status::error("Connexion au serveur perdue."));
                }
            }
            ConnEvent::GaveUp => reset_to_menu(state, Status::error("Connexion au serveur perdue.")),
            ConnEvent::Outdated => {
                state.outdated = true;
                reset_to_menu(state, Status::info(crate::update::NOTICE));
            }
        }
    }
}

fn on_opened(state: &mut State, recovered: bool) {
    let hello = ClientMessage::Hello {
        player_id: state.player_id.clone(),
        auth_token: state.auth.as_ref().map(|a| a.token.clone()),
        last_disconnect_reason: state.conn.take_unreported_drop(),
    };
    state.conn.send(&hello);
    state.maintenance = false;
    if let Some(id) = state.pending_join.take() {
        state.conn.send(&ClientMessage::JoinRoom { id });
    }
    if let Some(user_id) = state.pending_watch.take() {
        state.conn.send(&ClientMessage::WatchFriend { user_id });
    }
    if state.screen == Screen::Ranked && state.ranked.resume_search() {
        state.conn.send(&ClientMessage::JoinQueue);
    }
    if recovered {
        state.notice.clear();
    }
}

fn reset_to_menu(state: &mut State, notice: Status) {
    state.session = None;
    state.lobby = None;
    state.rooms.clear();
    state.screen = Screen::Menu;
    state.notice = notice;
}

fn lead_ticks(rtt_ms: Option<f32>) -> u32 {
    let rtt = rtt_ms.map_or(0, |ms| (ms / 1000.0 / config::CLIENT_SIM_DT).ceil() as u32);
    rtt.saturating_add(config::INPUT_LEAD_MARGIN_TICKS)
        .min(config::MAX_INPUT_LEAD_TICKS)
}

const DRIFT_WINDOW: u32 = 30;
const DRIFT_DEADBAND: i32 = 1;

fn sync_clock(session: &mut GameSession, server_tick: u32) {
    let target = server_tick.saturating_add(lead_ticks(session.ping_rtt_ms));
    let drift = session.local_tick as i64 - target as i64;
    if !session.synced || drift.unsigned_abs() > config::MAX_INPUT_LEAD_TICKS as u64 {
        session.local_tick = target;
        session.synced = true;
        session.clock_correction = 0;
        session.drift_min = None;
        session.drift_samples = 0;
        return;
    }
    let drift = drift as i32;
    let min = session.drift_min.map_or(drift, |m| m.min(drift));
    session.drift_min = Some(min);
    session.drift_samples += 1;
    if session.drift_samples >= DRIFT_WINDOW {
        session.clock_correction = if min.abs() > DRIFT_DEADBAND { -min } else { 0 };
        session.drift_min = None;
        session.drift_samples = 0;
    }
}

pub struct Update {
    pub board: Board,
    pub other_board: Board,
    pub ack: u32,
    pub tick: u32,
    pub incoming: Vec<IncomingGarbage>,
    pub opp_incoming: Vec<IncomingGarbage>,
}

pub fn reconcile(session: &mut GameSession, update: Update) {
    let prev_piece = session.predicted_board.active_piece.clone();
    let prev_piece_id = session.predicted_board.piece_id;
    let nuisance_before = session.my_nuisance();

    session.board = update.board;
    session.other_board = update.other_board;
    session.my_ack = update.ack;
    session.server_tick = update.tick;
    session.incoming = update.incoming;
    session.opp_incoming = update.opp_incoming;
    session.pending_inputs.retain(|(seq, _)| *seq > update.ack);

    if let Some(&(_, sent)) = session
        .sent_at
        .iter()
        .filter(|(seq, _)| *seq <= update.ack)
        .max_by_key(|(seq, _)| *seq)
    {
        session.last_rtt_ms = ((session.clock - sent) * 1000.0) as f32;
    }
    session.sent_at.retain(|(seq, _)| *seq > update.ack);

    sync_clock(session, update.tick);

    let pending: Vec<StampedInput> = session.pending_inputs.iter().map(|(_, input)| *input).collect();
    let predicted = session
        .board
        .replay(update.tick, session.local_tick, &pending, &session.incoming);

    if predicted.piece_id == prev_piece_id {
        if let (Some(prev), Some(cur)) = (&prev_piece, predicted.active_piece.as_ref()) {
            let off = &mut session.piece_visual_offset;
            off.0 = (off.0 + (prev.row - cur.row) as f32).clamp(-3.0, 3.0);
            off.1 = (off.1 + (prev.col - cur.col) as f32).clamp(-3.0, 3.0);
        }
    } else {
        session.piece_visual_offset = (0.0, 0.0);
    }

    session.predicted_board = predicted;
    crate::logic::announce_events(session);
    if session.my_nuisance() > nuisance_before {
        crate::audio::play_garbage();
    }

    if cfg!(debug_assertions) {
        session.last_server_msg = format!(
            "StateUpdate ack={} pending={} seq={}",
            update.ack,
            session.pending_inputs.len(),
            session.input_seq,
        );
    }
}

fn board_labels(info: &shared::LobbyInfo) -> Option<[String; 2]> {
    let name = |i: usize| info.names.get(i).cloned().unwrap_or_default();
    match info.your_slot {
        0 => Some([name(0), name(1)]),
        slot => Some(["YOU".to_owned(), name(2 - usize::from(slot))]),
    }
}

/// A spectator draws what the server sent: there are no inputs to predict.
fn watch(
    session: &mut GameSession,
    board: Board,
    other_board: Board,
    tick: u32,
    incoming: Vec<IncomingGarbage>,
    opp_incoming: Vec<IncomingGarbage>,
) {
    session.predicted_board = board.clone();
    session.board = board;
    session.other_board = other_board;
    session.server_tick = tick;
    session.local_tick = tick;
    session.incoming = incoming;
    session.opp_incoming = opp_incoming;
    crate::logic::announce_events(session);
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
            if state.lobby.as_ref().map(|l| l.id) != Some(info.id) {
                state.series_over = None;
                state.chat.clear();
            }
            state.screen = if info.ranked.is_some() {
                state.ranked.stop();
                Screen::Ranked
            } else {
                Screen::RoomLobby
            };
            state.lobby = Some(info);
            state.session = None;
        }
        ServerMessage::JoinFailed { reason } => {
            state.notice = Status::error(reason);
        }
        ServerMessage::GameStart => {
            let slot = state.lobby.as_ref().map_or(1, |l| l.your_slot);
            let mut session = GameSession::new(slot);
            session.labels = state.lobby.as_ref().and_then(board_labels);
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
            p1_incoming,
            p2_incoming,
        } => {
            if let Some(session) = state.session.as_mut() {
                let (mut board, mut other_board, ack, my_rng, opp_rng, incoming, opp_incoming) = match session.my_slot {
                    0 | 1 => (*p1_board, *p2_board, p1_ack, p1_rng, p2_rng, p1_incoming, p2_incoming),
                    2 => (*p2_board, *p1_board, p2_ack, p2_rng, p1_rng, p2_incoming, p1_incoming),
                    _ => return,
                };
                match my_rng {
                    Some(r) => board.set_rng(*r),
                    None => board.set_rng(session.board.rng_state()),
                }
                match opp_rng {
                    Some(r) => other_board.set_rng(*r),
                    None => other_board.set_rng(session.other_board.rng_state()),
                }
                session
                    .opponent_view
                    .push(tick, other_board.clone(), crate::connection::now_secs());
                if session.spectating() {
                    watch(session, board, other_board, tick, incoming, opp_incoming);
                    return;
                }
                reconcile(
                    session,
                    Update {
                        board,
                        other_board,
                        ack,
                        tick,
                        incoming,
                        opp_incoming,
                    },
                );
            }
        }
        ServerMessage::Restart => {
            if let Some(session) = state.session.as_mut() {
                let mut fresh = GameSession::new(session.my_slot);
                fresh.ping_rtt_ms = session.ping_rtt_ms;
                fresh.labels = session.labels.take();
                fresh.last_server_msg = "Restart".to_string();
                *session = fresh;
            }
        }
        ServerMessage::OpponentDisconnected => {
            if let Some(session) = state.session.as_mut() {
                session.opponent_disconnected = true;
                session.last_server_msg = "OpponentDisconnected".to_string();
            }
        }
        ServerMessage::QueueRefused { reason } => {
            state.ranked.stop();
            state.notice = Status::error(reason);
        }
        ServerMessage::MatchFound {
            opponent,
            opponent_elo,
            secs,
        } => {
            if state.ranked.match_found(opponent, opponent_elo, secs) {
                crate::audio::play_all_clear();
            }
        }
        ServerMessage::MatchCancelled { requeued } => {
            state.ranked.match_cancelled(requeued);
            state.notice = if requeued {
                Status::info("L'adversaire n'a pas accepté, retour dans la file.")
            } else {
                Status::error("Partie non acceptée.")
            };
        }
        ServerMessage::QueueCooldown { secs } => state.ranked.cooldown(secs),
        ServerMessage::SeriesScore { wins } => {
            if let Some(ranked) = state.lobby.as_mut().and_then(|l| l.ranked.as_mut()) {
                ranked.wins = wins;
            }
        }
        ServerMessage::SeriesOver {
            winner_slot,
            elo_change,
        } => {
            if state.series_over.is_none() {
                if let Some(auth) = state.auth.as_mut() {
                    auth.elo += elo_change;
                }
            }
            state.series_over = Some((winner_slot, elo_change));
        }
        ServerMessage::Chat { from, text, spectator } => state.chat.push(&from, &text, spectator),
        ServerMessage::SessionRevoked => {
            if state.auth.is_some() {
                crate::menu::clear_auth(state);
                if matches!(
                    state.screen,
                    Screen::Profile | Screen::Friends | Screen::OtherProfile | Screen::Ranked
                ) {
                    state.screen = Screen::Menu;
                }
                state.notice = Status::info("Votre session a été fermée depuis un autre appareil.");
            }
        }
        ServerMessage::Pong { .. } => {}
        ServerMessage::Maintenance => state.maintenance = true,
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
    use shared::{GameState, InputKind};

    use super::*;

    #[test]
    fn lead_without_a_measurement_is_just_the_margin() {
        assert_eq!(lead_ticks(None), config::INPUT_LEAD_MARGIN_TICKS);
        assert_eq!(lead_ticks(Some(0.0)), config::INPUT_LEAD_MARGIN_TICKS);
    }

    #[test]
    fn lead_covers_the_whole_round_trip() {
        let lead = lead_ticks(Some(200.0));
        assert_eq!(lead, 12 + config::INPUT_LEAD_MARGIN_TICKS);
        assert!(lead_ticks(Some(200.0)) > lead_ticks(Some(20.0)));
    }

    #[test]
    fn lead_is_capped() {
        assert_eq!(lead_ticks(Some(10_000.0)), config::MAX_INPUT_LEAD_TICKS);
        assert_eq!(lead_ticks(Some(f32::INFINITY)), config::MAX_INPUT_LEAD_TICKS);
    }

    #[test]
    fn a_nonsense_sample_falls_back_to_the_margin() {
        for bad in [f32::NAN, -1.0, f32::NEG_INFINITY] {
            assert_eq!(lead_ticks(Some(bad)), config::INPUT_LEAD_MARGIN_TICKS, "sample {bad}");
        }
    }

    fn session_with_rtt(rtt_ms: f32) -> GameSession {
        let mut session = GameSession::new(1);
        session.ping_rtt_ms = Some(rtt_ms);
        session
    }

    #[test]
    fn the_first_update_places_the_clock() {
        let mut session = session_with_rtt(100.0);
        sync_clock(&mut session, 500);
        assert!(session.synced);
        assert_eq!(session.local_tick, 500 + lead_ticks(Some(100.0)));
    }

    fn feed_window(session: &mut GameSession, drifts: impl Fn(u32) -> i32) {
        let lead = lead_ticks(session.ping_rtt_ms) as i64;
        for i in 0..DRIFT_WINDOW {
            let server_tick = session.local_tick as i64 - lead - drifts(i) as i64;
            sync_clock(session, server_tick as u32);
            session.local_tick += 1;
        }
    }

    #[test]
    fn a_clock_behind_is_pushed_forward_and_one_ahead_held_back() {
        let mut session = session_with_rtt(60.0);
        sync_clock(&mut session, 1_000);
        feed_window(&mut session, |_| -4);
        assert_eq!(session.clock_correction, 4);

        let mut session = session_with_rtt(60.0);
        sync_clock(&mut session, 1_000);
        feed_window(&mut session, |_| 4);
        assert_eq!(session.clock_correction, -4);
    }

    #[test]
    fn late_updates_do_not_pull_the_clock_back() {
        let mut session = session_with_rtt(60.0);
        sync_clock(&mut session, 1_000);
        feed_window(&mut session, |i| if i % 10 == 0 { 0 } else { (i % 7) as i32 });
        assert_eq!(session.clock_correction, 0);
    }

    #[test]
    fn a_small_drift_is_left_alone() {
        let mut session = session_with_rtt(60.0);
        sync_clock(&mut session, 1_000);
        feed_window(&mut session, |_| DRIFT_DEADBAND);
        assert_eq!(session.clock_correction, 0);
    }

    #[test]
    fn a_clock_far_off_is_placed_at_once() {
        let mut session = session_with_rtt(60.0);
        sync_clock(&mut session, 1_000);
        session.local_tick -= 200;
        sync_clock(&mut session, 1_000);
        assert_eq!(session.local_tick, 1_000 + lead_ticks(Some(60.0)));
    }

    #[test]
    fn updates_never_correct_a_board_that_was_predicted_right() {
        let latency = 6;
        let margin = config::INPUT_LEAD_MARGIN_TICKS;
        let mut server = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 77, 1, 5);
        server.spawn_piece();
        let mut history = vec![server.clone()];
        let mut queue: Vec<(u32, StampedInput)> = Vec::new();

        let mut session = session_with_rtt((2 * latency) as f32 * 1000.0 * config::CLIENT_SIM_DT - 0.5);
        assert_eq!(lead_ticks(session.ping_rtt_ms), 2 * latency + margin, "setup");
        session.board = server.clone();
        session.predicted_board = server.clone();
        session.synced = true;

        let script = [
            InputKind::MoveLeft,
            InputKind::RotateCW,
            InputKind::HardDrop,
            InputKind::MoveRight,
            InputKind::MoveRight,
            InputKind::SoftDropPress,
            InputKind::RotateCCW,
            InputKind::SoftDropRelease,
            InputKind::HardDrop,
        ];
        let mut corrections = 0;
        let mut server_tick = 0u32;
        for step in 0..1_200u32 {
            crate::logic::advance_one(&mut session);

            if step % 9 == 0 && session.predicted_board.state == GameState::Playing {
                let kind = script[(step / 9) as usize % script.len()];
                session.input_seq += 1;
                let input = StampedInput {
                    tick: session.local_tick + 1,
                    kind,
                };
                session.predicted_board.apply_input(kind);
                session.pending_inputs.push((session.input_seq, input));
                queue.push((session.input_seq, input));
            }

            while server_tick + latency + margin < session.local_tick {
                server_tick += 1;
                let due = queue
                    .iter()
                    .position(|(_, i)| i.tick > server_tick)
                    .unwrap_or(queue.len());
                assert!(
                    queue[..due].iter().all(|(_, i)| i.tick == server_tick),
                    "an input arrived late"
                );
                let applied: Vec<_> = queue.drain(..due).collect();
                server.step(applied.iter().map(|(_, i)| i.kind), 0);
                history.push(server.clone());
            }

            if step % 4 == 0 && server_tick >= latency {
                let at = server_tick - latency;
                let ack = acked_by(&session, at);
                let before = session.predicted_board.state_hash();
                let local = session.local_tick;
                reconcile(
                    &mut session,
                    Update {
                        board: history[at as usize].clone(),
                        other_board: history[at as usize].clone(),
                        ack,
                        tick: at,
                        incoming: Vec::new(),
                        opp_incoming: Vec::new(),
                    },
                );
                assert_eq!(session.local_tick, local, "the clock moved on a steady link");
                if session.predicted_board.state_hash() != before {
                    corrections += 1;
                }
            }
        }
        assert!(
            session.predicted_board.piece_id > 5,
            "the script placed too few pieces to prove much"
        );
        assert_eq!(
            corrections, 0,
            "{corrections} updates corrected a correctly predicted board"
        );
    }

    fn acked_by(session: &GameSession, at: u32) -> u32 {
        session
            .pending_inputs
            .iter()
            .filter(|(_, i)| i.tick <= at)
            .map(|(seq, _)| *seq)
            .fold(session.my_ack, u32::max)
    }
}
