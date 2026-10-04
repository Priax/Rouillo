use super::*;

pub(super) fn hello_as(mgr: &mut Manager, conn: ConnId, token: &str, user: u128) {
    mgr.handle(Command::Hello {
        conn,
        token: token.to_string(),
        user_id: Some(Uuid::from_u128(user)),
        username: Some(token.to_lowercase()),
        session: None,
        last_disconnect_reason: None,
    });
}

pub(super) fn friends_only_room(mgr: &mut Manager) -> (mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
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

pub(super) fn only_join_check(mgr: &mut Manager) -> FriendCheck {
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
        name: "K".into(),
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

pub(super) fn inviter_and_target(mgr: &mut Manager) -> mpsc::Receiver<Vec<u8>> {
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

pub(super) fn invite_b(mgr: &mut Manager) {
    mgr.handle(Command::InviteFriend {
        conn: 1,
        target_user_id: Uuid::from_u128(2).to_string(),
    });
}

pub(super) fn invitations(rx: &mut mpsc::Receiver<Vec<u8>>) -> usize {
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
