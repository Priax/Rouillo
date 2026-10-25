#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    reason = "test fixtures build small, known values"
)]

use std::collections::HashSet;

use super::*;
use crate::config::{self, GRACE_FRAMES, GRID_HEIGHT, GRID_WIDTH, SPAWN_COL, VISIBLE_ROW_OFFSET};
use crate::room::RoomSettings;

fn empty_board() -> Board {
    Board::new(1, &Rules::default())
}

fn falls(b: &Board) -> &[CellFall] {
    match b.phase() {
        Phase::Falling(fall) => &fall.cells,
        _ => &[],
    }
}

fn falling_frames(b: &Board) -> u32 {
    let Phase::Falling(fall) = b.phase() else {
        panic!("not falling: {:?}", b.phase());
    };
    fall.frames
}

fn piece(row: i32, col: i32, rotation: Rotation) -> ActivePuyo {
    ActivePuyo {
        row,
        col,
        rotation,
        axis_type: PuyoType::Red,
        sat_type: PuyoType::Blue,
    }
}

#[test]
fn same_seed_yields_identical_piece_sequence() {
    let mut a = Board::new(42, &Rules::default());
    let mut b = Board::new(42, &Rules::default());
    assert_eq!(a.next_types, b.next_types);
    assert_eq!(a.next_next_types, b.next_next_types);
    for _ in 0..50 {
        a.spawn_piece();
        b.spawn_piece();
        let pa = a.take_piece().expect("piece a");
        let pb = b.take_piece().expect("piece b");
        assert_eq!((pa.axis_type, pa.sat_type), (pb.axis_type, pb.sat_type));
    }
}

#[test]
fn garbage_placement_leaves_the_piece_sequence_alone() {
    // Both players share a seed; only one takes nuisance. The leftover columns are
    // drawn at random, and that draw must not shift the pairs they are dealt.
    let mut a = Board::new(42, &Rules::default());
    let mut b = Board::new(42, &Rules::default());
    for i in 0..50 {
        if i % 5 == 0 {
            b.cells.rows_mut().for_each(|row| row.fill(None));
            b.pending_garbage = 3; // not a multiple of 6: the leftover columns are drawn
            b.drop_pending_garbage();
        }
        a.spawn_piece();
        b.spawn_piece();
        let pa = a.take_piece().expect("piece a");
        let pb = b.take_piece().expect("piece b");
        assert_eq!((pa.axis_type, pa.sat_type), (pb.axis_type, pb.sat_type), "pair {i}");
    }
    let draws = |x: &Board| x.rng.garbage;
    assert_ne!(draws(&a), draws(&b), "setup: the drops drew nothing");
}

#[test]
fn the_default_room_plays_on_the_standard_board() {
    let board = Board::for_match(1, &RoomSettings::default().rules);
    assert_eq!(
        (board.width(), board.height(), board.spawn_col()),
        (GRID_WIDTH, GRID_HEIGHT, SPAWN_COL)
    );
}

#[test]
fn a_custom_room_sets_the_board_size_and_spawns_in_the_middle() {
    let rules = Rules {
        columns: 9,
        rows: 15,
        ..Rules::default()
    };
    let board = Board::for_match(1, &rules);
    assert_eq!((board.width(), board.height()), (9, 16));
    assert_eq!(board.piece().map(|p| p.col), Some(4));
}

#[test]
fn four_connected_same_color_clears() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    }
    assert_eq!(b.check_matches(), Some(40)); // chain 1, group of 4, no bonus -> 10*4*1
    for r in 9..=12 {
        assert!(b.cells[(r, 0)].is_none());
    }
    assert_eq!(b.chain_count, 1);
}

#[test]
fn three_connected_does_not_clear() {
    let mut b = empty_board();
    for r in 10..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    }
    assert_eq!(b.check_matches(), None);
    for r in 10..=12 {
        assert!(b.cells[(r, 0)].is_some());
    }
}

#[test]
fn group_only_in_hidden_row_does_not_clear() {
    let mut b = empty_board();
    b.cells.rows_mut().next().unwrap()[..4].fill(Some(PuyoType::Red));
    assert_eq!(b.check_matches(), None);
}

#[test]
fn adjacent_garbage_is_cleared() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    }
    b.cells[(9, 1)] = Some(PuyoType::Garbage); // touches the group
    b.cells[(5, 5)] = Some(PuyoType::Garbage); // far away
    b.check_matches();
    assert!(b.cells[(9, 1)].is_none(), "adjacent garbage should clear");
    assert_eq!(
        b.cells[(5, 5)],
        Some(PuyoType::Garbage),
        "distant garbage should remain"
    );
}

#[test]
fn garbage_does_not_self_match() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Garbage);
    }
    assert_eq!(b.check_matches(), None);
}

#[test]
fn gravity_drops_floating_puyo_to_floor() {
    let mut b = empty_board();
    b.cells[(3, 0)] = Some(PuyoType::Red);
    assert!(b.apply_board_gravity());
    assert!(b.cells[(3, 0)].is_none());
    assert_eq!(b.cells[(GRID_HEIGHT - 1, 0)], Some(PuyoType::Red));
}

#[test]
fn gravity_noop_when_settled() {
    let mut b = empty_board();
    b.cells[(GRID_HEIGHT - 1, 0)] = Some(PuyoType::Red);
    b.cells[(GRID_HEIGHT - 2, 0)] = Some(PuyoType::Blue);
    assert!(!b.apply_board_gravity());
}

// 70 points of clears == 1 garbage puyo sent; the remainder carries over to
// the next clear. Getting this wrong makes attacks unfair.
#[test]
fn a_pop_converts_score_to_garbage_with_carry() {
    let mut b = empty_board();
    for r in 8..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    } // group of 5 -> score 100
    b.settle();
    assert_eq!(b.tick().sent, 1); // floor(100 / 70)
    assert_eq!(b.nuisance_points, 30); // 100 % 70 carried for the next clear
}

#[test]
fn rotation_in_open_space_just_turns() {
    let mut b = empty_board();
    b.set_piece(piece(6, 2, Rotation::Up));
    b.apply_input(InputKind::RotateCW);
    let p = *b.piece().unwrap();
    assert_eq!((p.row, p.col, p.rotation), (6, 2, Rotation::Right));
}

#[test]
fn rotation_against_right_wall_kicks_left() {
    let mut b = empty_board();
    b.set_piece(piece(6, (GRID_WIDTH - 1) as i32, Rotation::Up));
    b.apply_input(InputKind::RotateCW);
    let p = *b.piece().unwrap();
    assert_eq!((p.col, p.rotation), ((GRID_WIDTH - 2) as i32, Rotation::Right));
}

#[test]
fn rotation_against_left_wall_kicks_right() {
    let mut b = empty_board();
    b.set_piece(piece(6, 0, Rotation::Up));
    b.apply_input(InputKind::RotateCCW);
    let p = *b.piece().unwrap();
    assert_eq!((p.col, p.rotation), (1, Rotation::Left));
}

#[test]
fn grounded_rotation_floor_kicks_up() {
    let mut b = empty_board();
    b.set_piece(piece((GRID_HEIGHT - 1) as i32, 2, Rotation::Left));
    b.apply_input(InputKind::RotateCCW); // -> rotation 2 (satellite below) into the floor, kicks up
    let p = *b.piece().unwrap();
    assert_eq!((p.row, p.rotation), ((GRID_HEIGHT - 2) as i32, Rotation::Down));
}

#[test]
fn piece_locks_after_its_grace_period() {
    let mut b = empty_board();
    b.set_piece(piece((GRID_HEIGHT - 1) as i32, 2, Rotation::Up));
    for _ in 0..GRACE_FRAMES {
        b.tick();
    }
    assert!(b.piece().is_some(), "locked before its grace period ran out");
    b.tick();
    assert!(b.piece().is_none());
    assert_eq!(b.cells[(GRID_HEIGHT - 1, 2)], Some(PuyoType::Red)); // axis
    assert_eq!(b.cells[(GRID_HEIGHT - 2, 2)], Some(PuyoType::Blue)); // satellite
    assert_eq!(b.state(), GameState::ResolvingMatches);
}

// As in Tsu, the grace period is a total over the pair's life: sliding along
// the floor does not buy more time.
#[test]
fn moving_on_the_ground_does_not_extend_the_grace_period() {
    let mut b = empty_board();
    b.set_piece(piece((GRID_HEIGHT - 1) as i32, 2, Rotation::Up));
    for _ in 0..GRACE_FRAMES / 2 {
        b.tick();
    }
    b.apply_input(InputKind::MoveLeft);
    for _ in 0..=(GRACE_FRAMES / 2) {
        b.tick();
    }
    assert!(b.piece().is_none(), "the move reset the grace period");
}

#[test]
fn soft_dropping_onto_something_locks_at_once() {
    let mut b = empty_board();
    b.set_piece(piece((GRID_HEIGHT - 1) as i32, 2, Rotation::Up));
    b.apply_input(InputKind::SoftDropPress);
    b.tick();
    assert!(b.piece().is_none());
}

#[test]
fn soft_drop_takes_two_frames_per_cell_and_the_natural_fall_sixteen() {
    let rows_after = |frames: u32, soft: bool| {
        let mut b = empty_board();
        b.set_piece(piece(2, 2, Rotation::Up));
        if soft {
            b.apply_input(InputKind::SoftDropPress);
        }
        for _ in 0..frames {
            b.tick();
        }
        b.piece().expect("still falling").row - 2
    };
    assert_eq!(rows_after(8, true), 4);
    assert_eq!(rows_after(32, false), 2);
}

#[test]
fn free_fall_matches_the_tsu_table() {
    let tsu = [10, 15, 19, 22, 25, 28, 31, 33, 35, 37, 39, 41, 43];
    for (cells, frames) in (1..).zip(tsu) {
        assert_eq!(
            frames_to_fall(cells, config::FREE_FALL_START, config::FREE_FALL_ACCEL),
            frames,
            "{cells} cells"
        );
    }
}

#[test]
fn ojama_fall_matches_the_tsu_table_in_every_column() {
    let tsu: [[u32; 13]; 6] = [
        [16, 22, 27, 31, 35, 38, 41, 44, 46, 49, 51, 53, 55],
        [16, 22, 26, 30, 34, 37, 40, 43, 45, 47, 50, 52, 54],
        [17, 24, 29, 33, 37, 40, 43, 46, 49, 52, 54, 56, 59],
        [15, 21, 25, 29, 32, 35, 38, 41, 43, 45, 47, 49, 51],
        [17, 23, 28, 32, 36, 39, 42, 45, 48, 50, 52, 55, 57],
        [15, 21, 26, 30, 33, 36, 39, 41, 44, 46, 48, 51, 53],
    ];
    for (col, frames) in tsu.iter().enumerate() {
        for (cells, &expected) in (1..).zip(frames) {
            assert_eq!(
                frames_to_fall(cells, 0, config::OJAMA_ACCEL[col]),
                expected,
                "column {col}, {cells} cells"
            );
        }
    }
}

#[test]
fn faster_drops_earn_a_point_per_cell() {
    let mut b = empty_board();
    b.set_piece(piece(1, 2, Rotation::Up));
    b.apply_input(InputKind::HardDrop);
    let cells = (GRID_HEIGHT - 2) as u32;
    assert_eq!((b.score, b.nuisance_points), (cells, cells));

    let mut b = empty_board();
    b.set_piece(piece(1, 2, Rotation::Up));
    b.apply_input(InputKind::SoftDropPress);
    while b.piece().is_some() {
        b.tick();
    }
    assert_eq!((b.score, b.nuisance_points), (cells, cells));

    let mut b = empty_board();
    b.set_piece(piece(1, 2, Rotation::Up));
    while b.piece().is_some() {
        b.tick();
    }
    assert_eq!((b.score, b.nuisance_points), (0, 0));
}

/// A pair locked with its satellite hanging over a hole: the satellite falls
/// with gravity, the chain check waits for it to land and bounce.
#[test]
fn a_split_pair_falls_before_anything_pops() {
    let mut b = empty_board();
    // Satellite to the right of the axis, over an empty column.
    b.cells[(GRID_HEIGHT - 1, 2)] = Some(PuyoType::Green);
    b.set_piece(piece((GRID_HEIGHT - 2) as i32, 2, Rotation::Right));
    b.apply_input(InputKind::HardDrop);
    assert_eq!(b.state(), GameState::ResolvingMatches);
    let hanging = falls(&b).iter().find(|f| f.col == 3).expect("the satellite falls");
    assert_eq!(hanging.cells_fallen, 1);
    let frames = falling_frames(&b);
    assert_eq!(
        frames,
        config::SPLIT_DELAY_SATELLITE as u32 + 10 + config::BOUNCE_FRAMES
    );
    for _ in 0..frames - 1 {
        b.tick();
        assert_eq!(b.state(), GameState::ResolvingMatches);
    }
    b.tick();
    assert_eq!(b.state(), GameState::Playing, "the next pair never came");
}

#[test]
fn a_group_flashes_before_it_vanishes() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    }
    b.settle();
    b.tick();
    assert!(matches!(b.phase(), Phase::Popping { cells, .. } if cells.len() == 4));
    for _ in 0..config::POP_FRAMES - 1 {
        b.tick();
    }
    assert_eq!(b.cells[(12, 0)], Some(PuyoType::Red), "cleared before the flash ended");
    b.tick();
    assert_eq!(b.cells[(12, 0)], None);
}

#[test]
fn spawning_into_blocked_column_is_game_over() {
    let mut b = empty_board();
    b.cells[(VISIBLE_ROW_OFFSET, 2)] = Some(PuyoType::Red);
    b.spawn_piece();
    assert_eq!(b.state(), GameState::GameOver);
}

// ---- Determinism ---------------------------------------------------------
//
// The whole point of the digest: a board replayed elsewhere from the same seed
// and the same inputs must land on the same state, tick for tick. Everything a
// tick-stamped protocol could later be built on rests on that holding.

/// A deterministic input schedule. Not random play, just enough variety to
/// reach locks, chains, garbage drops and game overs.
fn scripted_input(step: u64) -> Option<InputKind> {
    // SplitMix64, so the schedule depends only on `step` and never on a
    // sequence of calls: a caller can jump anywhere in the script.
    let mut z = step.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    match (z ^ (z >> 31)) % 12 {
        0 => Some(InputKind::MoveLeft),
        1 => Some(InputKind::MoveRight),
        2 => Some(InputKind::RotateCW),
        3 => Some(InputKind::RotateCCW),
        4 => Some(InputKind::SoftDropPress),
        6 => Some(InputKind::SoftDropRelease),
        5 => Some(InputKind::HardDrop),
        _ => None,
    }
}

struct Run {
    /// One digest per tick, so a divergence is located rather than just seen.
    hashes: Vec<u64>,
    boards: u32,
    max_chain: u32,
    garbage_dropped: u32,
}

/// Plays `ticks` fixed steps of the script. A board that tops out is replaced
/// with a fresh one from a derived seed, so the run keeps exercising spawning,
/// locking, chains and garbage instead of freezing on the first game over.
/// A seed whose script reaches a chain of 3 within 6000 ticks: random play
/// rarely chains, and the tests below need the resolution path exercised.
const SCRIPT_SEED: u64 = 628;

fn run_script(seed: u64, ticks: u64, skip_input_at: Option<u64>) -> Run {
    let fresh = |s: u64| {
        let mut b = Board::new(s, &Rules::default());
        b.spawn_piece();
        b
    };
    let mut board = fresh(seed);
    let mut run = Run {
        hashes: Vec::with_capacity(ticks as usize),
        boards: 1,
        max_chain: 0,
        garbage_dropped: 0,
    };

    for step in 0..ticks {
        if board.state() == GameState::GameOver {
            board = fresh(seed.wrapping_add(run.boards as u64));
            run.boards += 1;
        }
        if Some(step) != skip_input_at {
            if let Some(input) = scripted_input(step) {
                board.apply_input(input);
            }
        }
        // Hand it nuisance regularly: `drop_garbage` draws from its own RNG stream, so
        // a script that never took that path would not prove much about it.
        if step % 240 == 239 {
            board.pending_garbage += 9;
            run.garbage_dropped += 9;
        }
        board.tick();
        run.max_chain = run.max_chain.max(board.chain_count);
        run.hashes.push(board.state_hash());
    }
    run
}

/// Each step reports exactly what its state changes show, over a long game
/// with chains, nuisance and top outs.
#[test]
fn events_tell_what_the_board_shows() {
    let fresh = |s: u64| Board::for_match(s, &Rules::default());
    let mut board = fresh(SCRIPT_SEED);
    let mut boards = 1;
    let mut seen = Events::default();
    let (mut links, mut longest) = (0, 0);
    for step in 0..6_000u64 {
        if board.state() == GameState::GameOver {
            board = fresh(SCRIPT_SEED + boards);
            boards += 1;
        }
        if step % 240 == 239 {
            board.pending_garbage += 9;
        }
        let before = board.clone();
        let events = board.step(scripted_input(step), 0);
        let became = |state: GameState| board.state() == state && before.state() != state;
        assert_eq!(events.spawned, board.piece_id != before.piece_id, "spawned at {step}");
        assert_eq!(events.topped_out, became(GameState::GameOver), "topped out at {step}");
        assert_eq!(
            events.ojama_dropped,
            became(GameState::DroppingGarbage),
            "ojama at {step}"
        );
        assert_eq!(
            events.locked,
            before.pair().is_some() && matches!(board.phase(), Phase::Falling(fall) if !fall.ojama),
            "locked at {step}"
        );
        let link = (board.chain_count > before.chain_count).then_some(board.chain_count);
        assert_eq!(events.link, link, "link at {step}");
        assert_eq!(
            events.all_clear,
            board.last_was_all_clear && !before.last_was_all_clear,
            "all clear at {step}"
        );
        links += u32::from(events.link.is_some());
        longest = longest.max(events.link.unwrap_or(0));
        seen.merge(events);
    }
    assert!(seen.locked && seen.spawned && seen.ojama_dropped && seen.topped_out && seen.sent > 0);
    assert!(links > 5 && longest >= 3, "{links} links, the longest chain {longest}");
}

fn first_divergence(a: &[u64], b: &[u64]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y)
}

/// The core guarantee. Running the identical script twice must produce the
/// identical sequence of states.
///
/// Less tautological than it looks: each run builds its own `HashSet`s inside
/// `check_matches` and `flood_fill`, and `RandomState` seeds every instance
/// differently. If clearing order ever leaked into the result (through the
/// score, the RNG draw order, anything), the two runs would part company here.
#[test]
fn the_same_script_replays_to_the_same_states() {
    let a = run_script(SCRIPT_SEED, 6_000, None);
    let b = run_script(SCRIPT_SEED, 6_000, None);
    assert_eq!(
        first_divergence(&a.hashes, &b.hashes),
        None,
        "two identical runs diverged"
    );
}

/// The script has to actually reach the interesting code, or the test above
/// proves only that an idle board stays idle.
#[test]
fn the_script_exercises_the_whole_simulation() {
    let run = run_script(SCRIPT_SEED, 6_000, None);
    assert!(run.max_chain >= 2, "no chain longer than {} occurred", run.max_chain);
    assert!(run.garbage_dropped > 0, "garbage was never dropped");
    assert!(run.boards >= 2, "no board ever topped out, spawning is under-covered");
    let distinct = run.hashes.iter().collect::<HashSet<_>>().len();
    assert!(distinct > 1_000, "only {distinct} distinct states in 6000 ticks");
}

/// The detector has to be able to fail: dropping a single input, once, must
/// show up, and must not be silently absorbed.
#[test]
fn one_dropped_input_diverges() {
    // Skipping a step the schedule leaves empty would prove nothing, so pick a
    // real one: a hard drop, which cannot fail to change the board.
    let at = (0..100)
        .find(|s| scripted_input(*s) == Some(InputKind::HardDrop))
        .expect("the schedule never hard drops");
    let base = run_script(SCRIPT_SEED, 6_000, None);
    let altered = run_script(SCRIPT_SEED, 6_000, Some(at));
    let diverged = first_divergence(&base.hashes, &altered.hashes).expect("a dropped input went unnoticed");
    assert_eq!(
        diverged as u64, at,
        "the divergence should surface on the very tick it was caused"
    );
}

/// Pins the script's outcome to a literal.
///
/// The two tests above compare a run against another run in the same process,
/// which cannot see a difference between *machines*. This one can: the server
/// is aarch64 and the clients are x86-64 and wasm32, and the tick-stamped
/// protocol this is all groundwork for assumes those three agree bit for bit.
/// Run `cargo test -p shared` on each to confirm it.
///
/// It therefore fails whenever the simulation changes, deliberately or not. If
/// the change was intended, re-read the diff, then paste the new value in.
const GOLDEN_FINAL_HASH: u64 = 5_837_063_029_504_399_203;

#[test]
fn scripted_run_matches_its_recorded_outcome() {
    let run = run_script(SCRIPT_SEED, 6_000, None);
    assert_eq!(
        run.hashes.last().copied(),
        Some(GOLDEN_FINAL_HASH),
        "the simulation's behaviour changed (or this platform disagrees)"
    );
}

/// The scripted run never pauses and never sets an all clear, so a few fields
/// hold one value throughout it and the tests above say nothing about them. The
/// compiler guarantees each is *hashed*; this checks each actually moves the
/// digest.
#[test]
fn the_digest_notices_fields_the_script_never_varies() {
    let base = empty_board();

    let mut paused = base.clone();
    paused.set_paused(true);
    assert_ne!(paused.state_hash(), base.state_hash(), "paused");

    let mut all_clear = base.clone();
    all_clear.last_was_all_clear = true;
    assert_ne!(all_clear.state_hash(), base.state_hash(), "last_was_all_clear");

    // Room settings reach the simulation through the fall speed and the piece
    // palette, so two boards set up differently must not look alike either.
    let faster = Board::new(
        1,
        &Rules {
            starting_level: 4,
            ..Rules::default()
        },
    );
    assert_ne!(faster.state_hash(), base.state_hash(), "start_level");
    let four_colours = Board::new(
        1,
        &Rules {
            colors: 4,
            ..Rules::default()
        },
    );
    assert_ne!(four_colours.state_hash(), base.state_hash(), "colors");
}

/// What the server does with one board: a queue of stamped inputs drained by
/// the prefix rule, attacks landing on their tick, `Board::step` in between.
/// `states[t]` is the state after tick t, tagged with which game it belongs
/// to: like `run_script`, a board that tops out is replaced so the timeline
/// keeps reaching chains and garbage.
/// Same idea for the replay timeline, which plays its script differently and
/// so needs a seed of its own that chains.
const TIMELINE_SEED: u64 = 4;

fn server_timeline(ticks: u32, inputs: &[StampedInput], incoming: &[IncomingGarbage]) -> Vec<(u32, Board)> {
    timeline_from(TIMELINE_SEED, ticks, inputs, incoming)
}

fn timeline_from(seed: u64, ticks: u32, inputs: &[StampedInput], incoming: &[IncomingGarbage]) -> Vec<(u32, Board)> {
    let fresh = |game: u32| {
        let mut b = Board::new(seed + game as u64, &Rules::default());
        b.spawn_piece();
        b
    };
    let mut game = 0;
    let mut board = fresh(game);
    let mut states = vec![(game, board.clone())];
    let mut next = 0;
    for t in 1..=ticks {
        if board.state() == GameState::GameOver {
            game += 1;
            board = fresh(game);
        }
        let start = next;
        while next < inputs.len() && inputs[next].tick <= t {
            next += 1;
        }
        let landed = incoming.iter().filter(|g| g.at == t).map(|g| g.amount).sum();
        board.step(inputs[start..next].iter().map(|i| i.kind), landed);
        states.push((game, board.clone()));
    }
    states
}

fn scripted_stamps(ticks: u32) -> Vec<StampedInput> {
    (1..=ticks)
        .filter_map(|t| scripted_input(t as u64 - 1).map(|kind| StampedInput { tick: t, kind }))
        .collect()
}

fn scripted_attacks(ticks: u32) -> Vec<IncomingGarbage> {
    (1..=ticks)
        .filter(|t| t % 240 == 0)
        .map(|at| IncomingGarbage { at, amount: 9 })
        .collect()
}

/// The contract stage 5 rests on: from the server's state after any tick,
/// replaying the inputs and attacks not yet in it reaches, bit for bit, the
/// state the server will be in. Otherwise every update would correct the
/// client's own board, however good the connection.
#[test]
fn replaying_from_any_server_state_lands_on_the_server_future() {
    let ticks = 6_000;
    let inputs = scripted_stamps(ticks);
    let attacks = scripted_attacks(ticks);
    let states = server_timeline(ticks, &inputs, &attacks);
    assert!(
        states.iter().any(|(_, b)| b.chain_count >= 2),
        "the timeline never chained, the replay is under-tested"
    );
    assert!(
        states.iter().any(|(_, b)| b.state() == GameState::DroppingGarbage),
        "the timeline never dropped garbage, the replay is under-tested"
    );

    let mut checked = 0;
    for from in (0..ticks - 40).step_by(3) {
        for ahead in [0, 1, 5, 17, 40] {
            let to = from + ahead;
            let (game, start) = &states[from as usize];
            if states[to as usize].0 != *game {
                continue; // a replay cannot see a restart coming
            }
            // What the client still holds: everything the server has not
            // applied by `from`, and every attack not landed by then.
            let pending: Vec<_> = inputs.iter().copied().filter(|i| i.tick > from).collect();
            let in_flight: Vec<_> = attacks.iter().copied().filter(|g| g.at > from).collect();
            let replayed = start.replay(from, to, &pending, &in_flight);
            // Inputs stamped past `to` sit on top of the replay without a tick.
            let mut expected = states[to as usize].1.clone();
            for i in pending.iter().filter(|i| i.tick > to) {
                expected.apply_input(i.kind);
            }
            assert_eq!(
                replayed.state_hash(),
                expected.state_hash(),
                "replay from tick {from} to {to} left the server's timeline"
            );
            checked += 1;
        }
    }
    assert!(checked > 5_000, "only {checked} replays were comparable");
}

#[test]
fn a_late_input_replays_on_the_first_tick_after_the_snapshot() {
    let states: Vec<_> = server_timeline(10, &[], &[]).into_iter().map(|(_, b)| b).collect();
    let late = [StampedInput {
        tick: 3,
        kind: InputKind::MoveLeft,
    }];
    let replayed = states[8].replay(8, 10, &late, &[]);

    let mut expected = states[8].clone();
    expected.step([InputKind::MoveLeft], 0);
    expected.step([], 0);
    assert_eq!(replayed.state_hash(), expected.state_hash());
}

#[test]
fn an_input_pressed_after_the_last_step_is_applied_without_a_tick() {
    let states: Vec<_> = server_timeline(5, &[], &[]).into_iter().map(|(_, b)| b).collect();
    let pressed = [StampedInput {
        tick: 6,
        kind: InputKind::MoveRight,
    }];
    let replayed = states[5].replay(5, 5, &pressed, &[]);

    let mut expected = states[5].clone();
    expected.apply_input(InputKind::MoveRight);
    assert_eq!(replayed.state_hash(), expected.state_hash());
}

#[test]
fn an_attack_lands_in_the_replay_on_its_own_tick() {
    let states: Vec<_> = server_timeline(10, &[], &[]).into_iter().map(|(_, b)| b).collect();
    let attack = [IncomingGarbage { at: 7, amount: 4 }];
    assert_eq!(states[5].replay(5, 6, &[], &attack).pending_garbage, 0);
    assert_eq!(states[5].replay(5, 7, &[], &attack).pending_garbage, 4);
}

/// The drawn height of the falling piece: its row plus how far it is towards
/// the next one.
fn drawn_row(b: &Board) -> f32 {
    b.piece().expect("a piece").row as f32 + b.fall_progress()
}

/// Left alone, a piece must glide: never move up, never jump. Across a
/// natural drop the row goes up by one while the progress falls back to 0,
/// and the two must cancel out.
#[test]
fn a_falling_piece_is_drawn_gliding_down() {
    let mut b = Board::new(3, &Rules::default());
    b.spawn_piece();
    let start_row = b.piece().expect("a piece").row;
    let mut prev = drawn_row(&b);
    let per_tick = 1.0 / config::FALL_FRAMES_PER_CELL as f32;
    while b.piece().is_some_and(|p| p.row < start_row + 3) {
        b.tick();
        let now = drawn_row(&b);
        assert!(now >= prev - 1e-4, "the piece went up: {prev} -> {now}");
        assert!(now - prev <= 2.0 * per_tick + 1e-4, "the piece jumped: {prev} -> {now}");
        prev = now;
    }
}

#[test]
fn a_resting_piece_is_drawn_on_its_cell() {
    let mut b = Board::new(3, &Rules::default());
    b.spawn_piece();
    b.piece_mut().expect("a piece").row = GRID_HEIGHT as i32 - 1;
    b.pair_mut().expect("a pair").fall_offset = config::HALF_CELL_UNITS; // mid-way, had it been able to fall
    assert_eq!(b.fall_progress(), 0.0);
}

/// A group of five reds at the bottom of column 0, and nothing else.
fn board_about_to_pop() -> Board {
    let mut b = empty_board();
    for r in 8..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    } // score 100 -> 1 nuisance
    b.settle();
    b
}

#[test]
fn a_chain_cancels_waiting_nuisance_before_attacking() {
    let mut b = board_about_to_pop();
    b.pending_garbage = 5;
    b.nuisance_points = 69; // 100 + 69 = 169 -> 2 nuisance
    assert_eq!(b.tick().sent, 0, "sent while nuisance was still waiting");
    assert_eq!(b.pending_garbage, 3);
}

#[test]
fn only_the_surplus_of_an_offset_is_sent() {
    let mut b = board_about_to_pop();
    b.pending_garbage = 1;
    b.nuisance_points = 69;
    assert_eq!(b.tick().sent, 1);
    assert_eq!(b.pending_garbage, 0);
}

#[test]
fn an_all_clear_pays_out_with_the_next_chain() {
    let mut b = empty_board();
    b.settle();
    let events = b.tick();
    assert_eq!(events.sent, 0, "an all clear was sent on its own");
    assert!(events.all_clear && events.spawned);
    assert!(b.last_was_all_clear && b.all_clear_bonus);

    let mut b = board_about_to_pop();
    b.all_clear_bonus = true;
    assert_eq!(b.tick().sent, 1 + config::ALL_CLEAR_BONUS);
    assert!(!b.all_clear_bonus, "the bonus was paid twice");
}

/// Nuisance that does not fit is lost; a full column is no reason to lose on
/// the spot, while the nuisance is still falling and nobody has seen it.
#[test]
fn nuisance_into_a_full_column_does_not_end_the_game_mid_drop() {
    let mut b = empty_board();
    for row in b.cells.rows_mut() {
        row[0] = Some(PuyoType::Red);
    }
    b.pending_garbage = 30;
    b.drop_pending_garbage();
    assert_ne!(b.state(), GameState::GameOver);
    assert!(falls(&b).iter().all(|f| f.col != 0), "nuisance landed in a full column");
    assert_eq!(falls(&b).len(), 25, "the other five columns take five rows each");
    assert_eq!(b.pending_garbage, 0, "what did not fit was kept for later");
}

/// Buried under nuisance, the board loses only once the drop is over and the
/// next pair has nowhere to appear.
#[test]
fn a_board_buried_by_nuisance_loses_after_the_drop_is_seen() {
    let mut b = empty_board();
    for row in b.cells.rows_mut().skip(3) {
        row.fill(Some(PuyoType::Garbage));
    }
    b.pending_garbage = 30;
    b.drop_pending_garbage();
    let frames = falling_frames(&b);
    assert!(frames > 0);
    for _ in 0..frames - 1 {
        b.tick();
        assert_ne!(b.state(), GameState::GameOver, "lost before the nuisance had landed");
    }
    b.tick();
    assert_eq!(b.state(), GameState::GameOver);
}

fn at_level(level: u32) -> Board {
    let mut b = empty_board();
    b.match_frames = (level - 1) * config::LEVEL_FRAMES;
    assert_eq!(b.level(), level);
    b
}

#[test]
fn margin_time_follows_the_level() {
    assert_eq!(at_level(1).target_points(), 70);
    assert_eq!(at_level(6).target_points(), 70, "reduced before the margin ran out");
    assert_eq!(at_level(7).target_points(), 52, "70 x 3/4");
    assert_eq!(at_level(8).target_points(), 39, "70 x (3/4)^2");
    assert_eq!(at_level(20).target_points(), 1, "fourteen steps down");
    assert_eq!(at_level(99).target_points(), 1, "never below 1");
}

/// A room started at a high level begins with its margin already spent.
#[test]
fn the_starting_level_counts_towards_the_margin() {
    let b = Board::new(
        1,
        &Rules {
            starting_level: 8,
            ..Rules::default()
        },
    );
    assert_eq!(b.target_points(), 39);
}

/// With the target fully run down, a plain 4-puyo pop (40 points) sends 40
/// nuisance: the figure quoted for Tsu.
#[test]
fn a_fully_run_down_margin_makes_a_single_pop_send_forty() {
    let mut b = at_level(20);
    for r in 9..=12 {
        b.cells[(r, 0)] = Some(PuyoType::Red);
    }
    b.settle();
    assert_eq!(b.tick().sent, 40);
}

#[test]
fn the_match_clock_only_runs_during_play() {
    let mut b = empty_board();
    b.spawn_piece();
    b.tick();
    assert_eq!(b.match_frames, 1);
    b.set_paused(true);
    b.tick();
    assert_eq!(b.match_frames, 1, "the clock ran while paused");
}

#[test]
fn a_higher_starting_level_starts_buried() {
    let rows_of_garbage = |level: u32| {
        let b = Board::new(
            1,
            &Rules {
                starting_level: level,
                ..Rules::default()
            },
        );
        b.cells
            .rows()
            .filter(|row| row.iter().all(|c| *c == Some(PuyoType::Garbage)))
            .count()
    };
    assert_eq!(rows_of_garbage(1), 0);
    assert_eq!(rows_of_garbage(3), 0);
    assert_eq!(rows_of_garbage(4), 1);
    assert_eq!(rows_of_garbage(7), 2);
    assert_eq!(rows_of_garbage(15), 4, "capped");
    let b = Board::new(
        1,
        &Rules {
            starting_level: 4,
            ..Rules::default()
        },
    );
    assert!(
        b.cells
            .rows()
            .last()
            .unwrap()
            .iter()
            .all(|c| *c == Some(PuyoType::Garbage)),
        "not at the bottom"
    );
}

#[test]
fn the_fall_speeds_up_one_frame_every_two_levels() {
    let frames = |level: u32| {
        Board::new(
            1,
            &Rules {
                starting_level: level,
                ..Rules::default()
            },
        )
        .fall_frames_per_cell()
    };
    assert_eq!(frames(1), 16);
    assert_eq!(frames(2), 16);
    assert_eq!(frames(3), 15);
    assert_eq!(frames(17), 8);
    assert_eq!(frames(40), 8, "never faster than Tsu's fastest");
}

/// Turning the satellite down against the floor pushes the pair up. It used
/// to try the sideways kicks first and hop a column over whenever the
/// neighbouring column was free lower down.
#[test]
fn turning_down_against_the_floor_pushes_up_not_sideways() {
    let mut b = empty_board();
    // The pair rests on a puyo in column 2, satellite to its right; column 1
    // is empty all the way down, so a sideways kick would have room.
    let floor = GRID_HEIGHT - 1;
    b.cells[(floor, 2)] = Some(PuyoType::Green);
    b.set_piece(piece((floor - 1) as i32, 2, Rotation::Right));
    b.apply_input(InputKind::RotateCW); // satellite right -> down, into the green puyo
    let p = *b.piece().unwrap();
    assert_eq!((p.row, p.col, p.rotation), ((floor - 2) as i32, 2, Rotation::Down));
}

fn stacked(heights: [usize; GRID_WIDTH]) -> Board {
    let mut board = empty_board();
    for (c, &h) in heights.iter().enumerate() {
        for r in GRID_HEIGHT - h..GRID_HEIGHT {
            board.cells[(r, c)] = Some(PuyoType::Garbage);
        }
    }
    board.spawn_piece();
    board
}

fn held(board: &mut Board, dir: InputKind, target: i32) -> bool {
    let id = board.piece_id;
    for frame in 0..120 {
        let press = frame == 0 || (frame >= 8 && (frame - 8) % 2 == 0);
        board.step(press.then_some(dir), 0);
        match board.piece() {
            Some(p) if board.piece_id == id && p.col == target => return true,
            Some(_) if board.piece_id == id => {}
            _ => return false,
        }
    }
    false
}

#[test]
fn a_held_pair_reaches_the_sides_over_eleven_high_columns() {
    assert!(held(&mut stacked([0, 0, 0, 11, 11, 0]), InputKind::MoveRight, 5));
    assert!(held(&mut stacked([11, 11, 0, 0, 0, 0]), InputKind::MoveLeft, 0));
    assert!(!held(&mut stacked([0, 0, 0, 12, 0, 0]), InputKind::MoveRight, 5));
}

#[test]
fn a_floor_kick_climbs_over_a_full_column() {
    let mut board = stacked([0, 0, 11, 12, 0, 0]);
    for input in [
        InputKind::RotateCCW,
        InputKind::RotateCCW,
        InputKind::RotateCCW,
        InputKind::MoveRight,
    ] {
        board.step([input], 0);
    }
    assert_eq!(board.piece().map(|p| (p.row, p.col)), Some((0, 3)));
}

#[test]
fn nothing_turns_into_the_fourteenth_row() {
    let mut board = stacked([0, 0, 11, 12, 0, 0]);
    for input in [InputKind::RotateCCW, InputKind::RotateCCW] {
        board.step([input], 0);
    }
    assert_eq!(board.piece().map(|p| (p.row, p.rotation)), Some((0, Rotation::Down)));
    board.step([InputKind::RotateCCW], 0);
    board.step([InputKind::RotateCCW], 0);
    let piece = board.piece().expect("still in play");
    assert!(
        piece.positions().iter().all(|&(r, _)| r >= 0),
        "{:?}",
        piece.positions()
    );
}

#[test]
fn danger_ranks_the_tallest_column_first() {
    let mut low = empty_board();
    low.cells.rows_mut().last().unwrap().fill(Some(PuyoType::Red));
    let mut tall = empty_board();
    for r in GRID_HEIGHT - 4..GRID_HEIGHT {
        tall.cells[(r, 0)] = Some(PuyoType::Red);
    }
    assert_eq!(low.danger(), (1, GRID_WIDTH));
    assert_eq!(tall.danger(), (4, 4));
    assert!(tall.danger() > low.danger());
    assert_eq!(empty_board().danger(), (0, 0));
}
