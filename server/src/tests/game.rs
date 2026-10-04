use super::*;

pub(super) fn last_update_tick(rx: &mut mpsc::Receiver<Vec<u8>>) -> Option<u32> {
    drain(rx).into_iter().rev().find_map(|m| match m {
        ServerMessage::StateUpdate { tick, .. } => Some(tick),
        _ => None,
    })
}

pub(super) const STEP: f32 = 1.0 / config::SERVER_TICK_HZ as f32;

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

pub(super) fn piece_col(mgr: &Manager, slot: usize) -> i32 {
    mgr.rooms[&1].sim.boards[slot]
        .active_piece
        .as_ref()
        .expect("a piece must be falling")
        .col
}

pub(super) fn press(mgr: &mut Manager, conn: ConnId, seq: u32, tick: u32) {
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

pub(super) fn sim_of(mgr: &mut Manager) -> &mut Sim {
    &mut mgr.rooms.get_mut(&1).expect("room 1").sim
}

pub(super) fn attack_on_next_tick(mgr: &mut Manager) {
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

pub(super) fn update_rngs(rx: &mut mpsc::Receiver<Vec<u8>>) -> Option<(bool, bool)> {
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
