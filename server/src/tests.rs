use shared::{PausePolicy, PuyoType};

use super::*;

fn new_mgr() -> Manager {
    Manager::new()
}

fn reg(mgr: &mut Manager, conn: ConnId) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(CLIENT_CHAN_CAP);
    mgr.handle(Command::Register { conn, sender: tx });
    rx
}

fn hello(mgr: &mut Manager, conn: ConnId, token: &str) {
    mgr.handle(Command::Hello {
        last_disconnect_reason: None,
        conn,
        token: token.to_string(),
        user_id: None,
        username: None,
    });
}

fn drain(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<ServerMessage> {
    let mut out = Vec::new();
    while let Ok(bytes) = rx.try_recv() {
        if let Some(msg) = shared::decode::<ServerMessage>(&bytes) {
            out.push(msg);
        }
    }
    out
}

fn last_lobby(msgs: &[ServerMessage]) -> Option<LobbyInfo> {
    msgs.iter().rev().find_map(|m| match m {
        ServerMessage::Lobby { info } => Some(info.clone()),
        _ => None,
    })
}

fn has(msgs: &[ServerMessage], f: impl Fn(&ServerMessage) -> bool) -> bool {
    msgs.iter().any(f)
}

fn two_player_room(mgr: &mut Manager) -> (RoomId, mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let rx1 = reg(mgr, 1);
    hello(mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let rx2 = reg(mgr, 2);
    hello(mgr, 2, "B");
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    (1, rx1, rx2)
}

#[test]
fn create_room_lobbies_host() {
    let mut mgr = new_mgr();
    let mut rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "Room".into(),
    });
    let info = last_lobby(&drain(&mut rx1)).expect("host should get a Lobby");
    assert_eq!(info.players, 1);
    assert_eq!(info.your_slot, 1);
    assert!(info.is_host);
    assert!(mgr.rooms.contains_key(&1));
}

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

fn drain_all(rxs: &mut [mpsc::Receiver<Vec<u8>>]) {
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

fn running_game(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
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
    assert_eq!(recs[0].winner_slot, 1, "the player who stayed wins");
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
    assert_eq!(recs[0].winner_slot, 1);
}

#[test]
fn host_returning_to_lobby_mid_game_is_a_forfeit() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::ReturnToLobby { conn: 1 });
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot, 2, "the host abandoned, the guest wins");
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
    assert_eq!(recs[0].winner_slot, 1);
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

fn set_pause_policy(mgr: &mut Manager, policy: PausePolicy) {
    for _ in 0..3 {
        if mgr.rooms[&1].settings.pause == policy {
            return;
        }
        mgr.handle(Command::SetSetting {
            conn: 1,
            index: 3,
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
        index: 3,
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
        index: 3,
        dir: 1,
    });
    assert_eq!(mgr.rooms[&1].settings.pause, PausePolicy::Everyone, "not mid-game");
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

fn hello_as(mgr: &mut Manager, conn: ConnId, token: &str, user: u128) {
    mgr.handle(Command::Hello {
        conn,
        token: token.to_string(),
        user_id: Some(Uuid::from_u128(user)),
        username: Some(token.to_lowercase()),
        last_disconnect_reason: None,
    });
}

fn friends_only_room(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let rx1 = reg(mgr, 1);
    hello_as(mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    mgr.handle(Command::SetSetting {
        conn: 1,
        index: 2,
        dir: 1,
    });
    assert!(mgr.rooms[&1].settings.friends_only, "setup");
    let rx2 = reg(mgr, 2);
    hello_as(mgr, 2, "J", 2);
    (rx1, rx2)
}

fn only_join_check(mgr: &mut Manager) -> FriendCheck {
    let checks = mgr.take_friend_checks();
    assert_eq!(checks.len(), 1, "exactly one lookup expected");
    assert!(matches!(checks[0], FriendCheck::Join { .. }));
    checks[0].clone()
}

#[test]
fn friends_only_join_is_queued_not_awaited() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    assert_eq!(mgr.room_of(2), None, "not joined before the answer");
    let check = only_join_check(&mut mgr);
    assert_eq!(check.users(), (Uuid::from_u128(2), Uuid::from_u128(1)));
}

#[test]
fn friends_only_join_completes_for_a_friend() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.room_of(2), Some(1));
}

#[test]
fn friends_only_join_refused_for_a_stranger() {
    let mut mgr = new_mgr();
    let (_rx1, mut rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    mgr.handle(Command::FriendCheckDone { check, friends: false });
    assert_eq!(mgr.room_of(2), None);
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn guest_cannot_join_friends_only_room() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "G"); // no account
    mgr.handle(Command::JoinRoom { conn: 3, id: 1 });
    assert!(mgr.take_friend_checks().is_empty(), "no lookup for a guest");
    assert!(has(&drain(&mut rx3), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn one_lookup_in_flight_per_connection() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    for _ in 0..20 {
        mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    }
    let check = only_join_check(&mut mgr);
    mgr.handle(Command::FriendCheckDone { check, friends: false });
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    assert_eq!(
        mgr.take_friend_checks().len(),
        1,
        "a new lookup once the last one answered"
    );
}

#[test]
fn stale_join_answer_is_ignored_if_the_player_moved() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    mgr.handle(Command::CreateRoom {
        conn: 2,
        name: "mine".into(),
    });
    let mine = mgr.room_of(2).expect("in own room");
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.room_of(2), Some(mine));
}

#[test]
fn join_answer_for_a_disconnected_player_is_ignored() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(!mgr.checks_in_flight.contains(&2), "cleaned up on disconnect");
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.rooms[&1].members.len(), 1);
}

#[test]
fn join_answer_after_the_room_filled_is_refused() {
    let mut mgr = new_mgr();
    let (_rx1, mut rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    mgr.rooms.get_mut(&1).unwrap().members.push(Member {
        token: "K".into(),
        conn: Some(7),
        disconnect_at: None,
        user_id: Some(Uuid::from_u128(7)),
    });
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.rooms[&1].members.len(), 2);
    assert_eq!(mgr.room_of(2), None);
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn invite_bookkeeping_is_cleaned_up_on_disconnect() {
    let mut mgr = new_mgr();
    let _rx2 = inviter_and_target(&mut mgr);
    invite_b(&mut mgr);
    mgr.handle(Command::Unregister { conn: 1 });
    assert!(!mgr.last_invite.contains_key(&1));
    assert!(!mgr.checks_in_flight.contains(&1));
}

#[test]
fn join_is_rechecked_if_the_host_changed() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    {
        let room = mgr.rooms.get_mut(&1).unwrap();
        room.members[0].token = "C".into();
        room.members[0].user_id = Some(Uuid::from_u128(3));
        room.host = "C".into();
    }
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.room_of(2), None, "must not join on an answer about A");
    let again = only_join_check(&mut mgr);
    assert_eq!(again.users(), (Uuid::from_u128(2), Uuid::from_u128(3)));
}

fn inviter_and_target(mgr: &mut Manager) -> mpsc::Receiver<Vec<u8>> {
    let _ = reg(mgr, 1);
    hello_as(mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let mut rx2 = reg(mgr, 2);
    hello_as(mgr, 2, "B", 2);
    let _ = drain(&mut rx2);
    rx2
}

fn invite_b(mgr: &mut Manager) {
    mgr.handle(Command::InviteFriend {
        conn: 1,
        target_user_id: Uuid::from_u128(2).to_string(),
    });
}

fn invitations(rx: &mut mpsc::Receiver<Vec<u8>>) -> usize {
    drain(rx)
        .iter()
        .filter(|m| matches!(m, ServerMessage::FriendInvitation { .. }))
        .count()
}

#[test]
fn invitation_requires_friendship() {
    let mut mgr = new_mgr();
    let mut rx2 = inviter_and_target(&mut mgr);
    invite_b(&mut mgr);
    assert_eq!(invitations(&mut rx2), 0, "nothing before the answer");
    let checks = mgr.take_friend_checks();
    assert_eq!(checks.len(), 1);
    mgr.handle(Command::FriendCheckDone {
        check: checks[0].clone(),
        friends: false,
    });
    assert_eq!(invitations(&mut rx2), 0, "never to a stranger");
}

#[test]
fn invitation_reaches_a_friend() {
    let mut mgr = new_mgr();
    let mut rx2 = inviter_and_target(&mut mgr);
    invite_b(&mut mgr);
    let check = mgr.take_friend_checks().remove(0);
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(invitations(&mut rx2), 1);
}

#[test]
fn invitations_are_rate_limited() {
    let mut mgr = new_mgr();
    let mut rx2 = inviter_and_target(&mut mgr);
    for _ in 0..50 {
        invite_b(&mut mgr);
    }
    let checks = mgr.take_friend_checks();
    assert_eq!(checks.len(), 1, "one lookup in flight");
    mgr.handle(Command::FriendCheckDone {
        check: checks[0].clone(),
        friends: true,
    });
    for _ in 0..50 {
        invite_b(&mut mgr);
    }
    assert!(mgr.take_friend_checks().is_empty(), "cooldown holds right after");
    assert_eq!(invitations(&mut rx2), 1);

    *mgr.last_invite.get_mut(&1).unwrap() -= INVITE_COOLDOWN;
    invite_b(&mut mgr);
    assert_eq!(mgr.take_friend_checks().len(), 1, "allowed again after the cooldown");
}

#[test]
fn guests_cannot_invite() {
    let mut mgr = new_mgr();
    let _ = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let _ = reg(&mut mgr, 2);
    hello_as(&mut mgr, 2, "B", 2);
    invite_b(&mut mgr);
    assert!(mgr.take_friend_checks().is_empty());
}

#[test]
fn inviting_an_offline_user_costs_no_lookup() {
    let mut mgr = new_mgr();
    let _ = inviter_and_target(&mut mgr);
    mgr.handle(Command::InviteFriend {
        conn: 1,
        target_user_id: Uuid::from_u128(77).to_string(),
    });
    assert!(mgr.take_friend_checks().is_empty());
}

#[test]
fn invitation_dropped_if_inviter_left_the_room() {
    let mut mgr = new_mgr();
    let mut rx2 = inviter_and_target(&mut mgr);
    let _rx3 = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "C", 3);
    mgr.handle(Command::JoinRoom { conn: 3, id: 1 });
    invite_b(&mut mgr);
    let check = mgr.take_friend_checks().remove(0);
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(mgr.rooms.contains_key(&1), "setup: the room must survive");
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(invitations(&mut rx2), 0);
}

#[test]
fn invitation_waits_for_a_slow_lookup_beyond_the_cooldown() {
    let mut mgr = new_mgr();
    let _rx2 = inviter_and_target(&mut mgr);
    invite_b(&mut mgr);
    assert_eq!(mgr.take_friend_checks().len(), 1);
    *mgr.last_invite.get_mut(&1).unwrap() -= INVITE_COOLDOWN; // unanswered, cooldown over
    invite_b(&mut mgr);
    assert!(mgr.take_friend_checks().is_empty(), "first lookup still in flight");
}

#[test]
fn limiter_allows_a_burst_then_drops() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    for _ in 0..CLIENT_MSG_BURST as usize {
        assert_eq!(l.check(t0), Verdict::Allow);
    }
    assert_eq!(l.check(t0), Verdict::Drop);
}

#[test]
fn limiter_never_drops_the_fastest_legitimate_player() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let per_sec = 250;
    for i in 0..(per_sec * 60) {
        let now = t0 + Duration::from_secs_f64(i as f64 / per_sec as f64);
        assert_eq!(l.check(now), Verdict::Allow, "legit message {i} throttled");
    }
}

#[test]
fn limiter_refills_over_time() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    for _ in 0..CLIENT_MSG_BURST as usize {
        l.check(t0);
    }
    assert_eq!(l.check(t0), Verdict::Drop);
    assert_eq!(l.check(t0 + Duration::from_millis(100)), Verdict::Allow);
}

#[test]
fn limiter_disconnects_a_sustained_flood() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let mut verdicts = (0..(CLIENT_MSG_BURST as u32 + FLOOD_DROPS_PER_SEC)).map(|_| l.check(t0));
    assert!(verdicts.all(|v| v != Verdict::Disconnect), "not before the threshold");
    assert_eq!(l.check(t0), Verdict::Disconnect);
}

async fn connect() -> (warp::test::WsClient, mpsc::Receiver<Command>) {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    let pool = db::DbPool::connect_lazy("postgres://unused@localhost/unused").expect("lazy pool");
    let client = warp::test::ws()
        .path("/ws")
        .handshake(ws_route(tx, pool))
        .await
        .expect("handshake");
    (client, rx)
}

fn frame(msg: &ClientMessage) -> warp::ws::Message {
    warp::ws::Message::binary(shared::encode(msg).expect("encode"))
}

async fn commands_until(rx: &mut mpsc::Receiver<Command>, stop: impl Fn(&Command) -> bool) -> Vec<Command> {
    let mut out = Vec::new();
    loop {
        let cmd = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for a command")
            .expect("channel closed");
        let done = stop(&cmd);
        out.push(cmd);
        if done {
            return out;
        }
    }
}

#[tokio::test]
async fn flooding_client_is_throttled_then_disconnected() {
    let (mut client, mut rx) = connect().await;
    let input = frame(&ClientMessage::Input {
        kind: InputKind::MoveLeft,
        seq: 1,
        tick: 0,
    });
    let started = Instant::now();
    for _ in 0..3_000 {
        client.send(input.clone()).await;
    }
    tokio::time::timeout(Duration::from_secs(5), client.recv_closed())
        .await
        .expect("server did not close the flooding connection")
        .expect("closed cleanly");
    let refill = started.elapsed().as_secs_f64() * CLIENT_MSG_RATE;

    let cmds = commands_until(&mut rx, |c| matches!(c, Command::Unregister { .. })).await;
    let inputs = cmds.iter().filter(|c| matches!(c, Command::Input { .. })).count();
    let most = (CLIENT_MSG_BURST + refill).ceil() as usize;
    assert!(
        inputs <= most,
        "{inputs} inputs reached the manager, at most {most} allowed"
    );
    assert!(
        inputs >= CLIENT_MSG_BURST as usize,
        "only {inputs}: the burst was not honoured"
    );
}

#[tokio::test]
async fn only_the_first_hello_per_connection_counts() {
    let (mut client, mut rx) = connect().await;
    let hello = frame(&ClientMessage::Hello {
        player_id: "p".into(),
        auth_token: None,
        username: None,
        last_disconnect_reason: None,
    });
    for _ in 0..5 {
        client.send(hello.clone()).await;
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await;
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    let hellos = cmds.iter().filter(|c| matches!(c, Command::Hello { .. })).count();
    assert_eq!(hellos, 1);
}

#[tokio::test]
async fn a_normal_session_gets_everything_through() {
    let (mut client, mut rx) = connect().await;
    for seq in 1..=100 {
        client
            .send(frame(&ClientMessage::Input {
                kind: InputKind::RotateCW,
                seq,
                tick: 0,
            }))
            .await;
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await;
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    assert_eq!(cmds.iter().filter(|c| matches!(c, Command::Input { .. })).count(), 100);
}

#[test]
fn limiter_absorbs_a_network_stall() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let stalled = 250 * 4;
    for i in 0..stalled {
        assert_eq!(l.check(t0), Verdict::Allow, "message {i} of the backlog dropped");
    }
}

#[test]
fn late_hello_from_a_dead_connection_is_ignored() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    hello(&mut mgr, 2, "B");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, None, "seat stays free for B's real reconnection");
    assert!(b.disconnect_at.is_some(), "grace keeps running");
    assert!(mgr.rooms[&1].sim.paused, "game stays paused");
    assert!(
        !mgr.conn_token.contains_key(&2),
        "no state recreated for the dead socket"
    );
}

#[test]
fn late_commands_from_a_dead_connection_are_ignored() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::Unregister { conn: 1 });
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "ghost".into(),
    });
    assert!(mgr.rooms.is_empty());
    assert!(!mgr.clients.contains_key(&1));
}

#[test]
fn real_reconnection_still_rebinds_the_seat() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, Some(3));
    assert!(!mgr.rooms[&1].sim.paused);
}

#[tokio::test]
async fn a_ping_is_answered_without_reaching_the_manager() {
    let (mut client, mut rx) = connect().await;
    client.send(frame(&ClientMessage::Ping { id: 7 })).await;

    let reply = tokio::time::timeout(Duration::from_secs(5), client.recv())
        .await
        .expect("no pong came back")
        .expect("socket error");
    assert!(matches!(
        shared::decode::<ServerMessage>(reply.as_bytes()),
        Some(ServerMessage::Pong { id: 7 })
    ));

    client.send(frame(&ClientMessage::RequestRoomList)).await;
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    assert!(
        matches!(cmds.first(), Some(Command::Register { .. })),
        "expected the socket's own Register first"
    );
    assert_eq!(cmds.len(), 2, "the ping reached the manager");
}

#[tokio::test]
async fn accepted_sockets_inherit_nodelay() {
    let listener = bind_listener(([127, 0, 0, 1], 0).into());
    let addr = listener.local_addr().expect("local_addr");
    let _client = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (accepted, _) = listener.accept().await.expect("accept");
    assert!(
        accepted.nodelay().expect("nodelay"),
        "TCP_NODELAY did not survive accept()"
    );
}

#[tokio::test(start_paused = true)]
async fn a_silent_socket_is_closed() {
    let (mut client, _rx) = connect().await;

    client
        .recv_closed()
        .await
        .expect("the server kept a socket that had gone quiet");
}

fn last_update_tick(rx: &mut mpsc::Receiver<Vec<u8>>) -> Option<u32> {
    drain(rx).into_iter().rev().find_map(|m| match m {
        ServerMessage::StateUpdate { tick, .. } => Some(tick),
        _ => None,
    })
}

const STEP: f32 = 1.0 / config::SERVER_TICK_HZ as f32;

#[test]
fn the_tick_counter_follows_the_simulation() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let start = mgr.rooms[&1].sim.tick;
    for _ in 0..10 {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.tick, start + 10);
}

#[test]
fn a_paused_game_stops_the_tick_counter() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::TogglePause { conn: 1 });
    let paused_at = mgr.rooms[&1].sim.tick;
    for _ in 0..10 {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.tick, paused_at, "the clock ran while paused");

    mgr.handle(Command::TogglePause { conn: 1 });
    mgr.tick(STEP, false);
    assert_eq!(mgr.rooms[&1].sim.tick, paused_at + 1, "and never restarted");
}

#[test]
fn a_decided_game_stops_the_tick_counter() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    let ended_at = mgr.rooms[&1].sim.tick;
    for _ in 0..10 {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.tick, ended_at);
}

#[test]
fn restart_resets_the_tick_counter() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    for _ in 0..30 {
        mgr.tick(STEP, false);
    }
    assert!(mgr.rooms[&1].sim.tick > 0, "setup: the clock must have run");

    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Restart { conn: 1 });
    assert_eq!(mgr.rooms[&1].sim.tick, 0);
}

#[test]
fn every_state_update_carries_the_current_tick() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = running_game(&mut mgr);
    for _ in 0..5 {
        mgr.tick(STEP, true);
    }
    let now = mgr.rooms[&1].sim.tick;
    assert_eq!(last_update_tick(&mut rx1), Some(now), "routine broadcast");

    mgr.send_snapshot(1);
    assert_eq!(last_update_tick(&mut rx1), Some(now), "snapshot");
}

fn piece_col(mgr: &Manager, slot: usize) -> i32 {
    mgr.rooms[&1].sim.boards[slot]
        .active_piece
        .as_ref()
        .expect("a piece must be falling")
        .col
}

fn press(mgr: &mut Manager, conn: ConnId, seq: u32, tick: u32) {
    mgr.handle(Command::Input {
        conn,
        kind: InputKind::MoveLeft,
        seq,
        tick,
    });
}

#[test]
fn an_input_waits_for_the_tick_it_was_stamped_for() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;
    let col = piece_col(&mgr, 0);

    press(&mut mgr, 1, 1, now + 5);
    for _ in 0..4 {
        mgr.tick(STEP, false);
    }
    assert_eq!(piece_col(&mgr, 0), col, "applied before the tick it named");

    mgr.tick(STEP, false);
    assert_eq!(piece_col(&mgr, 0), col - 1, "not applied on the tick it named");
}

#[test]
fn a_late_input_is_still_applied_and_counted() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    for _ in 0..20 {
        mgr.tick(STEP, false);
    }
    let col = piece_col(&mgr, 0);

    press(&mut mgr, 1, 1, 1);
    assert_eq!(mgr.rooms[&1].sim.late_inputs[0], 1, "not counted as late");
    mgr.tick(STEP, false);
    assert_eq!(piece_col(&mgr, 0), col - 1, "a late input must still land");
}

#[test]
fn an_input_stamped_far_ahead_is_clamped() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;
    let col = piece_col(&mgr, 0);

    press(&mut mgr, 1, 1, now + 100_000);
    for _ in 0..config::MAX_INPUT_LEAD_TICKS {
        mgr.tick(STEP, false);
    }
    assert_eq!(piece_col(&mgr, 0), col - 1, "an absurd stamp parked the input");
}

#[test]
fn an_input_is_acknowledged_only_once_processed() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;

    press(&mut mgr, 1, 7, now + 10);
    assert_eq!(mgr.rooms[&1].sim.last_seq[0], 0, "acknowledged on arrival");
    for _ in 0..10 {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.last_seq[0], 7);
}

#[test]
fn an_input_outside_a_running_game_is_acknowledged_and_dropped() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr); // still in the lobby
    press(&mut mgr, 1, 3, 0);
    assert_eq!(mgr.rooms[&1].sim.last_seq[0], 3);
    assert!(mgr.rooms[&1].sim.queued_inputs[0].is_empty());
}

#[test]
fn a_restart_clears_the_input_queue() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;
    press(&mut mgr, 1, 1, now + 20);
    assert!(!mgr.rooms[&1].sim.queued_inputs[0].is_empty(), "setup");

    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Restart { conn: 1 });
    assert!(mgr.rooms[&1].sim.queued_inputs[0].is_empty());
    assert_eq!(mgr.rooms[&1].sim.late_inputs, [0; 2]);
}

#[test]
fn a_full_input_queue_drops_without_acknowledging() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;

    let flood = INPUT_QUEUE_CAP as u32 + 50;
    for seq in 1..=flood {
        mgr.handle(Command::Input {
            conn: 1,
            kind: InputKind::RotateCW,
            seq,
            tick: now + 20,
        });
    }
    assert_eq!(mgr.rooms[&1].sim.queued_inputs[0].len(), INPUT_QUEUE_CAP);
    assert_eq!(mgr.rooms[&1].sim.last_seq[0], 0, "acknowledged what it dropped");

    for _ in 0..=config::MAX_INPUT_LEAD_TICKS {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.last_seq[0], INPUT_QUEUE_CAP as u32);
    assert!(mgr.rooms[&1].sim.queued_inputs[0].is_empty());
}

fn sim_of(mgr: &mut Manager) -> &mut Sim {
    &mut mgr.rooms.get_mut(&1).expect("room 1").sim
}

#[test]
fn an_attack_reaches_the_opponent_on_the_tick_that_produced_it() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    {
        let sim = sim_of(&mut mgr);
        for r in 8..=12 {
            sim.boards[0].cells[r][0] = Some(PuyoType::Red);
        }
        sim.boards[0].state = GameState::ResolvingMatches;
        sim.boards[0].resolve_timer = config::RESOLVE_STEP_INTERVAL;
    }
    assert_eq!(mgr.rooms[&1].sim.boards[1].pending_garbage, 0, "setup");

    mgr.tick(STEP, false);

    assert_eq!(
        mgr.rooms[&1].sim.boards[1].pending_garbage, 1,
        "the attack was held over to a later tick"
    );
    assert_eq!(mgr.rooms[&1].sim.nuisance_sent[0], 1);
    assert!(mgr.rooms[&1].sim.garbage_in_flight.is_empty(), "left in flight");
}

#[test]
fn an_attack_lands_on_the_tick_it_is_scheduled_for() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;
    sim_of(&mut mgr).send_garbage(0, 6, now + 5);

    for _ in 0..4 {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.boards[1].pending_garbage, 0, "landed early");

    mgr.tick(STEP, false);
    assert_eq!(mgr.rooms[&1].sim.boards[1].pending_garbage, 6);
}

#[test]
fn an_overdue_attack_lands_at_once() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    for _ in 0..20 {
        mgr.tick(STEP, false);
    }
    sim_of(&mut mgr).send_garbage(0, 3, 0);

    mgr.tick(STEP, false);
    assert_eq!(mgr.rooms[&1].sim.boards[1].pending_garbage, 3);
}

#[test]
fn an_empty_attack_is_not_an_event() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;
    for _ in 0..100 {
        sim_of(&mut mgr).send_garbage(0, 0, now);
    }
    assert!(mgr.rooms[&1].sim.garbage_in_flight.is_empty());
    assert_eq!(mgr.rooms[&1].sim.nuisance_sent, [0; 2]);
}

#[test]
fn a_restart_clears_attacks_in_flight() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let now = mgr.rooms[&1].sim.tick;
    sim_of(&mut mgr).send_garbage(0, 9, now + 50);
    assert!(!mgr.rooms[&1].sim.garbage_in_flight.is_empty(), "setup");

    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::Restart { conn: 1 });
    assert!(mgr.rooms[&1].sim.garbage_in_flight.is_empty());

    for _ in 0..60 {
        mgr.tick(STEP, false);
    }
    assert_eq!(
        mgr.rooms[&1].sim.boards[1].pending_garbage, 0,
        "an attack from the previous game landed in this one"
    );
}
