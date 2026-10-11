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
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::GameStart)));
}

#[test]
fn disconnect_reserves_seat_then_reconnect_restores_it() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);

    mgr.handle(Command::Unregister { conn: 2 });
    assert_eq!(mgr.rooms[&1].members.len(), 2, "seat must be kept during grace");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, None);

    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, Some(3u64), "reconnect should re-bind the same seat");
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

pub(super) fn drain_all(rxs: &mut [mpsc::Receiver<Vec<u8>>]) {
    for rx in rxs.iter_mut() {
        while rx.try_recv().is_ok() {}
    }
}

#[test]
#[ignore = "load test: run with --release -- --ignored --nocapture"]
fn load_many_rooms_tick_budget() {
    use std::time::{Duration, Instant};

    let rooms: usize = std::env::var("PUYO_LOAD_ROOMS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let ticks: usize = 300;
    let dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let budget = Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64);

    let mut mgr = new_mgr();
    let mut receivers = Vec::with_capacity(rooms * 2);
    for i in 0..rooms {
        let host = (2 * i + 1) as ConnId;
        let member = (2 * i + 2) as ConnId;
        receivers.push(reg(&mut mgr, host));
        hello(&mut mgr, host, &format!("h{i}"));
        mgr.handle(Command::CreateRoom {
            conn: host,
            name: "L".into(),
        });
        let id = (i + 1) as RoomId;
        receivers.push(reg(&mut mgr, member));
        hello(&mut mgr, member, &format!("m{i}"));
        mgr.handle(Command::JoinRoom { conn: member, id });
        mgr.handle(Command::ToggleCountdown { conn: host });
    }
    mgr.tick(3.5, false);
    let playing = mgr.rooms.values().filter(|r| matches!(r.phase, Phase::Playing)).count();
    assert_eq!(playing, rooms, "every room should be playing");
    drain_all(&mut receivers);

    let mut sum = Duration::ZERO;
    let mut max = Duration::ZERO;
    for _ in 0..ticks {
        let t0 = Instant::now();
        mgr.tick(dt, true);
        let e = t0.elapsed();
        sum += e;
        max = max.max(e);
        drain_all(&mut receivers);
    }

    let avg = sum / ticks as u32;
    let per_room_ns = avg.as_nanos() as f64 / rooms as f64;
    let peak_load = max.as_secs_f64() / budget.as_secs_f64() * 100.0;
    let rooms_at_budget = budget.as_nanos() as f64 / per_room_ns;
    println!("\nload: {rooms} rooms playing, {ticks} broadcast ticks");
    println!("tick  avg = {avg:?}   max = {max:?}   budget = {budget:?}");
    println!("per-room avg = {per_room_ns:.0} ns");
    println!("peak_load = {peak_load:.1}% of one 60Hz tick");
    println!("=> ~{rooms_at_budget:.0} rooms would fill one tick (single thread)\n");
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
