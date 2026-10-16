use std::time::{Duration, Instant};

use super::*;

const TICKS: u32 = 600;

fn env_list(name: &str, default: &[usize]) -> Vec<usize> {
    std::env::var(name).map_or_else(
        |_| default.to_vec(),
        |v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect(),
    )
}

/// What one connection receives: its reliable messages, and its states
/// board by board when it plays over datagrams.
struct Link {
    conn: ConnId,
    rx: mpsc::Receiver<Vec<u8>>,
    states: Option<mpsc::Receiver<Vec<bytes::Bytes>>>,
}

impl Link {
    fn new(mgr: &mut Manager, conn: ConnId, datagrams: bool) -> Self {
        let (tx, rx) = mpsc::channel(CLIENT_CHAN_CAP);
        let (states_tx, states) = datagrams.then(|| mpsc::channel(CLIENT_CHAN_CAP)).unzip();
        mgr.handle(Command::Register {
            conn,
            sender: tx,
            states: states_tx,
        });
        Self { conn, rx, states }
    }

    fn drain(&mut self) -> usize {
        let mut bytes = 0;
        while let Ok(msg) = self.rx.try_recv() {
            bytes += msg.len();
        }
        if let Some(states) = &mut self.states {
            while let Ok(boards) = states.try_recv() {
                bytes += boards.iter().map(bytes::Bytes::len).sum::<usize>();
            }
        }
        bytes
    }
}

/// `rooms` rooms of `players` each, all started. The seat limit is lifted
/// past `MAX_ROOM_PLAYERS` here only, to see what a bigger room would cost.
fn seat(mgr: &mut Manager, rooms: usize, players: usize, datagrams: bool) -> Vec<Link> {
    let mut links = Vec::with_capacity(rooms * players);
    for _ in 0..rooms {
        let first = links.len() as ConnId + 1;
        let host = Link::new(mgr, first, datagrams);
        hello(mgr, first, &format!("p{first}"));
        mgr.handle(Command::CreateRoom {
            conn: first,
            name: "load".into(),
        });
        let id = mgr.room_of(first).expect("the room was made");
        mgr.rooms.get_mut(&id).expect("room").settings.max_players = u8::try_from(players).expect("at most 255");
        links.push(host);
        let start = links.len() - 1;
        for conn in first + 1..first + players as ConnId {
            links.push(Link::new(mgr, conn, datagrams));
            hello(mgr, conn, &format!("p{conn}"));
            mgr.handle(Command::JoinRoom { conn, id });
            for link in &mut links[start..] {
                link.drain();
            }
        }
        mgr.handle(Command::ToggleCountdown { conn: first });
    }
    mgr.tick(3.5, false);
    let playing = mgr.rooms.values().filter(|r| r.game_running()).count();
    assert_eq!(playing, rooms, "every room should be playing");
    for link in &mut links {
        link.drain();
    }
    links
}

/// A player's inputs: now and then a move, a turn or a soft drop, never a
/// hard drop, so that boards last the whole run.
fn input(noise: &mut u64) -> Option<InputKind> {
    *noise = noise
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    match (*noise >> 33) % 48 {
        0 => Some(InputKind::MoveLeft),
        1 => Some(InputKind::MoveRight),
        2 => Some(InputKind::RotateCW),
        3 => Some(InputKind::RotateCCW),
        4 => Some(InputKind::SoftDropPress),
        5 | 6 => Some(InputKind::SoftDropRelease),
        _ => None,
    }
}

struct Report {
    avg: Duration,
    p99: Duration,
    max: Duration,
    bytes: usize,
    boards_alive: usize,
    boards: usize,
}

fn run(rooms: usize, players: usize, datagrams: bool) -> Report {
    let dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let mut mgr = new_mgr();
    let mut links = seat(&mut mgr, rooms, players, datagrams);
    let mut noise = 42;
    let mut seqs = vec![0u32; links.len()];
    let mut times = Vec::with_capacity(TICKS as usize);
    let mut bytes = 0;
    for _ in 0..TICKS {
        let t0 = Instant::now();
        for (link, seq) in links.iter().zip(&mut seqs) {
            if let Some(kind) = input(&mut noise) {
                let tick = mgr.room_of(link.conn).map_or(0, |id| mgr.rooms[&id].sim.tick) + 1;
                *seq += 1;
                mgr.handle(Command::Input {
                    conn: link.conn,
                    kind,
                    seq: *seq,
                    tick,
                });
            }
        }
        mgr.tick(dt, true);
        times.push(t0.elapsed());
        bytes += links.iter_mut().map(Link::drain).sum::<usize>();
    }
    times.sort_unstable();
    let boards = mgr.rooms.values().flat_map(|r| &r.sim.boards);
    Report {
        avg: times.iter().sum::<Duration>() / TICKS,
        p99: times[times.len() * 99 / 100],
        max: times[times.len() - 1],
        bytes,
        boards_alive: boards.clone().filter(|b| b.state != GameState::GameOver).count(),
        boards: boards.count(),
    }
}

/// What the game loop costs, and what it sends, for rooms of each size.
///
///     cargo test -p server --release -- --ignored --nocapture load_rooms
///
/// `PUYO_LOAD_PLAYERS=2,16,99` sets the room sizes, `PUYO_LOAD_TOTAL=1000`
/// the players in all (rooms = total / size, at least one), and
/// `PUYO_LOAD_ROOMS=500` a fixed number of rooms instead. Single thread,
/// no sockets: the manager's share of the work, as on the server.
#[test]
#[ignore = "load test: run with --release -- --ignored --nocapture"]
fn load_rooms() {
    let sizes = env_list("PUYO_LOAD_PLAYERS", &[2, 16, 99]);
    let total = env_list("PUYO_LOAD_TOTAL", &[1000])[0];
    let fixed = env_list("PUYO_LOAD_ROOMS", &[]).first().copied();
    let budget = Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64);
    let secs = f64::from(TICKS) / config::SERVER_TICK_HZ as f64;
    println!("\n{TICKS} ticks ({secs:.0} s of play), budget {budget:?} per tick, one thread");
    println!(
        "{:>6} {:>7} {:>8} {:>10} {:>10} {:>10} {:>7} {:>11} {:>10} {:>8}",
        "rooms", "players", "link", "tick avg", "tick p99", "tick max", "p99 %", "kB/s/player", "Mbit/s", "alive"
    );
    for players in sizes {
        let rooms = fixed.unwrap_or((total / players).max(1));
        for datagrams in [false, true] {
            let r = run(rooms, players, datagrams);
            let per_player = r.bytes as f64 / secs / (rooms * players) as f64 / 1000.0;
            let mbits = r.bytes as f64 * 8.0 / secs / 1e6;
            println!(
                "{rooms:>6} {players:>7} {:>8} {:>10.2?} {:>10.2?} {:>10.2?} {:>6.1}% {per_player:>11.1} {mbits:>10.1} {:>4}/{:<4}",
                if datagrams { "datagram" } else { "ws" },
                r.avg,
                r.p99,
                r.max,
                r.p99.as_secs_f64() / budget.as_secs_f64() * 100.0,
                r.boards_alive,
                r.boards,
            );
        }
    }
    println!();
}
