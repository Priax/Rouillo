use super::*;

pub(super) fn told_of_maintenance(rx: &mut mpsc::Receiver<Vec<u8>>) -> bool {
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

pub(super) fn lazy_pool() -> db::DbPool {
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
