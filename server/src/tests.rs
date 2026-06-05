use super::*;

fn reg(mgr: &mut Manager, conn: ConnId) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(CLIENT_CHAN_CAP);
    mgr.handle(Command::Register { conn, sender: tx });
    rx
}

fn hello(mgr: &mut Manager, conn: ConnId, token: &str) {
    mgr.handle(Command::Hello { conn, token: token.to_string() });
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
    mgr.handle(Command::CreateRoom { conn: 1, name: "R".into() });
    let rx2 = reg(mgr, 2);
    hello(mgr, 2, "B");
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    (1, rx1, rx2)
}

#[test]
fn create_room_lobbies_host() {
    let mut mgr = Manager::new();
    let mut rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom { conn: 1, name: "Room".into() });
    let info = last_lobby(&drain(&mut rx1)).expect("host should get a Lobby");
    assert_eq!(info.players, 1);
    assert_eq!(info.your_slot, 1);
    assert!(info.is_host);
    assert!(mgr.rooms.contains_key(&1));
}

#[test]
fn second_player_join_updates_both() {
    let mut mgr = Manager::new();
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
    let mut mgr = Manager::new();
    let mut rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::JoinRoom { conn: 1, id: 999 });
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::JoinFailed { .. })));
}

#[test]
fn join_full_room_fails() {
    let mut mgr = Manager::new();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    let mut rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "C");
    mgr.handle(Command::JoinRoom { conn: 3, id: 1 });
    assert!(has(&drain(&mut rx3), |m| matches!(m, ServerMessage::JoinFailed { .. })));
    assert_eq!(mgr.rooms[&1].members.len(), 2);
}

#[test]
fn countdown_needs_two_players() {
    let mut mgr = Manager::new();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom { conn: 1, name: "R".into() });
    mgr.handle(Command::ToggleCountdown { conn: 1 }); // host is alone -> must be ignored
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
}

#[test]
fn countdown_starts_game_after_delay() {
    let mut mgr = Manager::new();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::CountingDown(_)));
    mgr.tick(3.5, false); // countdown elapses
    assert!(matches!(mgr.rooms[&1].phase, Phase::Playing));
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::GameStart)));
}

// A dropped player keeps their seat (grace period); reconnecting with the same
// token on a fresh connection re-attaches to that exact seat.
#[test]
fn disconnect_reserves_seat_then_reconnect_restores_it() {
    let mut mgr = Manager::new();
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
    let mut mgr = Manager::new();
    let (_id, _rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::LeaveRoom { conn: 1 }); // host A leaves voluntarily
    let room = &mgr.rooms[&1];
    assert_eq!(room.members.len(), 1);
    assert_eq!(room.host.as_str(), "B");
}

#[test]
fn last_member_leaving_closes_room() {
    let mut mgr = Manager::new();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom { conn: 1, name: "R".into() });
    assert!(mgr.rooms.contains_key(&1));
    mgr.handle(Command::LeaveRoom { conn: 1 });
    assert!(!mgr.rooms.contains_key(&1));
}

#[test]
fn opponent_disconnect_pauses_running_game() {
    let mut mgr = Manager::new();
    let (_id, mut rx1, _rx2) = two_player_room(&mut mgr);
    mgr.handle(Command::ToggleCountdown { conn: 1 });
    mgr.tick(3.5, false);
    assert!(matches!(mgr.rooms[&1].phase, Phase::Playing));
    let _ = drain(&mut rx1); // clear GameStart

    mgr.handle(Command::Unregister { conn: 2 }); // opponent drops mid-game
    assert!(mgr.rooms[&1].sim.paused, "game should pause when opponent drops");
    assert!(has(&drain(&mut rx1), |m| matches!(m, ServerMessage::OpponentDisconnected)));
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

    let mut mgr = Manager::new();
    let mut receivers = Vec::with_capacity(rooms * 2);
    for i in 0..rooms {
        let host = (2 * i + 1) as ConnId;
        let member = (2 * i + 2) as ConnId;
        receivers.push(reg(&mut mgr, host));
        hello(&mut mgr, host, &format!("h{i}"));
        mgr.handle(Command::CreateRoom { conn: host, name: "L".into() });
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
