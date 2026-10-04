use super::*;

pub(super) fn hello_session(mgr: &mut Manager, conn: ConnId, token: &str, user: u128, session: u128) {
    mgr.handle(Command::Hello {
        conn,
        token: token.to_string(),
        user_id: Some(Uuid::from_u128(user)),
        username: Some(token.to_lowercase()),
        session: Some(Uuid::from_u128(session)),
        last_disconnect_reason: None,
    });
}

pub(super) fn queue_up(mgr: &mut Manager, conn: ConnId, user: u128, elo: i32, casual: i64) -> mpsc::Receiver<Vec<u8>> {
    let rx = reg(mgr, conn);
    hello_as(mgr, conn, &format!("P{conn}"), user);
    mgr.handle(Command::JoinQueue { conn });
    let checks = mgr.take_ranked_checks();
    assert_eq!(checks.len(), 1, "setup: one lookup per queue request");
    mgr.handle(Command::RankedCheckDone {
        check: checks[0],
        profile: Some(db::RankedProfile {
            elo,
            casual,
            avatar_url: Some(format!("/api/users/{user}/avatar?v=1")),
        }),
    });
    rx
}

pub(super) fn ranked_id(mgr: &Manager) -> Option<RoomId> {
    mgr.rooms.values().find(|r| r.series.is_some()).map(|r| r.id)
}

pub(super) fn accept_all(mgr: &mut Manager) {
    let conns: Vec<ConnId> = mgr
        .pending_matches
        .iter()
        .flat_map(|m| m.entries.iter().map(|e| e.conn))
        .collect();
    for conn in conns {
        mgr.handle(Command::AcceptMatch { conn });
    }
}

pub(super) fn match_up(mgr: &mut Manager) -> Option<RoomId> {
    mgr.tick(STEP, false);
    accept_all(mgr);
    ranked_id(mgr)
}

pub(super) fn ranked_pair(mgr: &mut Manager) -> (RoomId, mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let rx1 = queue_up(mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let rx2 = queue_up(mgr, 2, 2, 1050, config::RANKED_MIN_CASUAL);
    let id = match_up(mgr).expect("setup: the two players are matched");
    mgr.tick(3.5, false);
    assert!(mgr.rooms[&id].game_running(), "setup: the first game starts");
    (id, rx1, rx2)
}

pub(super) fn lose(mgr: &mut Manager, id: RoomId, slot: usize) {
    mgr.rooms.get_mut(&id).unwrap().sim.boards[slot].state = GameState::GameOver;
    mgr.tick(STEP, false);
}

#[test]
fn guests_cannot_queue() {
    let mut mgr = new_mgr();
    let mut rx = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::JoinQueue { conn: 1 });
    assert!(mgr.take_ranked_checks().is_empty());
    assert!(has(&drain(&mut rx), |m| matches!(
        m,
        ServerMessage::QueueRefused { .. }
    )));
}

#[test]
fn too_few_casual_games_keep_an_account_out_of_the_queue() {
    let mut mgr = new_mgr();
    let mut rx = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL - 2);
    assert!(mgr.queue.is_empty());
    let refused = drain(&mut rx).into_iter().find_map(|m| match m {
        ServerMessage::QueueRefused { reason } => Some(reason),
        _ => None,
    });
    assert_eq!(
        refused.as_deref(),
        Some("Jouez encore 2 parties amicales avant le classé.")
    );
}

#[test]
fn close_players_are_matched_into_a_series() {
    let mut mgr = new_mgr();
    let mut rx1 = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let _rx2 = queue_up(&mut mgr, 2, 2, 1050, config::RANKED_MIN_CASUAL);
    let id = match_up(&mut mgr).expect("matched");
    assert!(mgr.queue.is_empty());
    assert!(
        mgr.public_room_list().iter().all(|r| r.ranked),
        "a ranked room is listed, to be watched"
    );
    assert!(matches!(mgr.rooms[&id].phase, Phase::CountingDown(_)));
    let lobby = drain(&mut rx1).into_iter().find_map(|m| match m {
        ServerMessage::Lobby { info } => info.ranked,
        _ => None,
    });
    assert_eq!(
        lobby.map(|r| (r.opponent, r.opponent_elo, r.opponent_avatar)),
        Some(("p2".into(), 1050, Some("/api/users/2/avatar?v=1".into())))
    );
}

#[tokio::test(start_paused = true)]
async fn distant_players_wait_for_the_window_to_grow() {
    let mut mgr = new_mgr();
    let _rx1 = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let _rx2 = queue_up(&mut mgr, 2, 2, 1600, config::RANKED_MIN_CASUAL);
    assert!(match_up(&mut mgr).is_none(), "600 apart is too far at first");
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(match_up(&mut mgr).is_some(), "matched once the wait widened the window");
}

#[tokio::test(start_paused = true)]
async fn a_series_ends_at_three_wins_and_moves_elo_once() {
    let mut mgr = new_mgr();
    let (id, mut rx1, mut rx2) = ranked_pair(&mut mgr);
    for round in 1..=3u8 {
        lose(&mut mgr, id, 1);
        assert_eq!(mgr.rooms[&id].series.as_ref().unwrap().wins, [round, 0]);
        if round < 3 {
            assert!(mgr.take_unsaved_series().is_empty(), "no ELO before the end");
            tokio::time::advance(NEXT_GAME_DELAY).await;
            mgr.tick(STEP, false);
            assert!(mgr.rooms[&id].game_running(), "the next game starts by itself");
        }
    }
    assert_eq!(
        mgr.take_unsaved_series()
            .iter()
            .map(|r| (r.winner, r.loser))
            .collect::<Vec<_>>(),
        vec![(Uuid::from_u128(1), Uuid::from_u128(2))]
    );
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 3);
    assert!(recs.iter().all(|r| r.ranked));
    let over = |rx: &mut mpsc::Receiver<Vec<u8>>| {
        drain(rx).into_iter().find_map(|m| match m {
            ServerMessage::SeriesOver {
                winner_slot,
                elo_change,
            } => Some((winner_slot, elo_change)),
            _ => None,
        })
    };
    let (w1, gain) = over(&mut rx1).expect("winner told");
    let (w2, loss) = over(&mut rx2).expect("loser told");
    assert_eq!((w1, w2), (Some(1), Some(1)));
    assert!(gain > 0 && gain == -loss);
    tokio::time::advance(NEXT_GAME_DELAY).await;
    mgr.tick(STEP, false);
    assert!(!mgr.rooms[&id].game_running(), "nothing starts after the end");
}

#[test]
fn leaving_mid_series_loses_it() {
    let mut mgr = new_mgr();
    let (id, mut rx1, _rx2) = ranked_pair(&mut mgr);
    lose(&mut mgr, id, 0);
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert_eq!(
        mgr.take_unsaved_series()
            .iter()
            .map(|r| (r.winner, r.loser))
            .collect::<Vec<_>>(),
        vec![(Uuid::from_u128(1), Uuid::from_u128(2))]
    );
    assert!(has(&drain(&mut rx1), |m| matches!(
        m,
        ServerMessage::SeriesOver {
            winner_slot: Some(1),
            ..
        }
    )));
    assert!(
        mgr.rooms[&id].members.iter().all(|m| m.token != "P2"),
        "the leaver cannot come back into it"
    );
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(!mgr.rooms.contains_key(&id), "the room closes once both left");
}

#[test]
fn room_controls_do_nothing_in_a_series() {
    let mut mgr = new_mgr();
    let _rx1 = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let _rx2 = queue_up(&mut mgr, 2, 2, 1000, config::RANKED_MIN_CASUAL);
    let id = match_up(&mut mgr).unwrap();
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(
        matches!(mgr.rooms[&id].phase, Phase::CountingDown(_)),
        "the countdown cannot be stopped"
    );
    mgr.tick(3.5, false);
    mgr.handle(Command::TogglePause { conn: 1 });
    assert!(!mgr.rooms[&id].sim.paused);
    mgr.handle(Command::ReturnToLobby { conn: 1 });
    assert!(mgr.rooms[&id].game_running());
    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "C");
    mgr.handle(Command::JoinRoom { conn: 3, id });
    assert!(has(&drain(&mut rx3), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn an_account_cannot_queue_twice() {
    let mut mgr = new_mgr();
    let _rx1 = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let mut rx2 = reg(&mut mgr, 2);
    hello_as(&mut mgr, 2, "B", 1);
    mgr.handle(Command::JoinQueue { conn: 2 });
    assert!(mgr.take_ranked_checks().is_empty());
    assert!(has(&drain(&mut rx2), |m| matches!(
        m,
        ServerMessage::QueueRefused { .. }
    )));
    mgr.tick(STEP, false);
    assert!(ranked_id(&mgr).is_none());
}

#[test]
fn casual_games_leave_elo_alone() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    sim_of(&mut mgr).boards[1].state = GameState::GameOver;
    mgr.tick(STEP, false);
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert!(!recs[0].ranked);
    assert!(mgr.take_unsaved_series().is_empty());
}

#[test]
fn revoking_a_session_logs_out_the_other_connections_only() {
    let mut mgr = new_mgr();
    let mut rx1 = reg(&mut mgr, 1);
    hello_session(&mut mgr, 1, "A", 7, 100);
    let mut rx2 = reg(&mut mgr, 2);
    hello_session(&mut mgr, 2, "B", 7, 200);
    mgr.handle(Command::Revoke {
        user_id: Uuid::from_u128(7),
        keep: Some(Uuid::from_u128(100)),
    });
    assert!(!has(&drain(&mut rx1), |m| matches!(m, ServerMessage::SessionRevoked)));
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::SessionRevoked)));
    mgr.handle(Command::JoinQueue { conn: 2 });
    assert!(
        mgr.take_ranked_checks().is_empty(),
        "the revoked connection is a guest now"
    );
    mgr.handle(Command::JoinQueue { conn: 1 });
    assert_eq!(mgr.take_ranked_checks().len(), 1, "the kept one still is the account");
}

#[test]
fn an_oversized_player_id_is_ignored() {
    let mut mgr = new_mgr();
    let mut rx = reg(&mut mgr, 1);
    hello(&mut mgr, 1, &"x".repeat(65));
    assert!(mgr.conn_token.is_empty());
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_ranked_draw_scores_nobody_and_is_replayed() {
    let mut mgr = new_mgr();
    let (id, _rx1, _rx2) = ranked_pair(&mut mgr);
    for board in &mut mgr.rooms.get_mut(&id).unwrap().sim.boards {
        board.state = GameState::GameOver;
    }
    mgr.tick(STEP, false);
    assert_eq!(mgr.rooms[&id].series.as_ref().unwrap().wins, [0, 0]);
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(
        (recs[0].winner_slot, recs[0].ranked),
        (None, true),
        "recorded as a draw"
    );
    tokio::time::advance(NEXT_GAME_DELAY).await;
    mgr.tick(STEP, false);
    assert!(mgr.rooms[&id].game_running(), "the game is replayed");
}

#[test]
fn coming_back_after_a_finished_series_starts_afresh() {
    let mut mgr = new_mgr();
    let (id, _rx1, _rx2) = ranked_pair(&mut mgr);
    mgr.handle(Command::LeaveRoom { conn: 1 });
    mgr.handle(Command::Unregister { conn: 2 });
    let mut rx = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "P2", 2);
    assert_eq!(mgr.room_of(3), None, "not put back into the old series");
    assert!(!mgr.rooms.contains_key(&id), "the finished room is gone");
    assert!(has(&drain(&mut rx), |m| matches!(m, ServerMessage::RoomList { .. })));
}

#[test]
fn a_casual_game_where_both_top_out_together_is_a_draw() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    for board in &mut sim_of(&mut mgr).boards {
        board.state = GameState::GameOver;
    }
    mgr.tick(STEP, false);
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot, None);
}

#[test]
fn a_ranked_game_keeps_running_while_a_player_is_away() {
    let mut mgr = new_mgr();
    let (id, mut rx1, _rx2) = ranked_pair(&mut mgr);
    drain(&mut rx1);
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(!mgr.rooms[&id].sim.paused, "no pause for the one who stayed");
    let tick = mgr.rooms[&id].sim.tick;
    mgr.tick(STEP, false);
    assert!(mgr.rooms[&id].sim.tick > tick);

    let mut rx3 = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "P2", 2);
    assert_eq!(mgr.room_of(3), Some(id), "back in the series");
    let back = drain(&mut rx3);
    assert!(has(&back, |m| matches!(m, ServerMessage::GameStart)));
    assert!(has(&back, |m| matches!(m, ServerMessage::StateUpdate { .. })));
    let stayed = drain(&mut rx1);
    assert!(
        !has(&stayed, |m| matches!(
            m,
            ServerMessage::Lobby { .. } | ServerMessage::GameStart | ServerMessage::OpponentDisconnected
        )),
        "the one who stayed is not pulled out of the game"
    );
}

#[test]
fn a_pending_friend_check_does_not_swallow_a_queue_request() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    assert_eq!(mgr.take_friend_checks().len(), 1, "setup: a friend check is in flight");
    mgr.handle(Command::JoinQueue { conn: 2 });
    assert_eq!(mgr.take_ranked_checks().len(), 1);
}

#[test]
fn coming_back_through_the_queue_button_resumes_the_series() {
    let mut mgr = new_mgr();
    let (id, _rx1, _rx2) = ranked_pair(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    let mut rx = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "P2", 2);
    mgr.handle(Command::JoinQueue { conn: 3 });
    assert_eq!(mgr.room_of(3), Some(id));
    assert!(!has(&drain(&mut rx), |m| matches!(
        m,
        ServerMessage::QueueRefused { .. }
    )));
}

pub(super) fn cancelled(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<ServerMessage> {
    drain(rx)
        .into_iter()
        .filter(|m| {
            matches!(
                m,
                ServerMessage::MatchCancelled { .. } | ServerMessage::QueueCooldown { .. }
            )
        })
        .collect()
}

pub(super) fn found_pair(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let rx1 = queue_up(mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let rx2 = queue_up(mgr, 2, 2, 1050, config::RANKED_MIN_CASUAL);
    mgr.tick(STEP, false);
    assert_eq!(mgr.pending_matches.len(), 1, "setup: a match is proposed");
    (rx1, rx2)
}

#[test]
fn a_match_starts_only_once_both_players_accept() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = found_pair(&mut mgr);
    let found = drain(&mut rx1).into_iter().find_map(|m| match m {
        ServerMessage::MatchFound {
            opponent,
            opponent_elo,
            opponent_avatar,
            secs,
        } => Some((opponent, opponent_elo, opponent_avatar, secs)),
        _ => None,
    });
    assert_eq!(
        found,
        Some((
            "p2".into(),
            1050,
            Some("/api/users/2/avatar?v=1".into()),
            config::MATCH_ACCEPT_SECS
        ))
    );
    assert!(ranked_id(&mgr).is_none());
    mgr.handle(Command::AcceptMatch { conn: 1 });
    mgr.tick(STEP, false);
    assert!(ranked_id(&mgr).is_none(), "one acceptance is not enough");
    mgr.handle(Command::AcceptMatch { conn: 2 });
    let id = ranked_id(&mgr).expect("both accepted");
    assert!(matches!(mgr.rooms[&id].phase, Phase::CountingDown(_)));
    assert!(mgr.pending_matches.is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_match_requeues_who_accepted_and_holds_back_the_other() {
    let mut mgr = new_mgr();
    let (mut rx1, mut rx2) = found_pair(&mut mgr);
    mgr.handle(Command::AcceptMatch { conn: 1 });
    tokio::time::advance(ACCEPT_WINDOW).await;
    mgr.tick(STEP, false);
    assert!(mgr.pending_matches.is_empty());
    assert_eq!(mgr.queue.iter().map(|e| e.conn).collect::<Vec<_>>(), vec![1]);
    assert!(matches!(
        cancelled(&mut rx1)[..],
        [ServerMessage::MatchCancelled { requeued: true }]
    ));
    assert!(matches!(
        cancelled(&mut rx2)[..],
        [
            ServerMessage::MatchCancelled { requeued: false },
            ServerMessage::QueueCooldown {
                secs: config::QUEUE_COOLDOWN_SECS
            }
        ]
    ));

    mgr.handle(Command::JoinQueue { conn: 2 });
    assert!(mgr.take_ranked_checks().is_empty(), "held back for a minute");
    assert!(has(&drain(&mut rx2), |m| matches!(
        m,
        ServerMessage::QueueCooldown { .. }
    )));
    tokio::time::advance(QUEUE_COOLDOWN).await;
    mgr.tick(STEP, false);
    mgr.handle(Command::JoinQueue { conn: 2 });
    assert_eq!(mgr.take_ranked_checks().len(), 1, "free to queue again");
}

#[test]
fn declining_a_match_cancels_it_at_once() {
    let mut mgr = new_mgr();
    let (mut rx1, mut rx2) = found_pair(&mut mgr);
    mgr.handle(Command::LeaveQueue { conn: 2 });
    assert!(mgr.pending_matches.is_empty());
    assert_eq!(mgr.queue.iter().map(|e| e.conn).collect::<Vec<_>>(), vec![1]);
    assert!(matches!(
        cancelled(&mut rx1)[..],
        [ServerMessage::MatchCancelled { requeued: true }]
    ));
    assert!(has(&cancelled(&mut rx2), |m| matches!(
        m,
        ServerMessage::QueueCooldown { .. }
    )));
}

#[test]
fn disconnecting_from_a_proposed_match_declines_it() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = found_pair(&mut mgr);
    mgr.handle(Command::AcceptMatch { conn: 1 });
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(mgr.pending_matches.is_empty());
    assert_eq!(mgr.queue.iter().map(|e| e.conn).collect::<Vec<_>>(), vec![1]);
    assert!(matches!(
        cancelled(&mut rx1)[..],
        [ServerMessage::MatchCancelled { requeued: true }]
    ));
    assert!(ranked_id(&mgr).is_none());
}

#[test]
fn a_player_in_a_proposed_match_cannot_queue_again() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = found_pair(&mut mgr);
    let mut rx3 = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "C", 1);
    mgr.handle(Command::JoinQueue { conn: 3 });
    assert!(mgr.take_ranked_checks().is_empty());
    assert!(has(&drain(&mut rx3), |m| matches!(
        m,
        ServerMessage::QueueRefused { .. }
    )));
}

#[test]
fn joining_a_room_takes_a_player_out_of_the_ranked_queue() {
    let mut mgr = new_mgr();
    let _rx1 = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "r".into(),
    });
    assert!(mgr.queue.is_empty(), "a player in a room is not searching any more");
    let _rx2 = queue_up(&mut mgr, 2, 2, 1000, config::RANKED_MIN_CASUAL);
    mgr.tick(STEP, false);
    assert!(mgr.pending_matches.is_empty(), "no match proposed to someone in a room");
}

#[test]
fn leaving_for_a_room_while_matched_declines_the_match() {
    let mut mgr = new_mgr();
    let (mut rx1, mut rx2) = found_pair(&mut mgr);
    mgr.handle(Command::CreateRoom {
        conn: 2,
        name: "r".into(),
    });
    assert!(mgr.pending_matches.is_empty());
    assert_eq!(mgr.queue.iter().map(|e| e.conn).collect::<Vec<_>>(), vec![1]);
    assert!(matches!(
        cancelled(&mut rx1)[..],
        [ServerMessage::MatchCancelled { requeued: true }]
    ));
    assert!(has(&cancelled(&mut rx2), |m| matches!(
        m,
        ServerMessage::QueueCooldown { .. }
    )));
}
