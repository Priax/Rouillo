#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    reason = "test fixtures build small, known values"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;

use super::*;
use crate::config::{GRACE_FRAMES, GRID_HEIGHT, GRID_WIDTH, VISIBLE_ROW_OFFSET};

struct CappedAlloc;

const ALLOC_CAP: usize = 64 << 20;

unsafe impl GlobalAlloc for CappedAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > ALLOC_CAP {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() > ALLOC_CAP {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > ALLOC_CAP {
            return std::ptr::null_mut();
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CappedAlloc = CappedAlloc;

fn empty_board() -> Board {
    Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 1, 5)
}

fn piece(row: i32, col: i32, rotation: usize) -> ActivePuyo {
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
    let mut a = Board::new(GRID_WIDTH, GRID_HEIGHT, 42, 1, 5);
    let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, 42, 1, 5);
    assert_eq!(a.next_types, b.next_types);
    assert_eq!(a.next_next_types, b.next_next_types);
    for _ in 0..50 {
        a.spawn_piece();
        b.spawn_piece();
        let pa = a.active_piece.take().expect("piece a");
        let pb = b.active_piece.take().expect("piece b");
        assert_eq!((pa.axis_type, pa.sat_type), (pb.axis_type, pb.sat_type));
    }
}

#[test]
fn garbage_placement_leaves_the_piece_sequence_alone() {
    // Both players share a seed; only one takes nuisance. The leftover columns are
    // drawn at random, and that draw must not shift the pairs they are dealt.
    let mut a = Board::new(GRID_WIDTH, GRID_HEIGHT, 42, 1, 5);
    let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, 42, 1, 5);
    for i in 0..50 {
        if i % 5 == 0 {
            b.cells.iter_mut().for_each(|row| row.fill(None));
            b.pending_garbage = 3; // not a multiple of 6: the leftover columns are drawn
            b.state = GameState::DroppingGarbage;
            b.drop_garbage();
            b.state = GameState::Playing;
        }
        a.spawn_piece();
        b.spawn_piece();
        let pa = a.active_piece.take().expect("piece a");
        let pb = b.active_piece.take().expect("piece b");
        assert_eq!((pa.axis_type, pa.sat_type), (pb.axis_type, pb.sat_type), "pair {i}");
    }
    assert_ne!(a.rng_position(), b.rng_position(), "setup: the drops drew nothing");
}

#[test]
fn room_settings_survive_any_step_from_the_network() {
    let mut s = RoomSettings::default();
    for dir in [i32::MAX, i32::MIN, 7, -7] {
        s.adjust(0, dir);
        assert!(
            (1..=15).contains(&s.starting_level),
            "level {} after {dir}",
            s.starting_level
        );
        s.adjust(1, dir);
        assert!((4..=5).contains(&s.colors), "colors {} after {dir}", s.colors);
        s.adjust(3, dir);
    }
}

#[test]
fn four_connected_same_color_clears() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    }
    assert_eq!(b.check_matches(), Some(40)); // chain 1, group of 4, no bonus -> 10*4*1
    for r in 9..=12 {
        assert!(b.cells[r][0].is_none());
    }
    assert_eq!(b.chain_count, 1);
}

#[test]
fn three_connected_does_not_clear() {
    let mut b = empty_board();
    for r in 10..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    }
    assert_eq!(b.check_matches(), None);
    for r in 10..=12 {
        assert!(b.cells[r][0].is_some());
    }
}

#[test]
fn group_only_in_hidden_row_does_not_clear() {
    let mut b = empty_board();
    for c in 0..4 {
        b.cells[0][c] = Some(PuyoType::Red);
    }
    assert_eq!(b.check_matches(), None);
}

#[test]
fn adjacent_garbage_is_cleared() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    }
    b.cells[9][1] = Some(PuyoType::Garbage); // touches the group
    b.cells[5][5] = Some(PuyoType::Garbage); // far away
    b.check_matches();
    assert!(b.cells[9][1].is_none(), "adjacent garbage should clear");
    assert_eq!(b.cells[5][5], Some(PuyoType::Garbage), "distant garbage should remain");
}

#[test]
fn garbage_does_not_self_match() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[r][0] = Some(PuyoType::Garbage);
    }
    assert_eq!(b.check_matches(), None);
}

#[test]
fn gravity_drops_floating_puyo_to_floor() {
    let mut b = empty_board();
    b.cells[3][0] = Some(PuyoType::Red);
    assert!(b.apply_board_gravity());
    assert!(b.cells[3][0].is_none());
    assert_eq!(b.cells[GRID_HEIGHT - 1][0], Some(PuyoType::Red));
}

#[test]
fn gravity_noop_when_settled() {
    let mut b = empty_board();
    b.cells[GRID_HEIGHT - 1][0] = Some(PuyoType::Red);
    b.cells[GRID_HEIGHT - 2][0] = Some(PuyoType::Blue);
    assert!(!b.apply_board_gravity());
}

// 70 points of clears == 1 garbage puyo sent; the remainder carries over to
// the next clear. Getting this wrong makes attacks unfair.
#[test]
fn a_pop_converts_score_to_garbage_with_carry() {
    let mut b = empty_board();
    for r in 8..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    } // group of 5 -> score 100
    b.state = GameState::ResolvingMatches;
    assert_eq!(b.after_landing(), 1); // floor(100 / 70)
    assert_eq!(b.nuisance_points, 30); // 100 % 70 carried for the next clear
}

#[test]
fn rotation_in_open_space_just_turns() {
    let mut b = empty_board();
    b.active_piece = Some(piece(6, 2, 0));
    b.rotate_piece(1);
    let p = b.active_piece.unwrap();
    assert_eq!((p.row, p.col, p.rotation), (6, 2, 1));
}

#[test]
fn rotation_against_right_wall_kicks_left() {
    let mut b = empty_board();
    b.active_piece = Some(piece(6, (GRID_WIDTH - 1) as i32, 0));
    b.rotate_piece(1);
    let p = b.active_piece.unwrap();
    assert_eq!((p.col, p.rotation), ((GRID_WIDTH - 2) as i32, 1));
}

#[test]
fn rotation_against_left_wall_kicks_right() {
    let mut b = empty_board();
    b.active_piece = Some(piece(6, 0, 0));
    b.rotate_piece(3);
    let p = b.active_piece.unwrap();
    assert_eq!((p.col, p.rotation), (1, 3));
}

#[test]
fn grounded_rotation_floor_kicks_up() {
    let mut b = empty_board();
    b.active_piece = Some(piece((GRID_HEIGHT - 1) as i32, 2, 3));
    b.rotate_piece(3); // -> rotation 2 (satellite below) into the floor, kicks up
    let p = b.active_piece.unwrap();
    assert_eq!((p.row, p.rotation), ((GRID_HEIGHT - 2) as i32, 2));
}

#[test]
fn piece_locks_after_its_grace_period() {
    let mut b = empty_board();
    b.active_piece = Some(piece((GRID_HEIGHT - 1) as i32, 2, 0));
    for _ in 0..GRACE_FRAMES {
        b.tick();
    }
    assert!(b.active_piece.is_some(), "locked before its grace period ran out");
    b.tick();
    assert!(b.active_piece.is_none());
    assert_eq!(b.cells[GRID_HEIGHT - 1][2], Some(PuyoType::Red)); // axis
    assert_eq!(b.cells[GRID_HEIGHT - 2][2], Some(PuyoType::Blue)); // satellite
    assert_eq!(b.state, GameState::ResolvingMatches);
}

// As in Tsu, the grace period is a total over the pair's life: sliding along
// the floor does not buy more time.
#[test]
fn moving_on_the_ground_does_not_extend_the_grace_period() {
    let mut b = empty_board();
    b.active_piece = Some(piece((GRID_HEIGHT - 1) as i32, 2, 0));
    for _ in 0..GRACE_FRAMES / 2 {
        b.tick();
    }
    b.move_piece(-1);
    for _ in 0..=(GRACE_FRAMES / 2) {
        b.tick();
    }
    assert!(b.active_piece.is_none(), "the move reset the grace period");
}

#[test]
fn soft_dropping_onto_something_locks_at_once() {
    let mut b = empty_board();
    b.active_piece = Some(piece((GRID_HEIGHT - 1) as i32, 2, 0));
    b.apply_input(InputKind::SoftDropPress);
    b.tick();
    assert!(b.active_piece.is_none());
}

#[test]
fn soft_drop_takes_two_frames_per_cell_and_the_natural_fall_sixteen() {
    let rows_after = |frames: u32, soft: bool| {
        let mut b = empty_board();
        b.active_piece = Some(piece(2, 2, 0));
        if soft {
            b.apply_input(InputKind::SoftDropPress);
        }
        for _ in 0..frames {
            b.tick();
        }
        b.active_piece.expect("still falling").row - 2
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

/// A pair locked with its satellite hanging over a hole: the satellite falls
/// with gravity, the chain check waits for it to land and bounce.
#[test]
fn a_split_pair_falls_before_anything_pops() {
    let mut b = empty_board();
    // Satellite to the right of the axis, over an empty column.
    b.cells[GRID_HEIGHT - 1][2] = Some(PuyoType::Green);
    b.active_piece = Some(piece((GRID_HEIGHT - 2) as i32, 2, 1));
    b.hard_drop();
    assert_eq!(b.state, GameState::ResolvingMatches);
    let hanging = b.falls.iter().find(|f| f.col == 3).expect("the satellite falls");
    assert_eq!(hanging.cells_fallen, 1);
    let Settle::Falling { frames, .. } = b.settle else {
        panic!("not falling: {:?}", b.settle);
    };
    assert_eq!(
        frames,
        config::SPLIT_DELAY_SATELLITE as u32 + 10 + config::BOUNCE_FRAMES
    );
    for _ in 0..frames - 1 {
        b.tick();
        assert_eq!(b.state, GameState::ResolvingMatches);
    }
    b.tick();
    assert_eq!(b.state, GameState::Playing, "the next pair never came");
}

#[test]
fn a_group_flashes_before_it_vanishes() {
    let mut b = empty_board();
    for r in 9..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    }
    b.state = GameState::ResolvingMatches;
    b.after_landing();
    assert_eq!(b.popping.len(), 4);
    for _ in 0..config::POP_FRAMES - 1 {
        b.tick();
    }
    assert_eq!(b.cells[12][0], Some(PuyoType::Red), "cleared before the flash ended");
    b.tick();
    assert_eq!(b.cells[12][0], None);
}

#[test]
fn spawning_into_blocked_column_is_game_over() {
    let mut b = empty_board();
    b.cells[VISIBLE_ROW_OFFSET][2] = Some(PuyoType::Red);
    b.spawn_piece();
    assert_eq!(b.state, GameState::GameOver);
}

#[test]
fn state_update_survives_encode_decode() {
    let mut board = empty_board();
    board.spawn_piece();
    board.cells[GRID_HEIGHT - 1][0] = Some(PuyoType::Green);
    board.score = 1234;
    let msg = ServerMessage::StateUpdate {
        p1_board: Box::new(board.clone()),
        p2_board: Box::new(empty_board()),
        p1_rng: None,
        p2_rng: None,
        p1_ack: 7,
        p2_ack: 9,
        tick: 4_242,
        p1_incoming: Vec::new(),
        p2_incoming: Vec::new(),
    };
    let bytes = encode(&msg).expect("encode");
    let back: ServerMessage = decode(&bytes).expect("decode");
    match back {
        ServerMessage::StateUpdate {
            p1_board,
            p1_ack,
            p2_ack,
            tick,
            ..
        } => {
            assert_eq!(p1_ack, 7);
            assert_eq!(p2_ack, 9);
            assert_eq!(tick, 4_242);
            assert_eq!(p1_board.score, 1234);
            assert_eq!(p1_board.cells, board.cells);
            assert!(p1_board.active_piece.is_some());
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn rng_state_survives_encode_decode() {
    // The RNG is no longer part of a Board's wire format; it rides on StateUpdate as an
    // explicit field. This verifies that carrying it that way still round-trips and keeps
    // piece generation deterministic across the wire.
    let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT, 42, 1, 5);
    for _ in 0..10 {
        board.spawn_piece();
        board.active_piece = None;
    }

    let msg = ServerMessage::StateUpdate {
        p1_board: Box::new(board.clone()),
        p2_board: Box::new(empty_board()),
        p1_rng: Some(Box::new(board.rng_state())),
        p2_rng: None,
        p1_ack: 0,
        p2_ack: 0,
        tick: 0,
        p1_incoming: Vec::new(),
        p2_incoming: Vec::new(),
    };
    let bytes = encode(&msg).expect("encode");
    let back: ServerMessage = decode(&bytes).expect("decode");
    let ServerMessage::StateUpdate { p1_board, p1_rng, .. } = back else {
        panic!("wrong variant");
    };
    let mut restored = *p1_board;
    restored.set_rng(*p1_rng.expect("rng present"));

    for _ in 0..20 {
        board.spawn_piece();
        restored.spawn_piece();
        let a = board.active_piece.take().expect("piece a");
        let b = restored.active_piece.take().expect("piece b");
        assert_eq!((a.axis_type, a.sat_type), (b.axis_type, b.sat_type));
    }
}

#[test]
fn state_update_omits_rng_when_none() {
    // A routine (RNG-less) update must round-trip too: the board survives and the RNG
    // field comes back as None so the client knows to keep the RNG it already holds.
    let mut board = empty_board();
    board.spawn_piece();
    let msg = ServerMessage::StateUpdate {
        p1_board: Box::new(board.clone()),
        p2_board: Box::new(empty_board()),
        p1_rng: None,
        p2_rng: None,
        p1_ack: 3,
        p2_ack: 4,
        tick: 0,
        p1_incoming: Vec::new(),
        p2_incoming: Vec::new(),
    };
    let bytes = encode(&msg).expect("encode");
    let back: ServerMessage = decode(&bytes).expect("decode");
    let ServerMessage::StateUpdate { p1_board, p1_rng, .. } = back else {
        panic!("wrong variant");
    };
    assert!(p1_rng.is_none());
    assert_eq!(p1_board.cells, board.cells);
}

#[test]
fn decode_rejects_garbage_bytes() {
    assert!(decode::<ServerMessage>(&[0, 1, 2, 3]).is_none());
}

/// Regression: the size prefix is attacker-controlled and lz4_flex allocates it
/// before reading anything else. A 2 GiB prefix used to make the server request
/// 2 GiB, and a failed allocation aborts the whole process. With `CappedAlloc`
/// above, the old code aborts this test binary; the bound now rejects the
/// message before any allocation.
#[test]
fn decode_rejects_oversized_prefix_without_allocating() {
    for size in [u32::MAX, 0x7fff_ffff, MAX_DECODED_SIZE as u32 + 1] {
        let mut evil = size.to_le_bytes().to_vec();
        evil.push(0);
        assert!(decode::<ClientMessage>(&evil).is_none(), "size {size} accepted");
    }
}

#[test]
fn decode_rejects_input_shorter_than_its_prefix() {
    assert!(decode::<ClientMessage>(&[]).is_none());
    assert!(decode::<ClientMessage>(&[1, 0]).is_none());
}

/// The bound must not reject real traffic: a room list far bigger than anything
/// the server will send still round-trips.
#[test]
fn decode_accepts_large_legitimate_message() {
    let rooms: Vec<RoomInfo> = (0..2_000)
        .map(|id| RoomInfo {
            id,
            name: "x".repeat(24),
            players: 1,
            max: 2,
            in_game: false,
            friends_only: false,
        })
        .collect();
    let bytes = encode(&ServerMessage::RoomList { rooms }).expect("encode");
    match decode::<ServerMessage>(&bytes) {
        Some(ServerMessage::RoomList { rooms }) => assert_eq!(rooms.len(), 2_000),
        _ => panic!("large room list rejected"),
    }
}

#[test]
fn pause_policy_cycles_both_ways() {
    let mut s = RoomSettings::default();
    assert_eq!(s.pause, PausePolicy::Everyone);
    s.adjust(3, 1);
    assert_eq!(s.pause, PausePolicy::HostOnly);
    s.adjust(3, 1);
    assert_eq!(s.pause, PausePolicy::Nobody);
    s.adjust(3, 1);
    assert_eq!(s.pause, PausePolicy::Everyone, "wraps forward");
    s.adjust(3, -1);
    assert_eq!(s.pause, PausePolicy::Nobody, "wraps backward");
}

#[test]
fn pause_policy_permissions() {
    assert!(PausePolicy::Everyone.allows(false));
    assert!(PausePolicy::HostOnly.allows(true));
    assert!(!PausePolicy::HostOnly.allows(false));
    assert!(!PausePolicy::Nobody.allows(true));
}

/// Setting indices come from the client. An out-of-range one used to fall
/// through to the last arm and toggle "friends only".
#[test]
fn unknown_setting_index_changes_nothing() {
    let mut s = RoomSettings::default();
    s.adjust(200, 1);
    assert!(!s.friends_only);
    assert_eq!(s.pause, PausePolicy::Everyone);
}

/// Corrupted but well-framed messages must be rejected or decoded, never allowed
/// to allocate wildly. The size prefix is bounded by `decode` itself; this covers
/// what comes after it: lengths inside the bitcode payload. `CappedAlloc` turns
/// any runaway allocation into an abort. (Measured once over 200k iterations: the
/// largest allocation was 26 KB.)
#[test]
fn decode_survives_corrupted_messages() {
    use rand::SeedableRng;
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(42);
    let samples = [
        bitcode::serialize(&ClientMessage::Hello {
            player_id: "p".repeat(32),
            auth_token: Some("t".repeat(36)),
            username: Some("name".into()),
            last_disconnect_reason: Some("read: boom".into()),
        })
        .unwrap(),
        bitcode::serialize(&ClientMessage::CreateRoom { name: "room".into() }).unwrap(),
        bitcode::serialize(&ServerMessage::RoomList {
            rooms: (0..20)
                .map(|id| RoomInfo {
                    id,
                    name: "r".into(),
                    players: 1,
                    max: 2,
                    in_game: false,
                    friends_only: false,
                })
                .collect(),
        })
        .unwrap(),
    ];
    for i in 0..5_000 {
        let mut raw = samples[i % samples.len()].clone();
        for _ in 0..rng.random_range(1..6) {
            let at = rng.random_range(0..raw.len());
            raw[at] = rng.random();
        }
        if rng.random_bool(0.2) {
            raw.extend((0..rng.random_range(1..16)).map(|_| rng.random::<u8>()));
        }
        let bytes = lz4_flex::compress_prepend_size(&raw);
        let _ = decode::<ClientMessage>(&bytes);
        let _ = decode::<ServerMessage>(&bytes);
    }
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
const SCRIPT_SEED: u64 = 63;

fn run_script(seed: u64, ticks: u64, skip_input_at: Option<u64>) -> Run {
    let fresh = |s: u64| {
        let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, s, 1, 5);
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
        if board.state == GameState::GameOver {
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
const GOLDEN_FINAL_HASH: u64 = 2_759_900_574_527_949_148;

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
    assert_ne!(paused.state_hash(), base.state_hash(), "state");

    // `previous_state` is skipped on the wire but decides what unpausing
    // restores, so two boards differing only there really are different.
    let mut restores_elsewhere = paused.clone();
    restores_elsewhere.previous_state = Some(GameState::DroppingGarbage);
    assert_ne!(restores_elsewhere.state_hash(), paused.state_hash(), "previous_state");

    let mut all_clear = base.clone();
    all_clear.last_was_all_clear = true;
    assert_ne!(all_clear.state_hash(), base.state_hash(), "last_was_all_clear");

    // Room settings reach the simulation through the fall speed and the piece
    // palette, so two boards set up differently must not look alike either.
    let faster = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 4, 5);
    assert_ne!(faster.state_hash(), base.state_hash(), "start_level");
    let four_colours = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 1, 4);
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
        let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, seed + game as u64, 1, 5);
        b.spawn_piece();
        b
    };
    let mut game = 0;
    let mut board = fresh(game);
    let mut states = vec![(game, board.clone())];
    let mut next = 0;
    for t in 1..=ticks {
        if board.state == GameState::GameOver {
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
        states.iter().any(|(_, b)| b.state == GameState::DroppingGarbage),
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
    b.active_piece.as_ref().expect("a piece").row as f32 + b.fall_progress()
}

/// Left alone, a piece must glide: never move up, never jump. Across a
/// natural drop the row goes up by one while the progress falls back to 0,
/// and the two must cancel out.
#[test]
fn a_falling_piece_is_drawn_gliding_down() {
    let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, 3, 1, 5);
    b.spawn_piece();
    let start_row = b.active_piece.as_ref().expect("a piece").row;
    let mut prev = drawn_row(&b);
    let per_tick = 1.0 / config::FALL_FRAMES_PER_CELL as f32;
    while b.active_piece.as_ref().is_some_and(|p| p.row < start_row + 3) {
        b.tick();
        let now = drawn_row(&b);
        assert!(now >= prev - 1e-4, "the piece went up: {prev} -> {now}");
        assert!(now - prev <= 2.0 * per_tick + 1e-4, "the piece jumped: {prev} -> {now}");
        prev = now;
    }
}

#[test]
fn a_resting_piece_is_drawn_on_its_cell() {
    let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, 3, 1, 5);
    b.spawn_piece();
    b.active_piece.as_mut().expect("a piece").row = GRID_HEIGHT as i32 - 1;
    b.fall_offset = config::HALF_CELL_UNITS; // mid-way, had it been able to fall
    assert_eq!(b.fall_progress(), 0.0);
}

/// A group of five reds at the bottom of column 0, and nothing else.
fn board_about_to_pop() -> Board {
    let mut b = empty_board();
    for r in 8..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    } // score 100 -> 1 nuisance
    b.state = GameState::ResolvingMatches;
    b
}

#[test]
fn a_chain_cancels_waiting_nuisance_before_attacking() {
    let mut b = board_about_to_pop();
    b.pending_garbage = 5;
    b.nuisance_points = 69; // 100 + 69 = 169 -> 2 nuisance
    assert_eq!(b.after_landing(), 0, "sent while nuisance was still waiting");
    assert_eq!(b.pending_garbage, 3);
}

#[test]
fn only_the_surplus_of_an_offset_is_sent() {
    let mut b = board_about_to_pop();
    b.pending_garbage = 1;
    b.nuisance_points = 69;
    assert_eq!(b.after_landing(), 1);
    assert_eq!(b.pending_garbage, 0);
}

#[test]
fn an_all_clear_pays_out_with_the_next_chain() {
    let mut b = empty_board();
    b.state = GameState::ResolvingMatches;
    assert_eq!(b.after_landing(), 0, "an all clear was sent on its own");
    assert!(b.last_was_all_clear && b.all_clear_bonus);

    let mut b = Board {
        all_clear_bonus: true,
        ..board_about_to_pop()
    };
    assert_eq!(b.after_landing(), 1 + config::ALL_CLEAR_BONUS);
    assert!(!b.all_clear_bonus, "the bonus was paid twice");
}

/// Nuisance that does not fit is lost; a full column is no reason to lose on
/// the spot, while the nuisance is still falling and nobody has seen it.
#[test]
fn nuisance_into_a_full_column_does_not_end_the_game_mid_drop() {
    let mut b = empty_board();
    for r in 0..GRID_HEIGHT {
        b.cells[r][0] = Some(PuyoType::Red);
    }
    b.pending_garbage = 30;
    b.state = GameState::DroppingGarbage;
    b.drop_garbage();
    assert_ne!(b.state, GameState::GameOver);
    assert!(b.falls.iter().all(|f| f.col != 0), "nuisance landed in a full column");
    assert_eq!(b.falls.len(), 25, "the other five columns take five rows each");
    assert_eq!(b.pending_garbage, 0, "what did not fit was kept for later");
}

/// Buried under nuisance, the board loses only once the drop is over and the
/// next pair has nowhere to appear.
#[test]
fn a_board_buried_by_nuisance_loses_after_the_drop_is_seen() {
    let mut b = empty_board();
    for r in 3..GRID_HEIGHT {
        for c in 0..GRID_WIDTH {
            b.cells[r][c] = Some(PuyoType::Garbage);
        }
    }
    b.pending_garbage = 30;
    b.state = GameState::DroppingGarbage;
    b.drop_garbage();
    let Settle::Falling { frames, .. } = b.settle else {
        panic!("not falling: {:?}", b.settle);
    };
    assert!(frames > 0);
    for _ in 0..frames - 1 {
        b.tick();
        assert_ne!(b.state, GameState::GameOver, "lost before the nuisance had landed");
    }
    b.tick();
    assert_eq!(b.state, GameState::GameOver);
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
    let b = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 8, 5);
    assert_eq!(b.target_points(), 39);
}

/// With the target fully run down, a plain 4-puyo pop (40 points) sends 40
/// nuisance: the figure quoted for Tsu.
#[test]
fn a_fully_run_down_margin_makes_a_single_pop_send_forty() {
    let mut b = at_level(20);
    for r in 9..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    }
    b.state = GameState::ResolvingMatches;
    assert_eq!(b.after_landing(), 40);
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
        let b = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, level, 5);
        b.cells
            .iter()
            .filter(|row| row.iter().all(|c| *c == Some(PuyoType::Garbage)))
            .count()
    };
    assert_eq!(rows_of_garbage(1), 0);
    assert_eq!(rows_of_garbage(3), 0);
    assert_eq!(rows_of_garbage(4), 1);
    assert_eq!(rows_of_garbage(7), 2);
    assert_eq!(rows_of_garbage(15), 4, "capped");
    let b = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 4, 5);
    assert!(
        b.cells[GRID_HEIGHT - 1].iter().all(|c| *c == Some(PuyoType::Garbage)),
        "not at the bottom"
    );
}

#[test]
fn the_fall_speeds_up_one_frame_every_two_levels() {
    let frames = |level: u32| Board::new(GRID_WIDTH, GRID_HEIGHT, 1, level, 5).fall_frames_per_cell();
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
    b.cells[floor][2] = Some(PuyoType::Green);
    b.active_piece = Some(piece((floor - 1) as i32, 2, 1));
    b.rotate_piece(1); // satellite right -> down, into the green puyo
    let p = b.active_piece.unwrap();
    assert_eq!((p.row, p.col, p.rotation), ((floor - 2) as i32, 2, 2));
}

/// One sample of every `ClientMessage` variant. The match in `client_variant`
/// has no wildcard, so a new variant does not compile until it gets an index,
/// and `protocol_samples_cover_every_variant` fails until it gets a sample.
fn client_samples() -> Vec<ClientMessage> {
    vec![
        ClientMessage::Hello {
            player_id: "p".into(),
            auth_token: Some("t".into()),
            username: Some("u".into()),
            last_disconnect_reason: Some("r".into()),
        },
        ClientMessage::Input {
            kind: InputKind::HardDrop,
            seq: 7,
            tick: 9,
        },
        ClientMessage::TogglePause,
        ClientMessage::RequestRestart,
        ClientMessage::RequestRoomList,
        ClientMessage::CreateRoom { name: "n".into() },
        ClientMessage::JoinRoom { id: 3 },
        ClientMessage::LeaveRoom,
        ClientMessage::SetRoomSetting { index: 1, dir: -1 },
        ClientMessage::ToggleCountdown,
        ClientMessage::ReturnToLobby,
        ClientMessage::InviteFriend { user_id: "f".into() },
        ClientMessage::Ping { id: 5 },
    ]
}

const CLIENT_VARIANTS: usize = 13;

fn client_variant(m: &ClientMessage) -> usize {
    match m {
        ClientMessage::Hello { .. } => 0,
        ClientMessage::Input { .. } => 1,
        ClientMessage::TogglePause => 2,
        ClientMessage::RequestRestart => 3,
        ClientMessage::RequestRoomList => 4,
        ClientMessage::CreateRoom { .. } => 5,
        ClientMessage::JoinRoom { .. } => 6,
        ClientMessage::LeaveRoom => 7,
        ClientMessage::SetRoomSetting { .. } => 8,
        ClientMessage::ToggleCountdown => 9,
        ClientMessage::ReturnToLobby => 10,
        ClientMessage::InviteFriend { .. } => 11,
        ClientMessage::Ping { .. } => 12,
    }
}

/// A board with every optional and repeated part filled in: a `None` or an
/// empty `Vec` encodes the same whatever the type inside, so an empty board
/// would let a field added to `ActivePuyo` or `CellFall` go unnoticed.
fn busy_board() -> Board {
    let mut b = empty_board();
    b.spawn_piece();
    b.cells[GRID_HEIGHT - 1][0] = Some(PuyoType::Garbage);
    b.previous_state = Some(GameState::DroppingGarbage);
    b.settle = Settle::Falling { frame: 1, frames: 4 };
    b.falls.push(CellFall {
        row: 2,
        col: 3,
        cells_fallen: 4,
        delay: 1,
        ojama: true,
    });
    b.popping.push((5, 1));
    b
}

fn server_samples() -> Vec<ServerMessage> {
    let board = busy_board();
    let settings = RoomSettings::default();
    vec![
        ServerMessage::GameStart,
        ServerMessage::StateUpdate {
            p1_board: Box::new(board.clone()),
            p2_board: Box::new(board.clone()),
            p1_rng: Some(Box::new(board.rng_state())),
            p2_rng: Some(Box::new(board.rng_state())),
            p1_ack: 1,
            p2_ack: 2,
            tick: 3,
            p1_incoming: vec![IncomingGarbage { at: 4, amount: 5 }],
            p2_incoming: vec![IncomingGarbage { at: 6, amount: 7 }],
        },
        ServerMessage::Restart,
        ServerMessage::OpponentDisconnected,
        ServerMessage::RoomList {
            rooms: vec![RoomInfo {
                id: 1,
                name: "r".into(),
                players: 1,
                max: 2,
                in_game: false,
                friends_only: true,
            }],
        },
        ServerMessage::Lobby {
            info: LobbyInfo {
                id: 1,
                name: "l".into(),
                settings,
                players: 2,
                connected: 1,
                your_slot: 1,
                is_host: true,
                countdown: Some(3),
            },
        },
        ServerMessage::JoinFailed { reason: "x".into() },
        ServerMessage::FriendInvitation {
            from_username: "f".into(),
            room_id: 2,
            room_name: "r".into(),
        },
        ServerMessage::Pong { id: 8 },
        ServerMessage::Maintenance,
    ]
}

const SERVER_VARIANTS: usize = 10;

fn server_variant(m: &ServerMessage) -> usize {
    match m {
        ServerMessage::GameStart => 0,
        ServerMessage::StateUpdate { .. } => 1,
        ServerMessage::Restart => 2,
        ServerMessage::OpponentDisconnected => 3,
        ServerMessage::RoomList { .. } => 4,
        ServerMessage::Lobby { .. } => 5,
        ServerMessage::JoinFailed { .. } => 6,
        ServerMessage::FriendInvitation { .. } => 7,
        ServerMessage::Pong { .. } => 8,
        ServerMessage::Maintenance => 9,
    }
}

#[test]
fn protocol_samples_cover_every_variant() {
    let client: HashSet<usize> = client_samples().iter().map(client_variant).collect();
    let server: HashSet<usize> = server_samples().iter().map(server_variant).collect();
    assert_eq!(
        client,
        (0..CLIENT_VARIANTS).collect(),
        "a ClientMessage variant has no sample"
    );
    assert_eq!(
        server,
        (0..SERVER_VARIANTS).collect(),
        "a ServerMessage variant has no sample"
    );
}

/// Digest of the raw (uncompressed) encoding of every sample: it moves whenever
/// the wire layout does, in a way an old peer would misread.
fn protocol_digest() -> u64 {
    let mut h = Fnv::new();
    for m in client_samples() {
        h.bytes(&bitcode::serialize(&m).expect("encode"));
    }
    for m in server_samples() {
        h.bytes(&bitcode::serialize(&m).expect("encode"));
    }
    h.finish()
}

const PROTOCOL_DIGEST: (u32, u64) = (2, 8_597_404_285_938_695_702);

#[test]
fn protocol_changes_bump_the_version() {
    let digest = protocol_digest();
    assert_eq!(
        (PROTOCOL_VERSION, digest),
        PROTOCOL_DIGEST,
        "the wire format changed: increment PROTOCOL_VERSION in shared/src/lib.rs, \
         then set PROTOCOL_DIGEST to ({}, {digest})",
        PROTOCOL_VERSION + u32::from(PROTOCOL_DIGEST.0 == PROTOCOL_VERSION),
    );
}
