// Run with:  cargo bench -p shared
// HTML report lands in target/criterion/.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use shared::config::{GRID_HEIGHT, GRID_WIDTH};
use shared::{decode, encode, Board, PuyoType, ServerMessage};

fn empty_board() -> Board {
    let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 1, 5);
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
    let mut b = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 1, 5);
    b.spawn_piece();
    for r in 0..GRID_HEIGHT {
        for c in 0..GRID_WIDTH {
            b.cells[r][c] = Some(palette[(r + c) % palette.len()]);
        }
    }
    b
}

fn state_update(p1: &Board, p2: &Board) -> ServerMessage {
    ServerMessage::StateUpdate {
        p1_board: Box::new(p1.clone()),
        p2_board: Box::new(p2.clone()),
        // Snapshot-style update: RNG present (worst-case payload), matching the
        // server's full-send path. Routine updates send `None` here.
        p1_rng: Some(Box::new(p1.rng_state())),
        p2_rng: Some(Box::new(p2.rng_state())),
        p1_ack: 0,
        p2_ack: 0,
    }
}

fn bench_codec(c: &mut Criterion) {
    let empty = state_update(&empty_board(), &empty_board());
    let full = state_update(&full_board(), &full_board());
    let empty_bytes = encode(&empty).expect("encode empty StateUpdate");
    let full_bytes = encode(&full).expect("encode full StateUpdate");

    c.bench_function("encode_state_update_empty", |b| b.iter(|| encode(black_box(&empty))));
    c.bench_function("encode_state_update_full", |b| b.iter(|| encode(black_box(&full))));

    c.bench_function("decode_state_update_empty", |b| {
        b.iter(|| decode::<ServerMessage>(black_box(&empty_bytes)).unwrap())
    });
    c.bench_function("decode_state_update_full", |b| {
        b.iter(|| decode::<ServerMessage>(black_box(&full_bytes)).unwrap())
    });

    println!(
        "\n[payload sizes] empty StateUpdate = {} bytes, full = {} bytes\n",
        empty_bytes.len(),
        full_bytes.len()
    );
}

fn bench_simulation(c: &mut Criterion) {
    let dt = 1.0 / 60.0;

    c.bench_function("board_tick", |b| {
        b.iter_batched(
            empty_board,
            |mut board| {
                board.tick(black_box(dt));
                board
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("check_matches_full_board", |b| {
        b.iter_batched(
            || {
                let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT, 1, 1, 5);
                for row in board.cells.iter_mut() {
                    for cell in row.iter_mut() {
                        *cell = Some(PuyoType::Red);
                    }
                }
                board
            },
            |mut board| black_box(board.check_matches()),
            BatchSize::SmallInput,
        )
    });
}

fn bench_room_step(c: &mut Criterion) {
    let dt = 1.0 / 60.0;
    c.bench_function("room_step_tick_and_encode", |b| {
        b.iter_batched(
            || [empty_board(), empty_board()],
            |mut boards| {
                boards[0].tick(black_box(dt));
                boards[1].tick(black_box(dt));
                let msg = state_update(&boards[0], &boards[1]);
                black_box(encode(&msg))
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, bench_codec, bench_simulation, bench_room_step);
criterion_main!(benches);
