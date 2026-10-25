//! Bots that play on a real server, to see what it holds.
//!
//!     cargo run -p loadbot --release -- --rooms 4 --players 16 --secs 60
//!
//! Each room is made by its first bot, which raises the seat count, waits
//! for the others and starts the game, then restarts it whenever it ends.
//! Every bot sends inputs, pings once a second and counts what it gets.
//! With `--join ID`, the bots join a room someone made instead, and play
//! the games its host starts: a room to watch from a real client.
//!
//! The server allows 16 connections per address; each bot says it comes
//! from an address of its own in `X-Forwarded-For`, which the server only
//! believes from a loopback peer. Against a remote server, reach it through
//! an SSH tunnel to its loopback port, not through Caddy.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use shared::{ClientMessage, InputKind, RoomId, RoomSetting, ServerMessage};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const INPUT_LEAD: u32 = 6;
const STALL: Duration = Duration::from_millis(100);

struct Args {
    url: String,
    rooms: usize,
    players: usize,
    secs: u64,
    ramp: Duration,
    join: Option<RoomId>,
}

impl Args {
    fn parse() -> Self {
        let mut args = Self {
            url: format!("ws://127.0.0.1:{}/ws", shared::config::SERVER_PORT),
            rooms: 1,
            players: 16,
            secs: 60,
            ramp: Duration::from_millis(10),
            join: None,
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let value = it.next().unwrap_or_else(|| usage(&format!("{flag} needs a value")));
            let number = || {
                value
                    .parse::<u64>()
                    .unwrap_or_else(|_| usage(&format!("{flag}: {value}")))
            };
            match flag.as_str() {
                "--url" => args.url.clone_from(&value),
                "--rooms" => args.rooms = number() as usize,
                "--players" => args.players = number() as usize,
                "--secs" => args.secs = number(),
                "--ramp-ms" => args.ramp = Duration::from_millis(number()),
                "--join" => args.join = Some(RoomId::try_from(number()).unwrap_or_else(|_| usage("--join: a room id"))),
                _ => usage(&format!("unknown flag {flag}")),
            }
        }
        if args.players < 2 {
            usage("--players must be at least 2");
        }
        args
    }
}

fn usage(why: &str) -> ! {
    eprintln!(
        "{why}\nusage: loadbot [--url ws://127.0.0.1:8080/ws] [--rooms 1] [--players 16] [--secs 60] [--ramp-ms 10] [--join ROOM_ID]"
    );
    std::process::exit(2);
}

#[derive(Default)]
struct Stats {
    connected: AtomicU64,
    playing: AtomicU64,
    bytes: AtomicU64,
    states: AtomicU64,
    stalls: AtomicU64,
    refused: AtomicU64,
    dropped: AtomicU64,
    rtts: Mutex<Vec<Duration>>,
}

impl Stats {
    fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    fn sub(counter: &AtomicU64) {
        counter.fetch_sub(1, Ordering::Relaxed);
    }

    fn get(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy)]
enum Role {
    Host,
    Guest,
}

struct Bot {
    n: usize,
    role: Role,
    players: usize,
    room: watch::Sender<Option<RoomId>>,
    stats: Arc<Stats>,
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn frame(msg: &ClientMessage) -> Message {
    Message::binary(shared::encode(msg))
}

async fn connect(url: &str, n: usize) -> Result<Ws, String> {
    let mut req = format!("{url}?v={}", shared::PROTOCOL_VERSION)
        .into_client_request()
        .map_err(|e| e.to_string())?;
    let ip = format!("10.{}.{}.{}", (n >> 16) & 255, (n >> 8) & 255, n & 255);
    req.headers_mut().insert(
        "x-forwarded-for",
        HeaderValue::from_str(&ip).map_err(|e| e.to_string())?,
    );
    let (ws, _) = tokio_tungstenite::connect_async(req).await.map_err(|e| e.to_string())?;
    Ok(ws)
}

impl Bot {
    async fn run(self, url: String, until: Instant) -> Result<(), String> {
        let mut ws = connect(&url, self.n).await?;
        Stats::add(&self.stats.connected, 1);
        let mut playing = false;
        let result = self.play(&mut ws, until, &mut playing).await;
        Stats::sub(&self.stats.connected);
        if playing {
            Stats::sub(&self.stats.playing);
        }
        let _ = ws.close(None).await;
        result
    }

    async fn send(ws: &mut Ws, msg: &ClientMessage) -> Result<(), String> {
        ws.send(frame(msg)).await.map_err(|e| e.to_string())
    }

    async fn play(&self, ws: &mut Ws, until: Instant, playing: &mut bool) -> Result<(), String> {
        let hello = ClientMessage::Hello {
            player_id: format!("loadbot-{}", self.n),
            auth_token: None,
            last_disconnect_reason: None,
        };
        Self::send(ws, &hello).await?;
        match self.role {
            Role::Host => Self::send(ws, &ClientMessage::CreateRoom { name: "loadbot".into() }).await?,
            Role::Guest => {
                let mut room = self.room.subscribe();
                let made = room.wait_for(Option::is_some);
                let id = tokio::time::timeout_at(until.into(), made)
                    .await
                    .map_err(|_| "the room was never made".to_owned())?
                    .map_err(|e| e.to_string())?
                    .expect("waited for one");
                Self::send(ws, &ClientMessage::JoinRoom { id }).await?;
            }
        }

        let mut noise = self.n as u64 * 7_919 + 1;
        let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / 60.0));
        let mut ping = tokio::time::interval(Duration::from_secs(1));
        let mut pinged: Option<(u32, Instant)> = None;
        let mut ping_id = 0;
        let mut seq = 0;
        let mut server_tick = None;
        let mut last_state = None::<Instant>;
        let mut raised = false;
        let mut started = false;
        loop {
            if Instant::now() >= until {
                return Ok(());
            }
            tokio::select! {
                msg = ws.next() => {
                    let msg = match msg {
                        Some(Ok(msg)) => msg,
                        Some(Err(e)) => return Err(e.to_string()),
                        None => return Err("closed by the server".into()),
                    };
                    if let Message::Text(text) = &msg {
                        return Err(format!("refused: {text}"));
                    }
                    let data = msg.into_data();
                    Stats::add(&self.stats.bytes, data.len() as u64);
                    match shared::decode::<ServerMessage>(&data) {
                        Some(ServerMessage::Lobby { info }) => {
                            if !info.is_host {
                                continue;
                            }
                            if !raised {
                                raised = true;
                                for _ in 2..self.players {
                                    Self::send(ws, &ClientMessage::SetRoomSetting {
                                            setting: RoomSetting::MaxPlayers,
                                            dir: 1,
                                        }).await?;
                                }
                            }
                            if usize::from(info.settings.max_players) >= self.players {
                                self.room.send_if_modified(|room| room.replace(info.id).is_none());
                            }
                            let full = usize::from(info.players) == self.players && info.players == info.connected;
                            if full && !started && info.countdown.is_none() {
                                started = true;
                                Self::send(ws, &ClientMessage::ToggleCountdown).await?;
                            }
                        }
                        Some(ServerMessage::GameStart { .. }) => {
                            seq = 0;
                            server_tick = None;
                            last_state = None;
                            if !*playing {
                                *playing = true;
                                Stats::add(&self.stats.playing, 1);
                            }
                        }
                        Some(ServerMessage::StateUpdate { tick, .. }) => {
                            Stats::add(&self.stats.states, 1);
                            let now = Instant::now();
                            if last_state.is_some_and(|t| now - t > STALL) {
                                Stats::add(&self.stats.stalls, 1);
                            }
                            last_state = Some(now);
                            server_tick = Some(tick);
                        }
                        Some(ServerMessage::Pong { id }) => {
                            if let Some((sent_id, at)) = pinged.take() {
                                if sent_id == id {
                                    self.stats.rtts.lock().expect("rtts").push(at.elapsed());
                                }
                            }
                        }
                        Some(ServerMessage::JoinFailed { reason }) => {
                            Stats::add(&self.stats.refused, 1);
                            return Err(format!("join refused: {reason}"));
                        }
                        _ => {}
                    }
                }
                _ = tick.tick() => {
                    let Some(at) = server_tick else { continue };
                    let ended = last_state.is_some_and(|t| t.elapsed() > Duration::from_secs(2));
                    if ended {
                        last_state = None;
                        server_tick = None;
                        if std::mem::take(playing) {
                            Stats::sub(&self.stats.playing);
                        }
                        if matches!(self.role, Role::Host) {
                            Self::send(ws, &ClientMessage::RequestRestart).await?;
                        }
                        continue;
                    }
                    if let Some(kind) = input(&mut noise) {
                        seq += 1;
                        Self::send(ws, &ClientMessage::Input { kind, seq, tick: at + INPUT_LEAD }).await?;
                    }
                }
                _ = ping.tick() => {
                    ping_id += 1;
                    pinged = Some((ping_id, Instant::now()));
                    Self::send(ws, &ClientMessage::Ping { id: ping_id }).await?;
                }
            }
        }
    }
}

/// Now and then a move, a turn or a soft drop; never a hard drop, so that
/// boards last.
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

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    sorted
        .get(sorted.len().saturating_sub(1) * p / 100)
        .copied()
        .unwrap_or_default()
}

fn report(stats: &Stats, last: &mut (u64, u64, Instant), bots: usize) {
    let (bytes, states) = (Stats::get(&stats.bytes), Stats::get(&stats.states));
    let secs = last.2.elapsed().as_secs_f64();
    let rate = (bytes - last.0) as f64 / secs;
    let mut rtts = std::mem::take(&mut *stats.rtts.lock().expect("rtts"));
    rtts.sort_unstable();
    println!(
        "{:>4}/{bots} connected  {:>4} playing  in {:>8.1} kB/s ({:>6.1} kB/s/bot)  states {:>6.0}/s  rtt p50 {:>7.1?} p99 {:>7.1?}  stalls {}  refused {}  dropped {}",
        Stats::get(&stats.connected),
        Stats::get(&stats.playing),
        rate / 1000.0,
        rate / 1000.0 / bots.max(1) as f64,
        (states - last.1) as f64 / secs,
        percentile(&rtts, 50),
        percentile(&rtts, 99),
        Stats::get(&stats.stalls),
        Stats::get(&stats.refused),
        Stats::get(&stats.dropped),
    );
    *last = (bytes, states, Instant::now());
}

#[tokio::main]
async fn main() {
    let mut args = Args::parse();
    if args.join.is_some() {
        args.rooms = 1;
    }
    let seats = usize::from(shared::MAX_ROOM_PLAYERS);
    if args.players > seats {
        println!("a room seats {seats} at most: raise MAX_ROOM_PLAYERS, here and on the server, to go further");
        args.players = seats;
    }
    let bots = args.rooms * args.players;
    println!(
        "{} rooms of {} bots on {}, for {} s",
        args.rooms, args.players, args.url, args.secs
    );
    let stats = Arc::new(Stats::default());
    let until = Instant::now() + args.ramp * bots as u32 + Duration::from_secs(args.secs);
    let mut tasks = Vec::with_capacity(bots);
    for room in 0..args.rooms {
        let (id_tx, _) = watch::channel(args.join);
        for seat in 0..args.players {
            let bot = Bot {
                n: room * args.players + seat + 1,
                role: if seat == 0 && args.join.is_none() {
                    Role::Host
                } else {
                    Role::Guest
                },
                players: args.players,
                room: id_tx.clone(),
                stats: Arc::clone(&stats),
            };
            let (url, stats) = (args.url.clone(), Arc::clone(&stats));
            tasks.push(tokio::spawn(async move {
                let n = bot.n;
                if let Err(e) = bot.run(url, until).await {
                    Stats::add(&stats.dropped, 1);
                    eprintln!("bot {n}: {e}");
                }
            }));
            tokio::time::sleep(args.ramp).await;
        }
    }
    let reporter = {
        let stats = Arc::clone(&stats);
        tokio::spawn(async move {
            let mut last = (0, 0, Instant::now());
            let mut every = tokio::time::interval(Duration::from_secs(5));
            every.tick().await;
            loop {
                every.tick().await;
                report(&stats, &mut last, bots);
            }
        })
    };
    for task in tasks {
        let _ = task.await;
    }
    reporter.abort();
    println!("done");
}
