use futures_util::{SinkExt, StreamExt};
use shared::{
    config, ClientMessage, GameState, IncomingGarbage, InputKind, LobbyInfo, PausePolicy, PuyoType, RoomId,
    ServerMessage,
};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use super::*;
use crate::manager::friends::*;
use crate::manager::ranked::*;
use crate::manager::*;
use crate::room::*;
use crate::sim::*;
use crate::ws::*;

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
        session: None,
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
    assert_eq!(recs[0].winner_slot, Some(1), "the player who stayed wins");
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
    assert_eq!(recs[0].winner_slot, Some(1));
}

#[test]
fn host_returning_to_lobby_mid_game_is_a_forfeit() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::ReturnToLobby { conn: 1 });
    let recs = mgr.take_unsaved_matches();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].winner_slot, Some(2), "the host abandoned, the guest wins");
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
    assert_eq!(recs[0].winner_slot, Some(1));
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
        session: None,
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

type WsClient = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect() -> (WsClient, mpsc::Receiver<Command>) {
    connect_to(&format!("/ws?v={}", shared::PROTOCOL_VERSION)).await
}

async fn connect_to(path: &str) -> (WsClient, mpsc::Receiver<Command>) {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let app = ws_route(tx, lazy_pool()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let (client, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{path}"))
        .await
        .expect("handshake");
    (client, rx)
}

async fn closed(client: &mut WsClient) {
    while let Some(msg) = client.next().await {
        if matches!(msg, Ok(WsMessage::Close(_)) | Err(_)) {
            return;
        }
    }
}

#[tokio::test]
async fn an_outdated_client_is_told_so_and_dropped() {
    let other = shared::PROTOCOL_VERSION + 1;
    for path in ["/ws".to_string(), format!("/ws?v={other}")] {
        let (mut client, mut rx) = connect_to(&path).await;
        let msg = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("no reply")
            .expect("stream ended")
            .expect("recv");
        assert_eq!(msg.to_text().ok(), Some(shared::OUTDATED_FRAME), "{path}");
        let close = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("the server kept the connection open");
        assert!(
            matches!(close, Some(Ok(WsMessage::Close(Some(ref f)))) if u16::from(f.code) == 4426),
            "{path}: {close:?}"
        );
        assert!(rx.try_recv().is_err(), "{path}: an outdated client reached the manager");
    }
}

#[tokio::test]
async fn an_up_to_date_client_is_registered() {
    let (_client, mut rx) = connect().await;
    commands_until(&mut rx, |c| matches!(c, Command::Register { .. })).await;
}

fn frame(msg: &ClientMessage) -> WsMessage {
    WsMessage::binary(shared::encode(msg).expect("encode"))
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
        if client.send(input.clone()).await.is_err() {
            break;
        }
    }
    tokio::time::timeout(Duration::from_secs(5), closed(&mut client))
        .await
        .expect("server did not close the flooding connection");
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
        last_disconnect_reason: None,
    });
    for _ in 0..5 {
        client.send(hello.clone()).await.expect("send");
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await.expect("send");
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
            .await
            .expect("send");
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await.expect("send");
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
    client.send(frame(&ClientMessage::Ping { id: 7 })).await.expect("send");

    let reply = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("no pong came back")
        .expect("stream ended")
        .expect("socket error");
    assert!(matches!(
        shared::decode::<ServerMessage>(&reply.into_data()),
        Some(ServerMessage::Pong { id: 7 })
    ));

    client.send(frame(&ClientMessage::RequestRoomList)).await.expect("send");
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

    closed(&mut client).await;
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

fn attack_on_next_tick(mgr: &mut Manager) {
    let sim = sim_of(mgr);
    for row in &mut sim.boards[0].cells[8..=12] {
        row[0] = Some(PuyoType::Red);
    }
    sim.boards[0].state = GameState::ResolvingMatches;
    sim.boards[0].settle = shared::Settle::Idle;
}

#[test]
fn an_attack_travels_before_it_lands() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    attack_on_next_tick(&mut mgr);

    mgr.tick(STEP, false);
    let sent_at = mgr.rooms[&1].sim.tick;
    assert_eq!(mgr.rooms[&1].sim.garbage_in_flight.len(), 1, "no attack was produced");

    for _ in 1..config::GARBAGE_TRAVEL_TICKS {
        mgr.tick(STEP, false);
    }
    assert_eq!(mgr.rooms[&1].sim.boards[1].pending_garbage, 0, "landed early");
    assert_eq!(mgr.rooms[&1].sim.nuisance_sent[0], 0, "credited before it landed");

    mgr.tick(STEP, false);
    assert_eq!(mgr.rooms[&1].sim.tick, sent_at + config::GARBAGE_TRAVEL_TICKS);
    assert_eq!(mgr.rooms[&1].sim.boards[1].pending_garbage, 1);
    assert_eq!(mgr.rooms[&1].sim.nuisance_sent[0], 1);
    assert!(mgr.rooms[&1]
        .sim
        .garbage_in_flight
        .iter()
        .all(|g| g.at > sent_at + config::GARBAGE_TRAVEL_TICKS));
}

#[test]
fn the_victim_is_told_of_an_attack_in_flight() {
    let mut mgr = new_mgr();
    let (mut rx1, mut rx2) = running_game(&mut mgr);
    attack_on_next_tick(&mut mgr);
    mgr.tick(STEP, false);
    let lands = mgr.rooms[&1].sim.tick + config::GARBAGE_TRAVEL_TICKS;
    drain(&mut rx1);
    drain(&mut rx2);

    mgr.tick(STEP, true);
    let update = drain(&mut rx2)
        .into_iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::StateUpdate {
                p1_incoming,
                p2_incoming,
                ..
            } => Some((p1_incoming, p2_incoming)),
            _ => None,
        })
        .expect("no update");
    assert_eq!(update.0, vec![], "the attacker was told it is under attack");
    assert_eq!(update.1, vec![IncomingGarbage { at: lands, amount: 1 }]);
}

#[test]
fn an_attack_still_in_flight_at_the_end_is_not_credited() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    attack_on_next_tick(&mut mgr);
    mgr.tick(STEP, false);
    assert!(!mgr.rooms[&1].sim.garbage_in_flight.is_empty(), "setup");

    sim_of(&mut mgr).boards[1].state = GameState::GameOver;
    mgr.tick(STEP, false);
    assert!(mgr.rooms[&1].sim.finished, "setup: the game should be over");
    assert_eq!(mgr.rooms[&1].sim.nuisance_sent[0], 0);
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

fn update_rngs(rx: &mut mpsc::Receiver<Vec<u8>>) -> Option<(bool, bool)> {
    drain(rx).into_iter().rev().find_map(|m| match m {
        ServerMessage::StateUpdate { p1_rng, p2_rng, .. } => Some((p1_rng.is_some(), p2_rng.is_some())),
        _ => None,
    })
}

#[test]
fn the_rng_is_resent_when_a_drop_draws_from_it() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = running_game(&mut mgr);
    mgr.tick(STEP, true);
    drain(&mut rx1);
    mgr.tick(STEP, true);
    assert_eq!(
        update_rngs(&mut rx1),
        Some((false, false)),
        "setup: a quiet tick resends nothing"
    );

    let before = mgr.rooms[&1].sim.boards[0].rng_position();
    {
        let b = &mut sim_of(&mut mgr).boards[0];
        b.pending_garbage = 3; // not a multiple of 6: the leftover columns are drawn
        b.state = GameState::DroppingGarbage;
        b.drop_garbage();
    }
    assert_ne!(
        mgr.rooms[&1].sim.boards[0].rng_position(),
        before,
        "setup: the drop drew nothing"
    );
    let pid = mgr.rooms[&1].sim.boards[0].piece_id;
    mgr.tick(STEP, true);
    assert_eq!(
        mgr.rooms[&1].sim.boards[0].piece_id, pid,
        "setup: a new pair already came"
    );
    assert_eq!(update_rngs(&mut rx1), Some((true, false)));
}

fn told_of_maintenance(rx: &mut mpsc::Receiver<Vec<u8>>) -> bool {
    has(&drain(rx), |m| matches!(m, ServerMessage::Maintenance))
}

#[test]
fn a_shutdown_tells_everyone_connected() {
    let mut mgr = new_mgr();
    let (mut rx1, mut rx2) = running_game(&mut mgr);
    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "C");
    mgr.handle(Command::Shutdown);
    assert!(mgr.closing());
    for rx in [&mut rx1, &mut rx2, &mut rx3] {
        assert!(told_of_maintenance(rx));
    }
}

#[test]
fn a_shutdown_leaves_a_running_game_alone() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Shutdown);
    assert_eq!(mgr.games_running(), 1);
    let before = mgr.rooms[&1].sim.tick;
    mgr.tick(STEP, true);
    assert!(mgr.rooms[&1].sim.tick > before, "the game must keep advancing");
    assert!(!mgr.rooms[&1].sim.paused);
}

#[test]
fn a_decided_game_no_longer_holds_a_shutdown_back() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Shutdown);
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert_eq!(mgr.take_unsaved_matches().len(), 1, "the forfeit is still recorded");
    assert_eq!(mgr.games_running(), 0);
}

#[test]
fn no_rematch_during_a_shutdown() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    sim_of(&mut mgr).finished = true;
    mgr.handle(Command::Shutdown);
    mgr.handle(Command::Restart { conn: 1 });
    assert!(mgr.rooms[&1].sim.finished, "a rematch would hold the server up again");
    assert_eq!(mgr.games_running(), 0);
}

#[test]
fn a_shutdown_cancels_a_countdown_and_refuses_new_ones() {
    let mut mgr = new_mgr();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::CountingDown(_)));
    drain(&mut rx1);
    mgr.handle(Command::Shutdown);
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
    let lobby = last_lobby(&drain(&mut rx1)).expect("the host must see the countdown stop");
    assert_eq!(lobby.countdown, None);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
    assert_eq!(mgr.games_running(), 0);
}

#[test]
fn no_new_room_and_no_join_during_a_shutdown() {
    let mut mgr = new_mgr();
    let mut rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let mut rx2 = reg(&mut mgr, 2);
    hello(&mut mgr, 2, "B");
    mgr.handle(Command::Shutdown);
    drain(&mut rx1);
    drain(&mut rx2);

    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::JoinFailed { .. })));
    assert_eq!(mgr.rooms[&1].members.len(), 1);

    mgr.handle(Command::CreateRoom {
        conn: 2,
        name: "S".into(),
    });
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::JoinFailed { .. })));
    assert_eq!(mgr.rooms.len(), 1);
}

#[test]
fn a_join_being_checked_when_the_shutdown_starts_is_refused() {
    let mut mgr = new_mgr();
    let (_rx1, mut rx2) = friends_only_room(&mut mgr);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    let check = only_join_check(&mut mgr);
    mgr.handle(Command::Shutdown);
    drain(&mut rx2);
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.room_of(2), None);
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn a_player_can_still_come_back_during_a_shutdown() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Shutdown);
    mgr.handle(Command::Unregister { conn: 2 });
    assert!(mgr.rooms[&1].sim.paused);
    assert_eq!(mgr.games_running(), 1, "a held seat is still a game to wait for");

    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    assert_eq!(mgr.room_of(3), Some(1));
    assert!(!mgr.rooms[&1].sim.paused, "the game resumes");
    assert!(
        told_of_maintenance(&mut rx3),
        "a client that connects late must learn of it too"
    );
}

#[test]
fn a_shutdown_with_nothing_running_exits_at_once() {
    let now = Instant::now();
    assert_eq!(Winddown::new(now).check(0, now), Some(Exit::Clean));
}

#[test]
fn a_shutdown_waits_for_games_then_lingers() {
    let start = Instant::now();
    let mut w = Winddown::new(start);
    let ended = start + Duration::from_secs(90);
    assert_eq!(w.check(1, start), None);
    assert_eq!(w.check(1, ended), None);
    assert_eq!(w.check(0, ended + SHUTDOWN_LINGER / 2), None, "the result must be seen");
    assert_eq!(w.check(0, ended + SHUTDOWN_LINGER), Some(Exit::Clean));
}

#[test]
fn a_shutdown_gives_up_on_a_game_that_never_ends() {
    let start = Instant::now();
    let mut w = Winddown::new(start);
    assert_eq!(w.check(1, start + SHUTDOWN_DEADLINE - Duration::from_secs(1)), None);
    assert_eq!(w.check(1, start + SHUTDOWN_DEADLINE), Some(Exit::TimedOut));
}

fn lazy_pool() -> db::DbPool {
    db::DbPool::connect_lazy("postgres://unused@localhost/unused").expect("lazy pool")
}

#[tokio::test(start_paused = true)]
async fn the_manager_loop_returns_on_shutdown() {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    let manager = tokio::spawn(manager_loop(rx, tx.clone(), lazy_pool()));
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert!(!manager.is_finished(), "it must not stop unasked");
    tx.send(Command::Shutdown).await.expect("manager gone");
    tokio::time::timeout(Duration::from_secs(1), manager)
        .await
        .expect("the manager kept running with no game to wait for")
        .expect("manager panicked");
}

#[tokio::test(start_paused = true)]
async fn the_manager_loop_outlives_a_shutdown_while_a_game_runs() {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    let manager = tokio::spawn(manager_loop(rx, tx.clone(), lazy_pool()));
    for (conn, token) in [(1, "A"), (2, "B")] {
        let (sender, mut out) = mpsc::channel(CLIENT_CHAN_CAP);
        tokio::spawn(async move { while out.recv().await.is_some() {} });
        let hello = Command::Hello {
            last_disconnect_reason: None,
            conn,
            token: token.to_string(),
            user_id: None,
            username: None,
            session: None,
        };
        for cmd in [Command::Register { conn, sender }, hello] {
            tx.send(cmd).await.expect("manager gone");
        }
    }
    let start = [
        Command::CreateRoom {
            conn: 1,
            name: "R".into(),
        },
        Command::JoinRoom { conn: 2, id: 1 },
        Command::ToggleCountdown { conn: 1 },
    ];
    for cmd in start {
        tx.send(cmd).await.expect("manager gone");
    }
    tokio::time::sleep(Duration::from_secs(4)).await;
    tx.send(Command::TogglePause { conn: 1 }).await.expect("manager gone");
    tx.send(Command::Shutdown).await.expect("manager gone");
    tokio::time::sleep(SHUTDOWN_DEADLINE - Duration::from_secs(30)).await;
    assert!(!manager.is_finished(), "the server left in the middle of a game");
    tokio::time::timeout(Duration::from_secs(60), manager)
        .await
        .expect("a game that never ends held the server past its deadline")
        .expect("manager panicked");
}

#[test]
fn an_account_cannot_take_both_slots() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello_as(&mut mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let mut rx2 = reg(&mut mgr, 2);
    hello_as(&mut mgr, 2, "B", 1);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    assert_eq!(mgr.rooms[&1].members.len(), 1);
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn logged_reasons_are_bounded_and_single_line() {
    let s = log_safe(&format!("a\nfake log line\r{}", "x".repeat(10_000)));
    assert!(!s.contains('\n') && !s.contains('\r'));
    assert_eq!(s.chars().count(), 100);
}

#[test]
fn ws_connections_are_capped_per_ip_and_freed_on_drop() {
    let conns = IpConns::default();
    let ip = || Some("203.0.113.7".to_string());
    let slots: Vec<_> = (0..MAX_WS_PER_IP)
        .map(|_| IpSlot::take(&conns, ip()).unwrap())
        .collect();
    assert!(IpSlot::take(&conns, ip()).is_err());
    assert!(IpSlot::take(&conns, Some("198.51.100.1".into())).is_ok());
    drop(slots);
    assert!(IpSlot::take(&conns, ip()).is_ok());
    assert!(conns.lock().unwrap().is_empty());
}

fn hello_session(mgr: &mut Manager, conn: ConnId, token: &str, user: u128, session: u128) {
    mgr.handle(Command::Hello {
        conn,
        token: token.to_string(),
        user_id: Some(Uuid::from_u128(user)),
        username: Some(token.to_lowercase()),
        session: Some(Uuid::from_u128(session)),
        last_disconnect_reason: None,
    });
}

fn queue_up(mgr: &mut Manager, conn: ConnId, user: u128, elo: i32, casual: i64) -> mpsc::Receiver<Vec<u8>> {
    let rx = reg(mgr, conn);
    hello_as(mgr, conn, &format!("P{conn}"), user);
    mgr.handle(Command::JoinQueue { conn });
    let checks = mgr.take_ranked_checks();
    assert_eq!(checks.len(), 1, "setup: one lookup per queue request");
    mgr.handle(Command::RankedCheckDone {
        check: checks[0],
        profile: Some((elo, casual)),
    });
    rx
}

fn ranked_id(mgr: &Manager) -> Option<RoomId> {
    mgr.rooms.values().find(|r| r.series.is_some()).map(|r| r.id)
}

fn accept_all(mgr: &mut Manager) {
    let conns: Vec<ConnId> = mgr
        .pending_matches
        .iter()
        .flat_map(|m| m.entries.iter().map(|e| e.conn))
        .collect();
    for conn in conns {
        mgr.handle(Command::AcceptMatch { conn });
    }
}

fn match_up(mgr: &mut Manager) -> Option<RoomId> {
    mgr.tick(STEP, false);
    accept_all(mgr);
    ranked_id(mgr)
}

fn ranked_pair(mgr: &mut Manager) -> (RoomId, mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let rx1 = queue_up(mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let rx2 = queue_up(mgr, 2, 2, 1050, config::RANKED_MIN_CASUAL);
    let id = match_up(mgr).expect("setup: the two players are matched");
    mgr.tick(3.5, false);
    assert!(mgr.rooms[&id].game_running(), "setup: the first game starts");
    (id, rx1, rx2)
}

fn lose(mgr: &mut Manager, id: RoomId, slot: usize) {
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
fn close_players_are_matched_into_a_hidden_series() {
    let mut mgr = new_mgr();
    let mut rx1 = queue_up(&mut mgr, 1, 1, 1000, config::RANKED_MIN_CASUAL);
    let _rx2 = queue_up(&mut mgr, 2, 2, 1050, config::RANKED_MIN_CASUAL);
    let id = match_up(&mut mgr).expect("matched");
    assert!(mgr.queue.is_empty());
    assert!(mgr.public_room_list().is_empty(), "a ranked room is not listed");
    assert!(matches!(mgr.rooms[&id].phase, Phase::CountingDown(_)));
    let lobby = drain(&mut rx1).into_iter().find_map(|m| match m {
        ServerMessage::Lobby { info } => info.ranked,
        _ => None,
    });
    assert_eq!(lobby.map(|r| (r.opponent, r.opponent_elo)), Some(("p2".into(), 1050)));
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

fn cancelled(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<ServerMessage> {
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

fn found_pair(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
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
            secs,
        } => Some((opponent, opponent_elo, secs)),
        _ => None,
    });
    assert_eq!(found, Some(("p2".into(), 1050, config::MATCH_ACCEPT_SECS)));
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
