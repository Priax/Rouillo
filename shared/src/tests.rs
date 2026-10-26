#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    reason = "test fixtures build small, known values"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;
use std::hash::Hasher;

use super::*;
use crate::config::GRID_HEIGHT;
use crate::hash::Fnv;

fn empty_board() -> Board {
    Board::new(1, &Rules::default())
}

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

#[test]
fn room_settings_survive_any_step_from_the_network() {
    let mut s = RoomSettings::default();
    for dir in [i32::MAX, i32::MIN, 7, -7] {
        s.adjust(RoomSetting::StartingLevel, dir);
        assert!(
            (1..=15).contains(&s.rules.starting_level),
            "level {} after {dir}",
            s.rules.starting_level
        );
        s.adjust(RoomSetting::Colors, dir);
        assert!(
            (4..=5).contains(&s.rules.colors),
            "colors {} after {dir}",
            s.rules.colors
        );
        s.adjust(RoomSetting::Columns, dir);
        assert!(
            (4..=10).contains(&s.rules.columns),
            "columns {} after {dir}",
            s.rules.columns
        );
        s.adjust(RoomSetting::Rows, dir);
        assert!((8..=16).contains(&s.rules.rows), "rows {} after {dir}", s.rules.rows);
        s.adjust(RoomSetting::Pause, dir);
        s.adjust(RoomSetting::MaxPlayers, dir);
        assert!(
            (2..=MAX_ROOM_PLAYERS).contains(&s.max_players),
            "players {} after {dir}",
            s.max_players
        );
    }
}

fn player(slot: u8, board: &Board, ack: u32) -> PlayerUpdate {
    PlayerUpdate {
        slot,
        team: slot,
        board: std::sync::Arc::new(board.clone()),
        ack,
        incoming: Vec::new(),
        target: None,
    }
}

fn round_trip(players: Vec<PlayerUpdate>, tick: u32) -> (u32, Vec<PlayerUpdate>) {
    let bytes = encode(&ServerMessage::StateUpdate { game: 1, tick, players });
    match decode(&bytes).expect("decode") {
        ServerMessage::StateUpdate { tick, players, .. } => (tick, players),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn state_update_survives_encode_decode() {
    let mut board = empty_board();
    board.spawn_piece();
    board.cells_mut()[(GRID_HEIGHT - 1, 0)] = Some(PuyoType::Green);
    board.score = 1234;
    let (tick, players) = round_trip(vec![player(1, &board, 7), player(2, &empty_board(), 9)], 4_242);
    assert_eq!(tick, 4_242);
    assert_eq!(
        players.iter().map(|p| (p.slot, p.ack)).collect::<Vec<_>>(),
        [(1, 7), (2, 9)]
    );
    let first = &players[0].board;
    assert_eq!(first.score, 1234);
    assert_eq!(first.cells(), board.cells());
    assert!(first.piece().is_some());
}

#[test]
fn a_sent_board_draws_what_the_original_draws() {
    let mut board = Board::new(42, &Rules::default());
    for _ in 0..10 {
        board.spawn_piece();
        board.take_piece();
    }
    board.pending_garbage = 3;
    board.drop_pending_garbage();
    let (_, mut players) = round_trip(vec![player(1, &board, 0)], 0);
    let mut restored = std::sync::Arc::unwrap_or_clone(players.remove(0).board);
    assert_eq!(restored.state_hash(), board.state_hash());

    for _ in 0..20 {
        board.spawn_piece();
        restored.spawn_piece();
        let a = board.take_piece().expect("piece a");
        let b = restored.take_piece().expect("piece b");
        assert_eq!((a.axis_type, a.sat_type), (b.axis_type, b.sat_type));
    }
    for b in [&mut board, &mut restored] {
        b.cells_mut().rows_mut().for_each(|row| row.fill(None));
        b.pending_garbage = 3;
        b.drop_pending_garbage();
    }
    assert_eq!(restored.cells(), board.cells(), "the garbage stream drifted");
}

/// A thumb that does not hold together is turned down and leaves the board
/// as it was, rather than panicking later when the board is drawn.
#[test]
fn a_malformed_thumb_is_not_applied() {
    let board = busy_boards()[0].clone();
    let mut shown = empty_board();
    assert!(Thumb::of(&board, true).apply_to(&mut shown));
    let before = shown.state_hash();
    let short = Thumb {
        cells: Some(vec![None; 5]),
        ..Thumb::of(&board, true)
    };
    assert!(!short.apply_to(&mut shown), "too few cells for the size");
    let off_grid = Thumb {
        phase: Phase::Popping {
            frame: 0,
            cells: vec![(GRID_HEIGHT as u8, 0)],
        },
        ..Thumb::of(&board, false)
    };
    assert!(!off_grid.apply_to(&mut shown), "a popping cell below the floor");
    let no_rows = Thumb {
        height: 0,
        cells: Some(Vec::new()),
        ..Thumb::of(&board, false)
    };
    assert!(!no_rows.apply_to(&mut shown), "no room to spawn");
    assert_eq!(shown.state_hash(), before, "a rejected thumb changed the board");
    assert!(Grid::from_cells(6, 13, vec![None; 77]).is_none());
    assert!(Grid::from_cells(0, 0, Vec::new()).is_none());
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
            spectators: 0,
            ranked: false,
        })
        .collect();
    let bytes = encode(&ServerMessage::RoomList { rooms });
    match decode::<ServerMessage>(&bytes) {
        Some(ServerMessage::RoomList { rooms }) => assert_eq!(rooms.len(), 2_000),
        _ => panic!("large room list rejected"),
    }
}

#[test]
fn pause_policy_cycles_both_ways() {
    let mut s = RoomSettings::default();
    assert_eq!(s.pause, PausePolicy::Everyone);
    s.adjust(RoomSetting::Pause, 1);
    assert_eq!(s.pause, PausePolicy::HostOnly);
    s.adjust(RoomSetting::Pause, 1);
    assert_eq!(s.pause, PausePolicy::Nobody);
    s.adjust(RoomSetting::Pause, 1);
    assert_eq!(s.pause, PausePolicy::Everyone, "wraps forward");
    s.adjust(RoomSetting::Pause, -1);
    assert_eq!(s.pause, PausePolicy::Nobody, "wraps backward");
}

#[test]
fn pause_policy_permissions() {
    assert!(PausePolicy::Everyone.allows(false));
    assert!(PausePolicy::HostOnly.allows(true));
    assert!(!PausePolicy::HostOnly.allows(false));
    assert!(!PausePolicy::Nobody.allows(true));
}

/// Corrupted but well-framed messages must be rejected or decoded, never allowed
/// to allocate wildly. The size prefix is bounded by `decode` itself; this covers
/// what comes after it: lengths inside the bitcode payload. `CappedAlloc` turns
/// any runaway allocation into an abort. (Measured once over 200k iterations: the
/// largest allocation was 26 KB.)
#[test]
fn decode_survives_corrupted_messages() {
    use rand::{RngExt, SeedableRng};
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(42);
    let samples = [
        bitcode::encode(&ClientMessage::Hello {
            player_id: "p".repeat(32),
            auth_token: Some("t".repeat(36)),
            last_disconnect_reason: Some("read: boom".into()),
        }),
        bitcode::encode(&ClientMessage::CreateRoom { name: "room".into() }),
        bitcode::encode(&ServerMessage::RoomList {
            rooms: (0..20)
                .map(|id| RoomInfo {
                    id,
                    name: "r".into(),
                    players: 1,
                    max: 2,
                    in_game: false,
                    friends_only: false,
                    spectators: 0,
                    ranked: false,
                })
                .collect(),
        }),
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

/// One sample of every `ClientMessage` variant. The match in `client_variant`
/// has no wildcard, so a new variant does not compile until it gets an index,
/// and `protocol_samples_cover_every_variant` fails until it gets a sample.
fn client_samples() -> Vec<ClientMessage> {
    vec![
        ClientMessage::Hello {
            player_id: "p".into(),
            auth_token: Some("t".into()),
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
        ClientMessage::SetRoomSetting {
            setting: RoomSetting::Colors,
            dir: -1,
        },
        ClientMessage::ToggleCountdown,
        ClientMessage::ReturnToLobby,
        ClientMessage::InviteFriend { user_id: "f".into() },
        ClientMessage::Ping { id: 5 },
        ClientMessage::JoinQueue,
        ClientMessage::LeaveQueue,
        ClientMessage::AcceptMatch,
        ClientMessage::Chat { text: "c".into() },
        ClientMessage::Spectate { id: 9 },
        ClientMessage::WatchFriend { user_id: "u".into() },
        ClientMessage::SetTargeting {
            targeting: Targeting::Manual(2),
        },
        ClientMessage::SetTeam { team: Some(2) },
        ClientMessage::Inputs {
            inputs: vec![SentInput {
                seq: 4,
                tick: 90,
                kind: InputKind::HardDrop,
            }],
        },
        ClientMessage::Follow { slot: 3 },
        ClientMessage::AddCpu,
        ClientMessage::SetCpu {
            slot: 2,
            difficulty: crate::cpu::Difficulty::Hard,
            style: crate::cpu::Style::Architect,
            team: Some(1),
        },
        ClientMessage::RemoveCpu { slot: 2 },
    ]
}

const CLIENT_VARIANTS: usize = 26;

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
        ClientMessage::JoinQueue => 13,
        ClientMessage::LeaveQueue => 14,
        ClientMessage::AcceptMatch => 15,
        ClientMessage::Chat { .. } => 16,
        ClientMessage::Spectate { .. } => 17,
        ClientMessage::WatchFriend { .. } => 18,
        ClientMessage::SetTargeting { .. } => 19,
        ClientMessage::SetTeam { .. } => 20,
        ClientMessage::Inputs { .. } => 21,
        ClientMessage::Follow { .. } => 22,
        ClientMessage::AddCpu => 23,
        ClientMessage::SetCpu { .. } => 24,
        ClientMessage::RemoveCpu { .. } => 25,
    }
}

/// Boards with every phase that carries data, its parts filled in: a `None`
/// or an empty `Vec` encodes the same whatever the type inside, so an empty
/// board would let a field added to `ActivePuyo` or `CellFall` go unnoticed.
fn busy_boards() -> [Board; 3] {
    let mut steering = empty_board();
    steering.spawn_piece();
    steering.cells_mut()[(GRID_HEIGHT - 1, 0)] = Some(PuyoType::Garbage);
    let mut falling = steering.clone();
    falling.apply_input(InputKind::HardDrop);
    falling.tick();
    let mut popping = empty_board();
    for r in GRID_HEIGHT - 4..GRID_HEIGHT {
        popping.cells_mut()[(r, 5)] = Some(PuyoType::Red);
    }
    popping.settle();
    popping.tick();
    popping.tick();
    assert!(matches!(steering.phase(), Phase::Pair(_)));
    assert!(matches!(falling.phase(), Phase::Falling(_)));
    assert!(matches!(popping.phase(), Phase::Popping { .. }));
    let mut boards = [steering, falling, popping];
    for board in &mut boards {
        board.set_paused(true);
    }
    boards
}

fn server_samples() -> Vec<ServerMessage> {
    let boards = busy_boards();
    let settings = RoomSettings::default();
    vec![
        ServerMessage::GameStart {
            game: 9,
            your_slot: 2,
            names: vec!["a".into(), "b".into(), "c".into()],
        },
        ServerMessage::StateUpdate {
            game: 9,
            tick: 3,
            players: boards
                .iter()
                .map(|board| PlayerUpdate {
                    slot: 1,
                    team: 2,
                    board: std::sync::Arc::new(board.clone()),
                    ack: 1,
                    incoming: vec![IncomingGarbage { at: 4, amount: 5 }],
                    target: Some(3),
                })
                .collect(),
        },
        ServerMessage::OpponentDisconnected,
        ServerMessage::RoomList {
            rooms: vec![RoomInfo {
                id: 1,
                name: "r".into(),
                players: 1,
                max: 2,
                in_game: false,
                friends_only: true,
                spectators: 3,
                ranked: true,
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
                ranked: Some(RankedInfo {
                    opponent: "o".into(),
                    opponent_elo: 1010,
                    opponent_avatar: Some("/a".into()),
                    wins: [2, 1],
                }),
                names: vec!["a".into(), "b".into()],
                teams: vec![Some(1), None],
                cpu_seats: true,
                cpus: vec![None, Some((crate::cpu::Difficulty::Pro, crate::cpu::Style::Counter))],
                spectators: 4,
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
        ServerMessage::QueueRefused { reason: "q".into() },
        ServerMessage::MatchFound {
            opponent: "o".into(),
            opponent_elo: 990,
            opponent_avatar: None,
            secs: 10,
        },
        ServerMessage::MatchCancelled { requeued: true },
        ServerMessage::QueueCooldown { secs: 60 },
        ServerMessage::SeriesScore { wins: [1, 2] },
        ServerMessage::SeriesOver {
            winner_slot: Some(2),
            elo_change: -16,
        },
        ServerMessage::SessionRevoked,
        ServerMessage::Chat {
            from: "f".into(),
            text: "t".into(),
            spectator: true,
        },
        ServerMessage::Spectators { count: 3 },
        ServerMessage::OpenedElsewhere,
        ServerMessage::Thumbs {
            game: 9,
            tick: 3,
            thumbs: boards
                .iter()
                .map(|board| Thumb {
                    slot: 2,
                    team: 1,
                    target: Some(1),
                    incoming: vec![IncomingGarbage { at: 4, amount: 5 }],
                    ..Thumb::of(board, true)
                })
                .chain([Thumb::of(&boards[0], false)])
                .collect(),
        },
    ]
}

const SERVER_VARIANTS: usize = 20;

fn server_variant(m: &ServerMessage) -> usize {
    match m {
        ServerMessage::GameStart { .. } => 0,
        ServerMessage::StateUpdate { .. } => 1,
        ServerMessage::OpponentDisconnected => 2,
        ServerMessage::RoomList { .. } => 3,
        ServerMessage::Lobby { .. } => 4,
        ServerMessage::JoinFailed { .. } => 5,
        ServerMessage::FriendInvitation { .. } => 6,
        ServerMessage::Pong { .. } => 7,
        ServerMessage::Maintenance => 8,
        ServerMessage::QueueRefused { .. } => 9,
        ServerMessage::SeriesScore { .. } => 10,
        ServerMessage::SeriesOver { .. } => 11,
        ServerMessage::SessionRevoked => 12,
        ServerMessage::MatchFound { .. } => 13,
        ServerMessage::MatchCancelled { .. } => 14,
        ServerMessage::QueueCooldown { .. } => 15,
        ServerMessage::Chat { .. } => 16,
        ServerMessage::Spectators { .. } => 17,
        ServerMessage::OpenedElsewhere => 18,
        ServerMessage::Thumbs { .. } => 19,
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
        h.write(&bitcode::encode(&m));
    }
    for m in server_samples() {
        h.write(&bitcode::encode(&m));
    }
    h.finish()
}

const PROTOCOL_DIGEST: (u32, u64) = (13, 8_009_367_441_582_123_114);

#[test]
fn protocol_changes_bump_the_version() {
    let digest = protocol_digest();
    assert_eq!(
        (PROTOCOL_VERSION, digest),
        PROTOCOL_DIGEST,
        "the wire format changed: increment PROTOCOL_VERSION in shared/src/protocol.rs, \
         then set PROTOCOL_DIGEST to ({}, {digest})",
        PROTOCOL_VERSION + u32::from(PROTOCOL_DIGEST.0 == PROTOCOL_VERSION),
    );
}
