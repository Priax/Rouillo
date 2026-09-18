use shared::PausePolicy;

use super::*;

/// The manager holds no database pool, so it needs no runtime or DB to test.
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

/// Host "A" on conn 1 creates room 1, then "B" on conn 2 joins it.
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
    mgr.handle(Command::ToggleCountdown { conn: 1 }); // host is alone -> must be ignored
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
}

#[test]
fn countdown_starts_game_after_delay() {
    let mut mgr = new_mgr();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::CountingDown(_)));
    mgr.tick(3.5, false); // countdown elapses
    assert!(matches!(mgr.rooms[&1].phase, Phase::Playing));
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::GameStart)));
}

#[test]
fn disconnect_reserves_seat_then_reconnect_restores_it() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);

    mgr.handle(Command::Unregister { conn: 2 }); // player B drops
    assert_eq!(mgr.rooms[&1].members.len(), 2, "seat must be kept during grace");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, None);

    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B"); // same token, new connection
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, Some(3u64), "reconnect should re-bind the same seat");
}

#[test]
fn host_leaving_promotes_remaining_member() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::LeaveRoom { conn: 1 }); // host A leaves voluntarily
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

// Spins up K rooms all in-game and
// times Manager::tick across N broadcast ticks, then extrapolates how many rooms
// would fill one 60 Hz tick on a single thread. Debug timings are meaningless,
// To run it in release:
//   cargo test -p server --release -- --ignored --nocapture load_many_rooms
//   PUYO_LOAD_ROOMS=2000 cargo test -p server --release -- --ignored --nocapture load_many_rooms
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
    mgr.tick(3.5, false); // elapse every countdown -> all rooms enter Playing
    let playing = mgr.rooms.values().filter(|r| matches!(r.phase, Phase::Playing)).count();
    assert_eq!(playing, rooms, "every room should be playing");
    drain_all(&mut receivers);

    let mut sum = Duration::ZERO;
    let mut max = Duration::ZERO;
    for _ in 0..ticks {
        let t0 = Instant::now();
        mgr.tick(dt, true); // broadcast tick: simulates + encodes every room
        let e = t0.elapsed();
        sum += e;
        max = max.max(e);
        drain_all(&mut receivers); // kept out of the timed section
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

/// Host "A" (conn 1) and "B" (conn 2) in room 1, game started.
fn running_game(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let (_id, rx1, rx2) = two_player_room(mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    assert!(mgr.rooms[&1].game_running(), "setup: game must be running");
    assert!(mgr.take_unsaved_matches().is_empty(), "setup: nothing decided yet");
    (rx1, rx2)
}

// ---- A game never starts without both players ------------------------------

/// Regression: the countdown only checked the seat count, and a disconnected
/// player keeps their seat. The game then started without them, unpaused, and
/// their board ran out into a recorded loss.
#[test]
fn countdown_refused_while_a_player_is_disconnected() {
    let mut mgr = new_mgr();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
}

/// Regression: a player dropping mid-countdown did not cancel it.
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

// ---- Walking away from a running game is a loss ------------------------------

/// Regression: pausing then leaving used to record nothing, dodging the loss.
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

/// Joining or creating another room mid-game is leaving it too.
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
    // Grace expiry reads the wall clock, not `tick`'s dt: backdate the drop.
    let b = mgr
        .rooms
        .get_mut(&1)
        .unwrap()
        .members
        .iter_mut()
        .find(|m| m.token == "B")
        .unwrap();
    b.disconnect_at = Some(Instant::now() - GRACE - Duration::from_secs(1));
    mgr.tick(1.0 / 60.0, false);
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot, 1);
}

/// Leaving while the opponent is themselves disconnected voids the game: awarding
/// it would let a player hand their opponent a loss during a network blip.
#[test]
fn leaving_while_opponent_is_disconnected_voids_the_game() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(mgr.take_unsaved_matches().is_empty());
}

/// A decided game is never settled twice.
#[test]
fn leaving_a_finished_game_records_nothing_more() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.rooms.get_mut(&1).unwrap().sim.finished = true;
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert!(mgr.take_unsaved_matches().is_empty());
}

/// Regression: restart was accepted mid-game, discarding it without a result.
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

// ---- Rooms --------------------------------------------------------------------

/// Regression: once the only connected player left, the room stayed listed with
/// a disconnected host nobody could replace until the grace ran out.
#[test]
fn room_with_only_disconnected_members_is_closed() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(!mgr.rooms.contains_key(&1));
    assert!(mgr.public_room_list().is_empty());
}

/// The disconnected player of a closed room lands on the room list on return.
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

/// Regression: joining the room you are alone in emptied it and deleted it.
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

// ---- Pause --------------------------------------------------------------------

fn set_pause_policy(mgr: &mut Manager, policy: PausePolicy) {
    // Bounded: if the setting stopped changing, fail rather than loop forever.
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

/// Only the host may change the policy, and only in the lobby.
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

/// A disconnect pause is lifted by the reconnection, never by the opponent:
/// unpausing would let the absent player's board run out.
#[test]
fn opponent_cannot_lift_a_disconnect_pause() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(mgr.rooms[&1].sim.paused);
    mgr.handle(Command::TogglePause { conn: 1 });
    assert!(mgr.rooms[&1].sim.paused);
}

/// The host is told a seat is only held, so the lobby can explain why the game
/// cannot be launched instead of the button silently doing nothing.
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

/// Regression: after a decided game, restarting while the opponent had closed
/// their client started a new, unpaused game against nobody — the same recorded
/// loss for an absent player that the countdown gate prevents.
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

// ---- Friendship checks never run on the loop ----------------------------------

fn hello_as(mgr: &mut Manager, conn: ConnId, token: &str, user: u128) {
    mgr.handle(Command::Hello {
        conn,
        token: token.to_string(),
        user_id: Some(Uuid::from_u128(user)),
        username: Some(token.to_lowercase()),
        last_disconnect_reason: None,
    });
}

/// Host "A" (conn 1, user 1) alone in friends-only room 1; "J" (conn 2, user 2)
/// connected and browsing.
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

/// Regression: the lookup used to run inside `handle` via `block_in_place`,
/// freezing every room for the round-trip. Now the join is only queued; the
/// manager has no database pool at all, so it cannot wait on one.
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

/// Repeated join clicks must not become a stream of database queries.
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

/// The player did something else while waiting: the late answer must not drag
/// them out of where they are now.
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
    // Connection ids are never reused: anything left here would leak forever.
    assert!(!mgr.checks_in_flight.contains(&2), "cleaned up on disconnect");
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.rooms[&1].members.len(), 1);
}

/// Someone else took the seat while the lookup was pending: the late answer
/// must not squeeze a third player into a two-seat room.
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

/// If the host changed while waiting, the answer was about the wrong person:
/// ask again about the new host rather than trusting it.
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

// ---- Invitations --------------------------------------------------------------

/// Host "A" (conn 1, user 1) in room 1; "B" (conn 2, user 2) connected.
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

/// Regression: invitations were delivered to anyone, without a friendship check.
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

/// Regression: 50 invite commands used to push 50 banners.
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

    *mgr.last_invite.get_mut(&1).unwrap() -= INVITE_COOLDOWN; // time passes
    invite_b(&mut mgr);
    assert_eq!(mgr.take_friend_checks().len(), 1, "allowed again after the cooldown");
}

#[test]
fn guests_cannot_invite() {
    let mut mgr = new_mgr();
    let _ = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A"); // no account
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

/// The room outlives the inviter (someone else is still in it): the invitation
/// must not send a friend into a room the inviter has already left.
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

/// With a slow database a lookup can outlast the cooldown: a second one must
/// still wait for the first rather than stack up.
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

// ---- Rate limiter -------------------------------------------------------------

#[test]
fn limiter_allows_a_burst_then_drops() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    for _ in 0..CLIENT_MSG_BURST as usize {
        assert_eq!(l.check(t0), Verdict::Allow);
    }
    assert_eq!(l.check(t0), Verdict::Drop);
}

/// The fastest legitimate player must never be throttled.
#[test]
fn limiter_never_drops_the_fastest_legitimate_player() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    // 200 moves/s (DAS at 5 ms) plus soft drop and rotations, for a minute.
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
    // The whole burst, then exactly FLOOD_DROPS_PER_SEC tolerated drops...
    let mut verdicts = (0..(CLIENT_MSG_BURST as u32 + FLOOD_DROPS_PER_SEC)).map(|_| l.check(t0));
    assert!(verdicts.all(|v| v != Verdict::Disconnect), "not before the threshold");
    // ...and the next one is the flood.
    assert_eq!(l.check(t0), Verdict::Disconnect);
}

// ---- End to end, through the real WebSocket layer -----------------------------

async fn connect() -> (warp::test::WsClient, mpsc::Receiver<Command>) {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    // Never used by these tests (no auth token is sent), so never connects.
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

/// Drains commands until `stop` matches one (inclusive), with a timeout.
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

/// Regression: every message went straight to the manager, unbounded. A flood
/// is now throttled before decoding and the connection closed.
#[tokio::test]
async fn flooding_client_is_throttled_then_disconnected() {
    let (mut client, mut rx) = connect().await;
    let input = frame(&ClientMessage::Input {
        kind: InputKind::MoveLeft,
        seq: 1,
    });
    let started = Instant::now();
    for _ in 0..3_000 {
        client.send(input.clone()).await;
    }
    tokio::time::timeout(Duration::from_secs(5), client.recv_closed())
        .await
        .expect("server did not close the flooding connection")
        .expect("closed cleanly");
    // The bucket refills in real time, so how much may get through depends on
    // how long this machine took: bound it by that, or a slow CI runner flakes.
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

/// Hello costs a database lookup; only the first one per socket counts.
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

/// A normal client is untouched by all of this.
#[tokio::test]
async fn a_normal_session_gets_everything_through() {
    let (mut client, mut rx) = connect().await;
    for seq in 1..=100 {
        client
            .send(frame(&ClientMessage::Input {
                kind: InputKind::RotateCW,
                seq,
            }))
            .await;
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await;
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    assert_eq!(cmds.iter().filter(|c| matches!(c, Command::Input { .. })).count(), 100);
}

/// A player whose link stalled for a few seconds sends everything at once on
/// recovery. At the fastest legitimate rate, none of it may be dropped.
#[test]
fn limiter_absorbs_a_network_stall() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let stalled = 250 * 4; // 4 s of play at 250 msg/s, arriving in one instant
    for i in 0..stalled {
        assert_eq!(l.check(t0), Verdict::Allow, "message {i} of the backlog dropped");
    }
}

/// Regression: a Hello its socket had queued just before dying could land after
/// the Unregister (abort() does not wait for the reader task). It rebound the seat
/// to the dead connection, cleared the grace timer and resumed the game.
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

/// Same window, any command: nothing a dead connection sends may create state.
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

/// B's genuine reconnection still works: it arrives on a new, registered socket.
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
