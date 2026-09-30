mod auth;
mod db;
mod manager;
mod room;
mod sim;
mod ws;

use shared::config;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{interval, Duration, Instant};
use tracing::{error, info, warn};
use warp::Filter;

use crate::manager::{Command, Manager};
use crate::ws::{bind_listener, ws_route, CMD_CHAN_CAP};

type ConnId = u64;
type Token = String;

const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(600);
const SHUTDOWN_LINGER: Duration = Duration::from_secs(4);

struct TickProfile {
    enabled: bool,
    budget: Duration,
    sum: Duration,
    max: Duration,
    count: u32,
    since_report: Instant,
}

impl TickProfile {
    fn new() -> Self {
        Self {
            enabled: std::env::var("PUYO_PROFILE").is_ok(),
            budget: Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64),
            sum: Duration::ZERO,
            max: Duration::ZERO,
            count: 0,
            since_report: Instant::now(),
        }
    }

    fn record(&mut self, elapsed: Duration, rooms: usize) {
        self.sum += elapsed;
        self.max = self.max.max(elapsed);
        self.count += 1;
        if self.since_report.elapsed() < Duration::from_secs(5) {
            return;
        }
        if self.enabled {
            let avg = self.sum / self.count.max(1);
            let peak_load = self.max.as_secs_f64() / self.budget.as_secs_f64() * 100.0;
            info!(
                "[tick] rooms={rooms} avg={avg:?} max={:?} peak={peak_load:.1}%",
                self.max
            );
        }
        self.sum = Duration::ZERO;
        self.max = Duration::ZERO;
        self.count = 0;
        self.since_report = Instant::now();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Exit {
    Clean,
    TimedOut,
}

struct Winddown {
    deadline: Instant,
    last_game: Option<Instant>,
}

impl Winddown {
    fn new(now: Instant) -> Self {
        Self {
            deadline: now + SHUTDOWN_DEADLINE,
            last_game: None,
        }
    }

    fn check(&mut self, running: usize, now: Instant) -> Option<Exit> {
        if running > 0 {
            self.last_game = Some(now);
            return (now >= self.deadline).then_some(Exit::TimedOut);
        }
        self.last_game
            .is_none_or(|t| now.duration_since(t) >= SHUTDOWN_LINGER)
            .then_some(Exit::Clean)
    }
}

fn run_side_effects(mgr: &mut Manager, pool: &db::DbPool, cmd_tx: &mpsc::Sender<Command>, saves: &mut JoinSet<()>) {
    while saves.try_join_next().is_some() {}
    for rec in mgr.take_unsaved_matches() {
        let pool = pool.clone();
        saves.spawn(async move {
            if let Err(e) = db::record_match_result(&pool, rec).await {
                error!("Match save: {e}");
            }
        });
    }
    for check in mgr.take_friend_checks() {
        let (pool, cmd_tx) = (pool.clone(), cmd_tx.clone());
        tokio::spawn(async move {
            let (a, b) = check.users();
            let friends = db::are_friends(&pool, a, b).await.unwrap_or_else(|e| {
                error!("Friend check: {e}");
                false
            });
            let _ = cmd_tx.send(Command::FriendCheckDone { check, friends }).await;
        });
    }
}

async fn manager_loop(mut cmd_rx: mpsc::Receiver<Command>, cmd_tx: mpsc::Sender<Command>, pool: db::DbPool) {
    let tick_dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let mut ticker = interval(Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64));
    let broadcast_period = Duration::from_secs_f64(1.0 / config::STATE_BROADCAST_HZ as f64);
    let mut mgr = Manager::new();
    let mut last_broadcast = Instant::now();
    let mut profile = TickProfile::new();
    let mut saves = JoinSet::new();
    let mut winddown: Option<Winddown> = None;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let do_broadcast = last_broadcast.elapsed() >= broadcast_period;
                if do_broadcast { last_broadcast = Instant::now(); }
                let t0 = Instant::now();
                mgr.tick(tick_dt, do_broadcast);
                profile.record(t0.elapsed(), mgr.rooms.len());
                mgr.reap_dead();
                run_side_effects(&mut mgr, &pool, &cmd_tx, &mut saves);
            }
            Some(cmd) = cmd_rx.recv() => {
                mgr.handle(cmd);
                mgr.reap_dead();
                run_side_effects(&mut mgr, &pool, &cmd_tx, &mut saves);
            }
        }
        if !mgr.closing() {
            continue;
        }
        let now = Instant::now();
        let running = mgr.games_running();
        match winddown.get_or_insert_with(|| Winddown::new(now)).check(running, now) {
            Some(Exit::Clean) => {
                info!("Plus aucune partie en cours, arrêt");
                break;
            }
            Some(Exit::TimedOut) => {
                warn!(
                    "{running} partie(s) encore en cours après {}s, arrêt quand même",
                    SHUTDOWN_DEADLINE.as_secs()
                );
                break;
            }
            None => {}
        }
    }
    while saves.join_next().await.is_some() {}
}

async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn watch_signals(cmd_tx: mpsc::Sender<Command>) {
    stop_signal().await;
    info!("Arrêt demandé: les parties en cours se terminent, plus rien ne démarre");
    let _ = cmd_tx.send(Command::Shutdown).await;
    stop_signal().await;
    warn!("Second signal, arrêt immédiat");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "console")]
    console_subscriber::init();
    #[cfg(not(feature = "console"))]
    {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .init();
    }

    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = db::init_pool(&database_url)
        .await
        .expect("Failed to connect to database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");
    info!("DB connectée");
    tokio::task::spawn_blocking(db::dummy_hash)
        .await
        .expect("dummy hash init failed");

    let port = config::SERVER_PORT;
    info!("Écoute sur :{port}");

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(CMD_CHAN_CAP);
    let manager = tokio::spawn(manager_loop(cmd_rx, cmd_tx.clone(), pool.clone()));
    tokio::spawn(watch_signals(cmd_tx.clone()));

    let pool_cleanup = pool.clone();
    tokio::spawn(async move {
        let start = tokio::time::Instant::now() + Duration::from_secs(3600);
        let mut ticker = tokio::time::interval_at(start, Duration::from_secs(3600));
        loop {
            ticker.tick().await;
            match db::cleanup_expired_sessions(&pool_cleanup).await {
                Ok(n) if n > 0 => info!("Sessions expirées: {n} supprimées"),
                Err(e) => error!("Session cleanup: {e}"),
                _ => {}
            }
        }
    });

    let routes = ws_route(cmd_tx, pool.clone())
        .or(auth::routes(pool))
        .recover(auth::handle_rejection)
        .with(warp::log::custom(|info| {
            if info.status() == warp::http::StatusCode::SWITCHING_PROTOCOLS {
                return;
            }
            let ms = info.elapsed().as_millis();
            let status = info.status();
            let msg = format!("{} {} {} {}ms", info.method(), info.path(), status.as_u16(), ms);
            if status.is_server_error() {
                error!("{msg}");
            } else if status.is_client_error() {
                warn!("{msg}");
            } else {
                info!("{msg}");
            }
        }));

    let server = warp::serve(routes)
        .incoming(bind_listener((config::SERVER_BIND_ADDRESS, port).into()))
        .run();
    // The manager only returns once a shutdown has let the games finish.
    tokio::select! {
        () = server => {}
        _ = manager => {}
    }
}

#[cfg(test)]
mod tests;
