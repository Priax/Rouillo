use super::*;

pub(super) fn spectator(mgr: &mut Manager, conn: ConnId) -> mpsc::Receiver<Vec<u8>> {
    let mut rx = reg(mgr, conn);
    hello(mgr, conn, &format!("S{conn}"));
    mgr.handle(Command::Spectate { conn, id: 1 });
    let _ = &mut rx;
    rx
}

#[test]
fn a_spectator_sees_the_game_and_cannot_touch_it() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let mut rx3 = spectator(&mut mgr, 3);
    assert_eq!(mgr.room_of(3), Some(1));
    let got = drain(&mut rx3);
    assert!(has(
        &got,
        |m| matches!(m, ServerMessage::Lobby { info } if info.your_slot == 0 && !info.is_host)
    ));
    assert!(has(&got, |m| matches!(m, ServerMessage::GameStart)));
    assert!(has(&got, |m| matches!(m, ServerMessage::StateUpdate { .. })));

    mgr.handle(Command::TogglePause { conn: 3 });
    assert!(!mgr.rooms[&1].sim.paused, "a spectator paused the game");
    mgr.handle(Command::Input {
        conn: 3,
        kind: InputKind::HardDrop,
        seq: 1,
        tick: 1,
    });
    assert!(mgr.rooms[&1].sim.queued_inputs.iter().all(Vec::is_empty));
    mgr.handle(Command::InviteFriend {
        conn: 3,
        target_user_id: Uuid::from_u128(2).to_string(),
    });
    assert!(mgr.take_friend_checks().is_empty());

    mgr.tick(STEP, true);
    assert!(has(&drain(&mut rx3), |m| matches!(
        m,
        ServerMessage::StateUpdate { .. }
    )));
}

#[test]
fn spectators_leave_and_are_sent_back_when_the_room_closes() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let mut rx3 = spectator(&mut mgr, 3);
    let mut rx4 = spectator(&mut mgr, 4);
    assert_eq!(mgr.public_room_list()[0].spectators, 2);

    mgr.handle(Command::LeaveRoom { conn: 3 });
    assert_eq!((mgr.room_of(3), mgr.rooms[&1].spectators.len()), (None, 1));
    assert!(has(&drain(&mut rx3), |m| matches!(m, ServerMessage::RoomList { .. })));
    assert!(
        mgr.take_unsaved_matches().is_empty(),
        "a spectator leaving is no forfeit"
    );

    drain(&mut rx4);
    mgr.handle(Command::LeaveRoom { conn: 1 });
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert!(mgr.rooms.is_empty());
    assert_eq!(mgr.room_of(4), None);
    assert!(has(&drain(&mut rx4), |m| matches!(m, ServerMessage::RoomList { .. })));
}

#[test]
fn a_spectator_coming_or_going_leaves_the_players_in_their_game() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = running_game(&mut mgr);
    drain(&mut rx1);
    let _rx3 = spectator(&mut mgr, 3);
    mgr.handle(Command::LeaveRoom { conn: 3 });
    assert!(
        !has(&drain(&mut rx1), |m| matches!(m, ServerMessage::Lobby { .. })),
        "a Lobby message would have sent the player back to the lobby"
    );
}

#[test]
fn a_disconnected_spectator_is_gone_at_once() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let _rx3 = spectator(&mut mgr, 3);
    mgr.handle(Command::Unregister { conn: 3 });
    assert!(mgr.rooms[&1].spectators.is_empty());
    assert!(!mgr.rooms[&1].sim.paused, "only a player's drop pauses");
}

#[test]
fn players_in_a_game_hear_how_many_people_watch() {
    let mut mgr = new_mgr();
    let (mut rx1, _rx2) = running_game(&mut mgr);
    drain(&mut rx1);
    let counts = |rx: &mut mpsc::Receiver<Vec<u8>>| -> Vec<u8> {
        drain(rx)
            .into_iter()
            .filter_map(|m| match m {
                ServerMessage::Spectators { count } => Some(count),
                _ => None,
            })
            .collect()
    };
    let _rx3 = spectator(&mut mgr, 3);
    let mut rx4 = spectator(&mut mgr, 4);
    assert_eq!(counts(&mut rx1), vec![1, 2]);
    drain(&mut rx4);
    mgr.handle(Command::Unregister { conn: 3 });
    assert_eq!(counts(&mut rx1), vec![1]);
    assert_eq!(counts(&mut rx4), vec![1]);
}

pub(super) fn playing(mgr: &mut Manager, users: Vec<Uuid>) -> Vec<Uuid> {
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    mgr.handle(Command::Playing { users, reply });
    answer.try_recv().expect("answered at once")
}

#[test]
fn the_friend_list_learns_who_is_in_a_room() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello_as(&mut mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let _rx2 = reg(&mut mgr, 2);
    hello_as(&mut mgr, 2, "B", 2);
    let (u1, u2, u3) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
    assert_eq!(playing(&mut mgr, vec![u1, u2, u3]), vec![u1]);
    mgr.handle(Command::Unregister { conn: 1 });
    assert!(
        playing(&mut mgr, vec![u1]).is_empty(),
        "gone players are not shown as playing"
    );
}

pub(super) fn watch_check(mgr: &mut Manager) -> FriendCheck {
    let checks = mgr.take_friend_checks();
    assert_eq!(checks.len(), 1, "exactly one lookup expected");
    assert!(matches!(checks[0], FriendCheck::Watch { .. }));
    checks[0].clone()
}

#[test]
fn a_friends_only_room_is_watched_by_the_hosts_friends_only() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = friends_only_room(&mut mgr);
    let mut guest = spectator(&mut mgr, 3);
    assert!(mgr.rooms[&1].spectators.is_empty(), "a guest has no friends");
    assert!(has(&drain(&mut guest), |m| matches!(
        m,
        ServerMessage::JoinFailed { .. }
    )));

    let mut rx4 = reg(&mut mgr, 4);
    hello_as(&mut mgr, 4, "F", 4);
    mgr.handle(Command::Spectate { conn: 4, id: 1 });
    let check = watch_check(&mut mgr);
    assert_eq!(
        check.users(),
        (Uuid::from_u128(4), Uuid::from_u128(1)),
        "asked about the host"
    );
    mgr.handle(Command::FriendCheckDone {
        check: check.clone(),
        friends: false,
    });
    assert!(mgr.rooms[&1].spectators.is_empty());
    assert!(has(&drain(&mut rx4), |m| matches!(m, ServerMessage::JoinFailed { .. })));
    mgr.handle(Command::Spectate { conn: 4, id: 1 });
    let check = watch_check(&mut mgr);
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.rooms[&1].spectators.len(), 1);
}

#[test]
fn a_late_watch_answer_leaves_a_player_who_moved_on_alone() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello_as(&mut mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let _rx4 = reg(&mut mgr, 4);
    hello_as(&mut mgr, 4, "F", 4);
    mgr.handle(Command::WatchFriend {
        conn: 4,
        user_id: Uuid::from_u128(1).to_string(),
    });
    let check = watch_check(&mut mgr);
    mgr.handle(Command::CreateRoom {
        conn: 4,
        name: "own".into(),
    });
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.room_of(4), Some(2), "pulled out of the room it made meanwhile");
}

#[test]
fn watching_through_a_friend_respects_the_spectator_limit() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello_as(&mut mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let _watchers: Vec<_> = (0..shared::MAX_SPECTATORS as u64)
        .map(|i| spectator(&mut mgr, 100 + i))
        .collect();
    assert_eq!(mgr.rooms[&1].spectators.len(), shared::MAX_SPECTATORS, "setup");
    let _rx4 = reg(&mut mgr, 4);
    hello_as(&mut mgr, 4, "F", 4);
    mgr.handle(Command::WatchFriend {
        conn: 4,
        user_id: Uuid::from_u128(1).to_string(),
    });
    let check = watch_check(&mut mgr);
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.rooms[&1].spectators.len(), shared::MAX_SPECTATORS);
    assert_eq!(mgr.room_of(4), None);
}

#[test]
fn a_ranked_series_is_listed_and_watched_but_never_joined() {
    let mut mgr = new_mgr();
    let (id, _rx1, _rx2) = ranked_pair(&mut mgr);
    assert!(mgr.public_room_list().iter().any(|r| r.id == id && r.ranked));
    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "S3");
    mgr.handle(Command::Spectate { conn: 3, id });
    assert_eq!(mgr.rooms[&id].spectators.len(), 1);
    mgr.handle(Command::JoinRoom { conn: 3, id });
    assert_eq!(mgr.rooms[&id].members.len(), 2);
    assert_eq!(mgr.rooms[&id].spectators.len(), 1, "still watching");
}

#[test]
fn a_spectator_takes_a_free_seat() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let _rx3 = spectator(&mut mgr, 3);
    mgr.handle(Command::JoinRoom { conn: 3, id: 1 });
    assert_eq!(mgr.rooms[&1].spectators.len(), 1, "no seat while both play");
    mgr.handle(Command::LeaveRoom { conn: 2 });
    assert!(matches!(mgr.rooms[&1].phase, Phase::Lobby));
    mgr.handle(Command::JoinRoom { conn: 3, id: 1 });
    let room = &mgr.rooms[&1];
    assert!(room.spectators.is_empty());
    assert_eq!(room.slot_of_conn(3), Some(1));
    assert_eq!(room.members[1].name, "Invité");
}

#[test]
fn watching_a_friend_finds_their_room_after_checking_the_friendship() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello_as(&mut mgr, 1, "A", 1);
    let mut rx4 = reg(&mut mgr, 4);
    hello_as(&mut mgr, 4, "F", 4);
    let friend = Uuid::from_u128(1).to_string();
    mgr.handle(Command::WatchFriend {
        conn: 4,
        user_id: friend.clone(),
    });
    assert!(
        has(&drain(&mut rx4), |m| matches!(m, ServerMessage::JoinFailed { .. })),
        "not playing"
    );

    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    mgr.handle(Command::WatchFriend {
        conn: 4,
        user_id: friend,
    });
    let check = watch_check(&mut mgr);
    assert_eq!(check.users(), (Uuid::from_u128(4), Uuid::from_u128(1)));
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert_eq!(mgr.room_of(4), Some(1));
    assert_eq!(mgr.rooms[&1].spectators.len(), 1);
}

#[test]
fn chat_stays_in_the_room_and_is_rate_limited() {
    let mut mgr = new_mgr();
    let (_rx1, mut rx2) = running_game(&mut mgr);
    let mut rx3 = spectator(&mut mgr, 3);
    let mut outside = reg(&mut mgr, 9);
    hello(&mut mgr, 9, "Z");
    for rx in [&mut rx2, &mut rx3, &mut outside] {
        drain(rx);
    }
    mgr.handle(Command::Chat {
        conn: 1,
        text: "  salut\tà tous \u{7}".into(),
    });
    let chats = |rx: &mut mpsc::Receiver<Vec<u8>>| {
        drain(rx)
            .into_iter()
            .filter_map(|m| match m {
                ServerMessage::Chat { from, text, spectator } => Some((from, text, spectator)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        chats(&mut rx2),
        [("Invité".to_owned(), "salut à tous".to_owned(), false)]
    );
    assert_eq!(chats(&mut rx3).len(), 1);
    assert!(chats(&mut outside).is_empty());

    mgr.handle(Command::Chat {
        conn: 3,
        text: "vu".into(),
    });
    assert!(chats(&mut rx2)[0].2, "marked as a spectator's");
    for i in 0..20 {
        mgr.handle(Command::Chat {
            conn: 1,
            text: format!("spam {i}"),
        });
    }
    let got = chats(&mut rx2).len();
    assert!((4..=5).contains(&got), "{got} messages got through a burst of 20");
}

#[test]
fn a_room_name_is_one_printable_line() {
    use crate::manager::rooms::clean_name;
    assert_eq!(clean_name("ma\nroom\u{7}"), "ma room");
    assert_eq!(clean_name(" \u{0} "), "Room");
    assert_eq!(clean_name(&"x".repeat(40)).len(), 24);
}

#[test]
fn chat_text_is_one_bounded_printable_line() {
    assert_eq!(clean_chat("a\nb\r\nc"), Some("a b  c".to_owned()));
    assert_eq!(clean_chat(" \u{0}\u{7f} "), None);
    assert_eq!(
        clean_chat(&"é".repeat(500)).map(|s| s.chars().count()),
        Some(shared::MAX_CHAT_CHARS)
    );
}

#[test]
fn a_rename_shows_in_the_next_invitation() {
    let mut mgr = new_mgr();
    let mut rx2 = inviter_and_target(&mut mgr);
    mgr.handle(Command::Rename {
        user_id: Uuid::from_u128(1),
        username: "Alicia".into(),
    });
    invite_b(&mut mgr);
    let check = mgr.take_friend_checks().pop().expect("an invite check");
    mgr.handle(Command::FriendCheckDone { check, friends: true });
    assert!(has(&drain(&mut rx2), |m| matches!(
        m,
        ServerMessage::FriendInvitation { from_username, .. } if from_username == "Alicia"
    )));
}
