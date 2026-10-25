// Run with:  cargo bench -p shared --features test-support
// HTML report lands in target/criterion/.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use shared::config::{GRID_HEIGHT, GRID_WIDTH};
use shared::{decode, encode, Board, PlayerUpdate, PuyoType, Rules, ServerMessage};

fn empty_board() -> Board {
    let mut b = Board::new(1, &Rules::default());
    b.spawn_piece();
    b
}

fn full_board() -> Board {
    let palette = [
        PuyoType::Red,
        PuyoType::Blue,
        PuyoType::Yellow,
        PuyoType::Green,
        PuyoType::Purple,
    ];
    let mut b = Board::new(1, &Rules::default());
    b.spawn_piece();
    for r in 0..GRID_HEIGHT {
        for c in 0..GRID_WIDTH {
            b.cells_mut()[(r, c)] = Some(palette[(r + c) % palette.len()]);
        }
    }
    b
}

fn state_update(p1: &Board, p2: &Board) -> ServerMessage {
    let player = |slot: u8, board: &Board| PlayerUpdate {
        slot,
        team: slot,
        board: std::sync::Arc::new(board.clone()),
        ack: 0,
        incoming: Vec::new(),
        target: Some(3 - slot),
    };
    ServerMessage::StateUpdate {
        game: 1,
        tick: 0,
        players: vec![player(1, p1), player(2, p2)],
    }
}

fn bench_codec(c: &mut Criterion) {
    let empty = state_update(&empty_board(), &empty_board());
    let full = state_update(&full_board(), &full_board());
    let empty_bytes = encode(&empty);
    let full_bytes = encode(&full);

    c.bench_function("encode_state_update_empty", |b| b.iter(|| encode(black_box(&empty))));
    c.bench_function("encode_state_update_full", |b| b.iter(|| encode(black_box(&full))));

    c.bench_function("decode_state_update_empty", |b| {
        b.iter(|| decode::<ServerMessage>(black_box(&empty_bytes)).unwrap());
    });
    c.bench_function("decode_state_update_full", |b| {
        b.iter(|| decode::<ServerMessage>(black_box(&full_bytes)).unwrap());
    });

    println!(
        "\n[payload sizes] empty StateUpdate = {} bytes, full = {} bytes\n",
        empty_bytes.len(),
        full_bytes.len()
    );
}

fn bench_simulation(c: &mut Criterion) {
    c.bench_function("board_tick", |b| {
        b.iter_batched(
            empty_board,
            |mut board| {
                black_box(board.tick());
                board
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("check_matches_full_board", |b| {
        b.iter_batched(
            || {
                let mut board = Board::new(1, &Rules::default());
                for row in board.cells_mut().rows_mut() {
                    row.fill(Some(PuyoType::Red));
                }
                board
            },
            |mut board| black_box(board.check_matches()),
            BatchSize::SmallInput,
        );
    });
}

fn bench_room_step(c: &mut Criterion) {
    c.bench_function("room_step_tick_and_encode", |b| {
        b.iter_batched(
            || [empty_board(), empty_board()],
            |mut boards| {
                black_box(boards[0].tick());
                black_box(boards[1].tick());
                let msg = state_update(&boards[0], &boards[1]);
                black_box(encode(&msg))
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_codec, bench_simulation, bench_room_step);
criterion_main!(benches);
