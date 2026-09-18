use std::alloc::{GlobalAlloc, Layout, System};

use super::*;
use crate::config::{GRID_HEIGHT, GRID_WIDTH, MAX_LOCK_TIME, VISIBLE_ROW_OFFSET};

/// Refuses any single allocation above 64 MiB for this whole test binary.
///
/// Linux overcommits lazily, so an oversized allocation normally "succeeds" and
/// a test cannot see it happen. Capping it turns a regression in `decode`'s size
/// bound into a hard abort of the test run instead of a silent pass. Nothing
/// legitimate in these tests comes close to the cap.
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

// A 4-group living entirely in the hidden buffer row must NOT pop: only
// groups with at least one cell in the visible field count.
#[test]
fn group_only_in_hidden_row_does_not_clear() {
    let mut b = empty_board();
    for c in 0..4 {
        b.cells[0][c] = Some(PuyoType::Red);
    }
    assert_eq!(b.check_matches(), None);
}

// Garbage touching a popping group is cleared with it; distant garbage stays.
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

// Garbage never forms a match on its own.
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
fn resolve_step_converts_score_to_garbage_with_carry() {
    let mut b = empty_board();
    for r in 8..=12 {
        b.cells[r][0] = Some(PuyoType::Red);
    } // group of 5 -> score 100
    assert_eq!(b.resolve_step(), 1); // floor(100 / 70)
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
    b.rotate_piece(1); // satellite would land out of bounds on the right
    let p = b.active_piece.unwrap();
    assert_eq!((p.col, p.rotation), ((GRID_WIDTH - 2) as i32, 1));
}

#[test]
fn rotation_against_left_wall_kicks_right() {
    let mut b = empty_board();
    b.active_piece = Some(piece(6, 0, 0));
    b.rotate_piece(3); // satellite would land out of bounds on the left
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
fn piece_locks_after_lock_delay() {
    let mut b = empty_board();
    b.active_piece = Some(piece((GRID_HEIGHT - 1) as i32, 2, 0));
    let locked = b.update_logic(MAX_LOCK_TIME + 0.1);
    assert!(locked);
    assert!(b.active_piece.is_none());
    assert_eq!(b.cells[GRID_HEIGHT - 1][2], Some(PuyoType::Red)); // axis
    assert_eq!(b.cells[GRID_HEIGHT - 2][2], Some(PuyoType::Blue)); // satellite
    assert_eq!(b.state, GameState::ResolvingMatches);
}

// Moving a grounded piece resets the lock timer (lets you slide before it
// locks). Two sub-threshold ticks must not lock if a move resets between them.
#[test]
fn moving_grounded_piece_resets_lock_timer() {
    let mut b = empty_board();
    b.active_piece = Some(piece((GRID_HEIGHT - 1) as i32, 2, 0));
    assert!(!b.update_logic(0.4)); // touching ground, timer at 0.4
    b.move_piece(-1); // resets timer to 0
    assert!(!b.update_logic(0.4)); // 0.4 again < 0.5 -> still not locked
    assert!(b.active_piece.is_some());
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
    };
    let bytes = encode(&msg).expect("encode");
    let back: ServerMessage = decode(&bytes).expect("decode");
    match back {
        ServerMessage::StateUpdate {
            p1_board,
            p1_ack,
            p2_ack,
            ..
        } => {
            assert_eq!(p1_ack, 7);
            assert_eq!(p2_ack, 9);
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
/// what comes after it — lengths inside the bitcode payload. `CappedAlloc` turns
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
