use super::*;

#[test]
fn second_player_join_updates_both() {
    let mut mgr = new_mgr();
    let (_id, mut rx1, mut rx2) = two_player_room(&mut mgr);
    let joiner = last_lobby(&drain(&mut rx2)).expect("joiner Lobby");
    assert_eq!(joiner.players, 2);
    assert_eq!(joiner.your_slot, 2);
    assert!(!joiner.is_host);
    let host = last_lobby(&drain(&mut rx1)).expect("host updated Lobby");
    assert_eq!(host.players, 2);
}

#[test]
fn join_missing_room_fails() {
    let mut mgr = new_mgr();
    let mut rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::JoinRoom { conn: 1, id: 999 });
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn join_full_room_fails() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "C");
    mgr.handle(Command::JoinRoom { conn: 3, id: 1 });
    assert!(has(&drain(&mut rx3), |m| matches!(m, ServerMessage::JoinFailed { .. })));
    assert_eq!(mgr.rooms[&1].members.len(), 2);
}

#[test]
fn countdown_needs_two_players() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
}

#[test]
fn countdown_starts_game_after_delay() {
    let mut mgr = new_mgr();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::CountingDown(_)));
    mgr.tick(3.5, false);
    assert!(matches!(mgr.rooms[&1].phase, Phase::Playing));
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::GameStart { .. })));
}

#[test]
fn host_leaving_promotes_remaining_member() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::LeaveRoom { conn: 1 });
    let room = &mgr.rooms[&1];
    assert_eq!(room.members.len(), 1);
    assert_eq!(room.host.as_str(), "B");
}

#[test]
fn last_member_leaving_closes_room() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    assert!(mgr.rooms.contains_key(&1));
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(!mgr.rooms.contains_key(&1));
}

#[test]
fn opponent_disconnect_pauses_running_game() {
    let mut mgr = new_mgr();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    assert!(matches!(mgr.rooms[&1].phase, Phase::Playing));
    let _ = drain(&mut rx1); // clear GameStart

    mgr.handle(Command::Unregister { conn: 2 }); // opponent drops mid-game
    assert!(mgr.rooms[&1].sim.paused, "game should pause when opponent drops");
    assert!(has(&drain(&mut rx1), |m| matches!(
        m,
        ServerMessage::OpponentDisconnected
    )));
}

pub(super) fn running_game(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let (_id, rx1, rx2) = two_player_room(mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    assert!(mgr.rooms[&1].game_running(), "setup: game must be running");
    assert!(mgr.take_unsaved_matches().is_empty(), "setup: nothing decided yet");
    (rx1, rx2)
}

#[test]
fn countdown_refused_while_a_player_is_disconnected() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
}

#[test]
fn disconnect_during_countdown_cancels_it() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
    mgr.tick(3.5, false);
    assert!(
        matches!(mgr.rooms[&1].phase, Phase::Lobby),
        "must not start later either"
    );
}

#[test]
fn leaving_a_running_game_is_a_forfeit() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::TogglePause { conn: 2 });
    mgr.handle(Command::LeaveRoom { conn: 2 });
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot(), Some(1), "the player who stayed wins");
}

#[test]
fn creating_another_room_mid_game_is_a_forfeit() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::CreateRoom {
        conn: 2,
        name: "escape".into(),
    });
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot(), Some(1));
}

#[test]
fn host_returning_to_lobby_mid_game_is_a_forfeit() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::ReturnToLobby { conn: 1 });
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot(), Some(2), "the host abandoned, the guest wins");
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
}

#[test]
fn never_coming_back_is_a_forfeit() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(mgr.take_unsaved_matches().is_empty(), "a drop alone decides nothing");
    let b = mgr
        .rooms
        .get_mut(&1)
        .unwrap()
        .members
        .iter_mut()
        .find(|m| m.token == "B")
        .unwrap();
    b.disconnect_at = Some(Instant::now() - GRACE - Duration::from_secs(1));
    mgr.tick(STEP, false);
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot(), Some(1));
}

#[test]
fn leaving_while_opponent_is_disconnected_voids_the_game() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(mgr.take_unsaved_matches().is_empty());
}

#[test]
fn leaving_a_finished_game_records_nothing_more() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert!(mgr.take_unsaved_matches().is_empty());
}

#[test]
fn restart_refused_while_the_game_is_undecided() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Restart { conn: 2 });
    assert!(mgr.rooms[&1].sim.last_restart.is_none());

    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Restart { conn: 2 });
    assert!(mgr.rooms[&1].sim.last_restart.is_some(), "allowed once decided");
}

#[test]
fn room_with_only_disconnected_members_is_closed() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(!mgr.rooms.contains_key(&1));
    assert!(mgr.public_room_list().is_empty());
}

#[test]
fn reconnecting_to_a_closed_room_lands_on_the_room_list() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::LeaveRoom { conn: 1 });
    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    assert_eq!(mgr.room_of(3), None);
    assert!(has(&drain(&mut rx3), |m| matches!(m, ServerMessage::RoomList { .. })));
}

#[test]
fn joining_your_own_room_keeps_it() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    mgr.handle(Command::JoinRoom { conn: 1, id: 1 });
    assert!(mgr.rooms.contains_key(&1));
    assert_eq!(mgr.room_of(1), Some(1));
}

pub(super) fn set_pause_policy(mgr: &mut Manager, policy: PausePolicy) {
    for _ in 0..3 {
        if mgr.rooms[&1].settings.pause == policy {
            return;
        }
        mgr.handle(Command::SetSetting {
            conn: 1,
            index: 5,
            dir: 1,
        });
    }
    assert_eq!(
        mgr.rooms[&1].settings.pause, policy,
        "host could not set the pause policy"
    );
}

#[test]
fn pause_policy_everyone() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::TogglePause { conn: 2 });
    assert!(mgr.rooms[&1].sim.paused);
}

#[test]
fn pause_policy_host_only() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    set_pause_policy(&mut mgr, PausePolicy::HostOnly);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);

    mgr.handle(Command::TogglePause { conn: 2 });
    assert!(!mgr.rooms[&1].sim.paused, "guest may not pause");
    mgr.handle(Command::TogglePause { conn: 1 });
    assert!(mgr.rooms[&1].sim.paused, "host may");
}

#[test]
fn pause_policy_nobody() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    set_pause_policy(&mut mgr, PausePolicy::Nobody);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);

    mgr.handle(Command::TogglePause { conn: 1 });
    mgr.handle(Command::TogglePause { conn: 2 });
    assert!(!mgr.rooms[&1].sim.paused);
}

#[test]
fn pause_policy_is_host_and_lobby_only() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::SetSetting {
        conn: 2,
        index: 5,
        dir: 1,
    });
    assert_eq!(
        mgr.rooms[&1].settings.pause,
        PausePolicy::Everyone,
        "guest cannot change it"
    );

    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    mgr.handle(Command::SetSetting {
        conn: 1,
        index: 5,
        dir: 1,
    });
    assert_eq!(mgr.rooms[&1].settings.pause, PausePolicy::Everyone, "not mid-game");
}

#[test]
fn the_host_sets_the_board_size_the_match_is_played_on() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    for (index, dir) in [(2, 1), (2, 1), (3, -1)] {
        mgr.handle(Command::SetSetting { conn: 1, index, dir });
    }
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    let board = &mgr.rooms[&1].sim.boards[0];
    assert_eq!((board.width, board.height), (8, 12));
}

#[test]
fn opponent_cannot_lift_a_disconnect_pause() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(mgr.rooms[&1].sim.paused);
    mgr.handle(Command::TogglePause { conn: 1 });
    assert!(mgr.rooms[&1].sim.paused);
}

#[test]
fn lobby_reports_disconnected_seats() {
    let mut mgr = new_mgr();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    let info = last_lobby(&drain(&mut rx1)).unwrap();
    assert_eq!((info.players, info.connected), (2, 2));

    mgr.handle(Command::Unregister { conn: 2 });
    let info = last_lobby(&drain(&mut rx1)).expect("host must be told");
    assert_eq!((info.players, info.connected), (2, 1));

    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    let info = last_lobby(&drain(&mut rx1)).expect("and told again on return");
    assert_eq!((info.players, info.connected), (2, 2));
}

#[test]
fn restart_refused_while_opponent_is_disconnected() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::Restart { conn: 1 });
    assert!(!mgr.rooms[&1].game_running());
    assert!(mgr.rooms[&1].sim.last_restart.is_none());
}

/// A room for up to `max` players, with `players` of them seated (conns 1..).
fn big_room(mgr: &mut Manager, max: u8, players: usize) -> Vec<mpsc::Receiver<Vec<u8>>> {
    let mut rxs = vec![reg(mgr, 1)];
    hello(mgr, 1, "P1");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "big".into(),
    });
    for _ in 2..max {
        mgr.handle(Command::SetSetting {
            conn: 1,
            index: 6,
            dir: 1,
        });
    }
    for conn in 2..=players as ConnId {
        rxs.push(reg(mgr, conn));
        hello(mgr, conn, &format!("P{conn}"));
        mgr.handle(Command::JoinRoom { conn, id: 1 });
    }
    rxs
}

fn start(mgr: &mut Manager) {
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
}

#[test]
fn a_room_seats_as_many_players_as_its_setting() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 3, 3);
    assert_eq!(mgr.rooms[&1].members.len(), 3);
    let _rx = reg(&mut mgr, 9);
    hello(&mut mgr, 9, "late");
    mgr.handle(Command::JoinRoom { conn: 9, id: 1 });
    assert_eq!(mgr.rooms[&1].members.len(), 3, "the fourth is refused");
    start(&mut mgr);
    assert_eq!(mgr.rooms[&1].sim.boards.len(), 3);
}

#[test]
fn one_team_alone_cannot_start() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 3, 3);
    for conn in 1..=3 {
        mgr.handle(Command::SetTeam { conn, team: Some(2) });
    }
    start(&mut mgr);
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby), "nobody to play against");
    mgr.handle(Command::SetTeam { conn: 3, team: None });
    start(&mut mgr);
    assert!(mgr.rooms[&1].game_running());
    assert_eq!(mgr.rooms[&1].sim.teams[0], mgr.rooms[&1].sim.teams[1]);
    assert_ne!(mgr.rooms[&1].sim.teams[0], mgr.rooms[&1].sim.teams[2]);
}

#[test]
fn a_team_is_picked_in_the_lobby_only_and_must_exist() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 3, 3);
    mgr.handle(Command::SetTeam {
        conn: 2,
        team: Some(shared::MAX_TEAMS + 1),
    });
    assert_eq!(mgr.rooms[&1].members[1].team, None);
    mgr.handle(Command::SetTeam { conn: 2, team: Some(1) });
    assert_eq!(mgr.rooms[&1].members[1].team, Some(1));
    start(&mut mgr);
    mgr.handle(Command::SetTeam { conn: 2, team: None });
    assert_eq!(mgr.rooms[&1].members[1].team, Some(1), "not during a game");
}

#[test]
fn the_seats_never_drop_below_who_is_seated() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 4, 3);
    for _ in 0..5 {
        mgr.handle(Command::SetSetting {
            conn: 1,
            index: 6,
            dir: -1,
        });
    }
    assert_eq!(mgr.rooms[&1].settings.max_players, 3);
}

#[test]
fn a_player_leaving_mid_game_is_out_and_the_others_play_on() {
    let mut mgr = new_mgr();
    let mut rxs = big_room(&mut mgr, 3, 3);
    start(&mut mgr);
    for rx in &mut rxs {
        drain(rx);
    }
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert!(mgr.rooms[&1].game_running(), "two players are still in");
    assert!(!mgr.rooms[&1].sim.alive(1));
    assert!(mgr.take_unsaved_matches().is_empty());
    for i in [0, 2] {
        assert!(
            last_lobby(&drain(&mut rxs[i])).is_none(),
            "nobody is sent back to the lobby"
        );
    }
    mgr.handle(Command::LeaveRoom { conn: 3 });
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot(), Some(1));
    assert_eq!(recs[0].players.len(), 3);
}

#[test]
fn a_late_comer_watches_then_plays_the_next_game() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 3, 2);
    start(&mut mgr);
    let mut late = reg(&mut mgr, 9);
    hello(&mut mgr, 9, "late");
    mgr.handle(Command::JoinRoom { conn: 9, id: 1 });
    let info = last_lobby(&drain(&mut late)).expect("a lobby");
    assert_eq!(info.your_slot, 0, "watching the running game");
    let before = mgr.rooms[&1].sim.queued_inputs.iter().map(Vec::len).sum::<usize>();
    let now = mgr.rooms[&1].sim.tick;
    super::game::press(&mut mgr, 9, 1, now);
    let after = mgr.rooms[&1].sim.queued_inputs.iter().map(Vec::len).sum::<usize>();
    assert_eq!(before, after, "no board to play");

    mgr.handle(Command::LeaveRoom { conn: 2 });
    mgr.handle(Command::ReturnToLobby { conn: 1 });
    start(&mut mgr);
    assert_eq!(mgr.rooms[&1].sim.boards.len(), 2, "host and late comer");
    assert_eq!(mgr.rooms[&1].members[1].game_slot, Some(1));
}

/// The slot and the number of names of the last game start.
fn game_start(msgs: &[ServerMessage]) -> Option<(u8, usize)> {
    msgs.iter().rev().find_map(|m| match m {
        ServerMessage::GameStart { your_slot, names, .. } => Some((*your_slot, names.len())),
        _ => None,
    })
}

#[test]
fn a_restart_after_a_leave_tells_each_player_their_new_slot() {
    let mut mgr = new_mgr();
    let mut rxs = big_room(&mut mgr, 3, 3);
    start(&mut mgr);
    for (i, rx) in rxs.iter_mut().enumerate() {
        assert_eq!(game_start(&drain(rx)), Some((slot_byte(i + 1), 3)));
    }
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::LeaveRoom { conn: 2 });
    mgr.handle(Command::Restart { conn: 3 });
    assert_eq!(game_start(&drain(&mut rxs[0])), Some((1, 2)));
    assert_eq!(game_start(&drain(&mut rxs[2])), Some((2, 2)));
}

#[test]
fn a_manual_target_is_a_slot_from_one() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 3, 3);
    start(&mut mgr);
    mgr.handle(Command::SetTargeting {
        conn: 1,
        targeting: shared::Targeting::Manual(3),
    });
    mgr.tick(0.02, true);
    assert_eq!(mgr.rooms[&1].sim.targets[0], Some(2));
}

fn update_slots(msgs: &[ServerMessage]) -> Vec<Vec<u8>> {
    msgs.iter()
        .filter_map(|m| match m {
            ServerMessage::StateUpdate { players, .. } => Some(players.iter().map(|p| p.slot).collect()),
            _ => None,
        })
        .collect()
}

#[test]
fn with_three_boards_the_others_come_one_tick_in_three() {
    let mut mgr = new_mgr();
    let mut rxs = big_room(&mut mgr, 3, 3);
    start(&mut mgr);
    let mut watcher = reg(&mut mgr, 8);
    hello(&mut mgr, 8, "W");
    mgr.handle(Command::Spectate { conn: 8, id: 1 });
    for rx in &mut rxs {
        drain(rx);
    }
    drain(&mut watcher);
    for _ in 0..6 {
        mgr.tick(1.0 / 60.0, true);
    }
    let seen = update_slots(&drain(&mut rxs[1]));
    assert_eq!(seen.len(), 6);
    assert_eq!(
        seen.iter().filter(|s| s.as_slice() == [2]).count(),
        4,
        "own board alone"
    );
    assert_eq!(seen.iter().filter(|s| s.as_slice() == [1, 2, 3]).count(), 2);
    let watched = update_slots(&drain(&mut watcher));
    assert!(
        watched.len() == 6 && watched.iter().all(|s| s.len() == 3),
        "a spectator sees every board"
    );
}

#[test]
fn a_duel_sends_both_boards_every_tick() {
    let mut mgr = new_mgr();
    let (_rx1, mut rx2) = running_game(&mut mgr);
    drain(&mut rx2);
    for _ in 0..4 {
        mgr.tick(1.0 / 60.0, true);
    }
    let seen = update_slots(&drain(&mut rx2));
    assert!(seen.len() == 4 && seen.iter().all(|s| s.len() == 2));
}

fn games_seen(msgs: &[ServerMessage]) -> (Vec<u32>, Vec<u32>) {
    let starts = msgs
        .iter()
        .filter_map(|m| match m {
            ServerMessage::GameStart { game, .. } => Some(*game),
            _ => None,
        })
        .collect();
    let mut states: Vec<u32> = msgs
        .iter()
        .filter_map(|m| match m {
            ServerMessage::StateUpdate { game, .. } => Some(*game),
            _ => None,
        })
        .collect();
    states.dedup();
    (starts, states)
}

#[test]
fn each_game_of_a_room_has_its_number() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = running_game(&mut mgr);
    mgr.tick(1.0 / 60.0, true);
    let (_, first) = games_seen(&drain(&mut rx1));
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Restart { conn: 2 });
    mgr.tick(1.0 / 60.0, true);
    let (starts, states) = games_seen(&drain(&mut rx1));
    assert_eq!(first.len(), 1);
    assert_eq!(starts, [first[0] + 1]);
    assert_eq!(states, starts, "the new game's updates carry its number");
}

fn datagram_slots(boards: &[bytes::Bytes]) -> Vec<Vec<u8>> {
    boards
        .iter()
        .map(|b| match shared::decode(b) {
            Some(ServerMessage::StateUpdate { players, .. }) => players.iter().map(|p| p.slot).collect(),
            _ => panic!("not a state"),
        })
        .collect()
}

#[test]
fn a_datagram_link_gets_its_own_board_alone_and_the_others_packed() {
    let mut mgr = new_mgr();
    let (tx, _rx) = mpsc::channel(CLIENT_CHAN_CAP);
    let (states_tx, mut states) = mpsc::channel(64);
    mgr.handle(Command::Register {
        conn: 9,
        sender: tx,
        states: Some(states_tx),
    });
    let _rxs = big_room(&mut mgr, 3, 3);
    start(&mut mgr);
    hello(&mut mgr, 9, "W");
    mgr.handle(Command::Spectate { conn: 9, id: 1 });
    let mut player = mgr.rooms[&1].members[1].conn.expect("seated");
    let (ptx, _prx) = mpsc::channel(CLIENT_CHAN_CAP);
    let (pstates_tx, mut pstates) = mpsc::channel(64);
    mgr.handle(Command::Unregister { conn: player });
    player = 10;
    mgr.handle(Command::Register {
        conn: player,
        sender: ptx,
        states: Some(pstates_tx),
    });
    hello(&mut mgr, player, "P2");
    for _ in 0..3 {
        mgr.tick(1.0 / 60.0, true);
    }
    let watched: Vec<_> = std::iter::from_fn(|| states.try_recv().ok())
        .map(|b| datagram_slots(&b))
        .collect();
    assert_eq!(watched.len(), 3);
    assert!(watched.iter().all(|t| t == &[vec![1, 2, 3]]), "{watched:?}");
    let played: Vec<_> = std::iter::from_fn(|| pstates.try_recv().ok())
        .map(|b| datagram_slots(&b))
        .collect();
    assert_eq!(
        played.iter().filter(|t| t.as_slice() == [vec![2]]).count(),
        2,
        "{played:?}"
    );
    assert_eq!(played.iter().filter(|t| t.as_slice() == [vec![1, 2, 3]]).count(), 1);
}

#[test]
fn a_big_room_is_packed_into_datagrams_that_fit() {
    let mut mgr = new_mgr();
    let players = 99;
    let _rxs = big_room(&mut mgr, 2, 1);
    mgr.rooms.get_mut(&1).unwrap().settings.max_players = players;
    let _rxs: Vec<_> = (2..=ConnId::from(players))
        .map(|conn| {
            let rx = reg(&mut mgr, conn);
            hello(&mut mgr, conn, &format!("P{conn}"));
            mgr.handle(Command::JoinRoom { conn, id: 1 });
            rx
        })
        .collect();
    assert_eq!(mgr.rooms[&1].members.len(), usize::from(players), "setup");
    start(&mut mgr);
    let colours = [
        PuyoType::Red,
        PuyoType::Green,
        PuyoType::Blue,
        PuyoType::Yellow,
        PuyoType::Purple,
    ];
    let mut noise = 7u32;
    for board in &mut sim_of(&mut mgr).boards {
        let h = board.height;
        for cell in board.cells[h - 10..].iter_mut().flatten() {
            noise = noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *cell = Some(colours[(noise >> 16) as usize % colours.len()]);
        }
    }
    let (tx, _rx) = mpsc::channel(CLIENT_CHAN_CAP);
    let (states_tx, mut states) = mpsc::channel(64);
    mgr.handle(Command::Register {
        conn: 500,
        sender: tx,
        states: Some(states_tx),
    });
    hello(&mut mgr, 500, "W");
    mgr.handle(Command::Spectate { conn: 500, id: 1 });
    mgr.tick(1.0 / 60.0, true);
    let datagrams = states.try_recv().expect("a state");
    assert!(datagrams.len() > 1, "99 boards in one datagram");
    assert!(datagrams.iter().all(|d| d.len() <= DATAGRAM_BUDGET));
    let slots: Vec<u8> = datagram_slots(&datagrams).concat();
    assert_eq!(slots, (1..=players).collect::<Vec<_>>());
}

#[test]
fn a_new_game_aims_at_random_as_the_client_shows() {
    let mut mgr = new_mgr();
    let _rxs = big_room(&mut mgr, 3, 3);
    start(&mut mgr);
    mgr.handle(Command::SetTargeting {
        conn: 1,
        targeting: shared::Targeting::Danger,
    });
    assert_eq!(mgr.rooms[&1].sim.targeting[0], shared::Targeting::Danger);
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Restart { conn: 2 });
    assert_eq!(mgr.rooms[&1].sim.targeting[0], shared::Targeting::Random);
}
