use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, Instant};
use warp::Filter;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use rand::RngExt; // rand 0.10 moved `random()`/`random_range()` to this trait
use shared::{Board, ClientMessage, ServerMessage, InputKind, GameState,
             RoomId, RoomSettings, RoomInfo, LobbyInfo, config};

type ConnId = u64;
type Token = String;

const GRACE: Duration = Duration::from_secs(120);

const CLIENT_CHAN_CAP: usize = 128;

enum Phase {
    Lobby,
    CountingDown(f32),
    Playing,
}

struct Sim {
    boards: [Board; 2],
    paused: bool,
    finished: bool,
    last_seq: [u32; 2],
    last_restart: Option<Instant>,
}

impl Sim {
    fn new(settings: &RoomSettings) -> Sim {
        Sim {
            boards: Self::fresh_boards(settings),
            paused: false,
            finished: false,
            last_seq: [0; 2],
            last_restart: None,
        }
    }

    fn fresh_boards(s: &RoomSettings) -> [Board; 2] {
        let seed: u64 = rand::rng().random();
        [
            Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, seed, s.starting_level, s.colors),
            Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, seed, s.starting_level, s.colors),
        ]
    }

    fn reset_boards(&mut self, s: &RoomSettings) {
        self.boards = Self::fresh_boards(s);
        self.boards[0].spawn_piece();
        self.boards[1].spawn_piece();
        self.paused = false;
        self.finished = false;
        self.last_seq = [0; 2];
    }
}

struct Member {
    token: Token,
    conn: Option<ConnId>,
    disconnect_at: Option<Instant>,
}

struct Room {
    id: RoomId,
    name: String,
    host: Token,
    members: Vec<Member>,
    settings: RoomSettings,
    phase: Phase,
    sim: Sim,
}

impl Room {
    fn slot_of_conn(&self, conn: ConnId) -> Option<usize> {
        self.members.iter().position(|m| m.conn == Some(conn))
    }

    fn is_host_conn(&self, conn: ConnId) -> bool {
        self.slot_of_conn(conn).is_some_and(|s| self.members[s].token == self.host)
    }

    fn all_connected(&self) -> bool {
        self.members.iter().all(|m| m.conn.is_some())
    }

    fn connected_conns(&self) -> Vec<ConnId> {
        self.members.iter().filter_map(|m| m.conn).collect()
    }

    fn lobby_info_for(&self, idx: usize) -> LobbyInfo {
        LobbyInfo {
            id: self.id,
            name: self.name.clone(),
            settings: self.settings,
            players: self.members.len() as u8,
            your_slot: (idx + 1) as u8,
            is_host: self.members[idx].token == self.host,
            countdown: match self.phase {
                Phase::CountingDown(t) => Some(t.ceil() as u8),
                _ => None,
            },
        }
    }

    fn info(&self) -> RoomInfo {
        RoomInfo {
            id: self.id,
            name: self.name.clone(),
            players: self.members.len() as u8,
            max: 2,
            in_game: !matches!(self.phase, Phase::Lobby),
        }
    }
}

struct Manager {
    rooms: HashMap<RoomId, Room>,
    clients: HashMap<ConnId, Option<RoomId>>,
    conn_token: HashMap<ConnId, Token>,
    senders: HashMap<ConnId, mpsc::Sender<Vec<u8>>>,
    dead: Vec<ConnId>,
    next_id: RoomId,
    room_list_dirty: bool,
    last_room_list: Instant,
}

enum Command {
    Register { conn: ConnId, sender: mpsc::Sender<Vec<u8>> },
    Unregister { conn: ConnId },
    Hello { conn: ConnId, token: Token },
    RequestRoomList { conn: ConnId },
    CreateRoom { conn: ConnId, name: String },
    JoinRoom { conn: ConnId, id: RoomId },
    LeaveRoom { conn: ConnId },
    SetSetting { conn: ConnId, index: u8, dir: i32 },
    ToggleCountdown { conn: ConnId },
    ReturnToLobby { conn: ConnId },
    Input { conn: ConnId, kind: InputKind, seq: u32 },
    TogglePause { conn: ConnId },
    Restart { conn: ConnId },
}

fn clean_name(name: String) -> String {
    let n = name.trim();
    if n.is_empty() { "Room".to_string() } else { n.chars().take(24).collect() }
}

impl Manager {
    fn new() -> Manager {
        Manager {
            rooms: HashMap::new(),
            clients: HashMap::new(),
            conn_token: HashMap::new(),
            senders: HashMap::new(),
            dead: Vec::new(),
            next_id: 1,
            room_list_dirty: false,
            last_room_list: Instant::now(),
        }
    }

    fn room_of(&self, conn: ConnId) -> Option<RoomId> {
        self.clients.get(&conn).copied().flatten()
    }

    /// Run `f` on the room with the given id, returning its result, or `None` if
    /// the room no longer exists.
    fn with_room<R>(&mut self, id: RoomId, f: impl FnOnce(&mut Room) -> R) -> Option<R> {
        self.rooms.get_mut(&id).map(f)
    }

    fn deliver(&mut self, conn: ConnId, payload: Vec<u8>) {
        if let Some(sender) = self.senders.get(&conn) {
            if sender.try_send(payload).is_err() {
                self.dead.push(conn);
            }
        }
    }

    fn send_room(&mut self, id: RoomId, payload: Vec<u8>) {
        let conns = match self.rooms.get(&id) {
            Some(room) => room.connected_conns(),
            None => return,
        };
        for c in conns {
            self.deliver(c, payload.clone());
        }
    }

    fn send_lobby(&mut self, id: RoomId) {
        let msgs: Vec<(ConnId, Vec<u8>)> = match self.rooms.get(&id) {
            Some(room) => room.members.iter().enumerate()
                .filter_map(|(i, m)| m.conn.map(|c|
                    (c, shared::encode(&ServerMessage::Lobby { info: room.lobby_info_for(i) }))))
                .collect(),
            None => return,
        };
        for (c, payload) in msgs {
            self.deliver(c, payload);
        }
    }

    fn send_snapshot(&mut self, id: RoomId) {
        let payload = match self.rooms.get(&id) {
            Some(room) => shared::encode(&ServerMessage::StateUpdate {
                p1_board: Box::new(room.sim.boards[0].clone()),
                p2_board: Box::new(room.sim.boards[1].clone()),
                p1_ack: room.sim.last_seq[0],
                p2_ack: room.sim.last_seq[1],
            }),
            None => return,
        };
        self.send_room(id, payload);
    }

    fn room_list(&self) -> Vec<RoomInfo> {
        self.rooms.values().map(|r| r.info()).collect()
    }

    fn broadcast_room_list(&mut self) {
        let payload = shared::encode(&ServerMessage::RoomList { rooms: self.room_list() });
        let browsing: Vec<ConnId> = self.clients.iter()
            .filter_map(|(&c, loc)| loc.is_none().then_some(c))
            .collect();
        for c in browsing {
            self.deliver(c, payload.clone());
        }
    }

    fn send_room_list_to(&mut self, conn: ConnId) {
        let payload = shared::encode(&ServerMessage::RoomList { rooms: self.room_list() });
        self.deliver(conn, payload);
    }

    fn room_of_token(&self, token: &str) -> Option<RoomId> {
        self.rooms.values()
            .find(|r| r.members.iter().any(|m| m.token == token))
            .map(|r| r.id)
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Register { conn, sender } => {
                self.senders.insert(conn, sender);
                self.clients.insert(conn, None);
            }
            Command::Hello { conn, token } => {
                self.conn_token.insert(conn, token.clone());
                if let Some(id) = self.room_of_token(&token) {
                    self.rejoin(conn, id);
                } else {
                    self.send_room_list_to(conn);
                }
            }
            Command::Unregister { conn } => {
                self.drop_connection(conn);
            }
            Command::RequestRoomList { conn } => {
                self.send_room_list_to(conn);
            }
            Command::CreateRoom { conn, name } => {
                let token = match self.conn_token.get(&conn) {
                    Some(t) => t.clone(),
                    None => return,
                };
                self.leave_current(conn);
                let id = self.next_id;
                self.next_id += 1;
                let settings = RoomSettings::default();
                let room = Room {
                    id,
                    name: clean_name(name),
                    host: token.clone(),
                    members: vec![Member { token, conn: Some(conn), disconnect_at: None }],
                    settings,
                    phase: Phase::Lobby,
                    sim: Sim::new(&settings),
                };
                self.rooms.insert(id, room);
                self.clients.insert(conn, Some(id));
                self.send_lobby(id);
                self.room_list_dirty = true;
                println!(">>> Room {} créée par {}.", id, conn);
            }
            Command::JoinRoom { conn, id } => {
                let token = match self.conn_token.get(&conn) {
                    Some(t) => t.clone(),
                    None => return,
                };
                let joinable = matches!(self.rooms.get(&id), Some(r) if r.members.len() < 2);
                if !joinable {
                    self.deliver(conn,
                        shared::encode(&ServerMessage::JoinFailed { reason: "Room indisponible".into() }));
                    return;
                }
                self.leave_current(conn);
                let pushed = self.with_room(id, |room| {
                    if room.members.len() < 2 {
                        room.members.push(Member { token, conn: Some(conn), disconnect_at: None });
                        true
                    } else {
                        false
                    }
                }).unwrap_or(false);
                if pushed {
                    self.clients.insert(conn, Some(id));
                    self.sync_after_attach(id, conn);
                } else {
                    self.deliver(conn,
                        shared::encode(&ServerMessage::JoinFailed { reason: "Room indisponible".into() }));
                }
            }
            Command::LeaveRoom { conn } => {
                self.leave_current(conn);
                self.send_room_list_to(conn);
            }
            Command::SetSetting { conn, index, dir } => {
                if let Some(id) = self.room_of(conn) {
                    let changed = self.with_room(id, |room| {
                        if room.is_host_conn(conn) && matches!(room.phase, Phase::Lobby) {
                            room.settings.adjust(index as usize, dir);
                            true
                        } else {
                            false
                        }
                    }).unwrap_or(false);
                    if changed {
                        self.send_lobby(id);
                    }
                }
            }
            Command::ToggleCountdown { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let toggled = self.with_room(id, |room| {
                        if room.is_host_conn(conn) {
                            room.phase = match room.phase {
                                Phase::Lobby if room.members.len() >= 2 => Phase::CountingDown(3.0),
                                Phase::Lobby => Phase::Lobby,
                                Phase::CountingDown(_) => Phase::Lobby,
                                Phase::Playing => Phase::Playing,
                            };
                            true
                        } else {
                            false
                        }
                    }).unwrap_or(false);
                    if toggled {
                        self.send_lobby(id);
                        self.room_list_dirty = true;
                    }
                }
            }
            Command::ReturnToLobby { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let done = self.with_room(id, |room| {
                        if room.is_host_conn(conn) {
                            room.phase = Phase::Lobby;
                            room.sim.finished = false;
                            room.sim.paused = false;
                            true
                        } else {
                            false
                        }
                    }).unwrap_or(false);
                    if done {
                        self.send_lobby(id);
                        self.room_list_dirty = true;
                    }
                }
            }
            Command::Input { conn, kind, seq } => {
                if let Some(id) = self.room_of(conn) {
                    self.with_room(id, |room| {
                        if let Some(idx) = room.slot_of_conn(conn) {
                            room.sim.last_seq[idx] = seq;
                            if matches!(room.phase, Phase::Playing) && !room.sim.paused && !room.sim.finished {
                                room.sim.boards[idx].apply_input(kind);
                            }
                        }
                    });
                }
            }
            Command::TogglePause { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let toggled = self.with_room(id, |room| {
                        if matches!(room.phase, Phase::Playing) && !room.sim.finished {
                            room.sim.paused = !room.sim.paused;
                            let p = room.sim.paused;
                            room.sim.boards.iter_mut().for_each(|b| b.set_paused(p));
                            true
                        } else {
                            false
                        }
                    }).unwrap_or(false);
                    if toggled {
                        self.send_snapshot(id);
                    }
                }
            }

            Command::Restart { conn } => {
                if let Some(id) = self.room_of(conn) {
                    let restarted = self.with_room(id, |room| {
                        if matches!(room.phase, Phase::Playing) {
                            let now = Instant::now();
                            let in_cooldown = room.sim.last_restart
                                .is_some_and(|t| now.duration_since(t) < Duration::from_secs(2));
                            if !in_cooldown {
                                room.sim.last_restart = Some(now);
                                room.sim.reset_boards(&room.settings);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }).unwrap_or(false);
                    if restarted {
                        self.send_room(id, shared::encode(&ServerMessage::Restart));
                    }
                }
            }
        }
    }

    fn rejoin(&mut self, conn: ConnId, id: RoomId) {
        let token = match self.conn_token.get(&conn) {
            Some(t) => t.clone(),
            None => return,
        };
        let mut replaced: Option<ConnId> = None;
        if let Some(room) = self.rooms.get_mut(&id) {
            if let Some(slot) = room.members.iter().position(|m| m.token == token) {
                replaced = room.members[slot].conn.filter(|&c| c != conn);
                room.members[slot].conn = Some(conn);
                room.members[slot].disconnect_at = None;
            }
        }
        self.clients.insert(conn, Some(id));
        if let Some(old) = replaced {
            self.clients.remove(&old);
            self.conn_token.remove(&old);
            self.senders.remove(&old);
        }
        self.sync_after_attach(id, conn);
        println!(">>> Reconnexion sur room {} (conn {}).", id, conn);
    }

    fn sync_after_attach(&mut self, id: RoomId, conn: ConnId) {
        let resume = if let Some(room) = self.rooms.get_mut(&id) {
            let playing = matches!(room.phase, Phase::Playing);
            let all = room.all_connected();
            if playing && all {
                room.sim.paused = false;
                room.sim.boards.iter_mut().for_each(|b| b.set_paused(false));
            }
            (playing, all)
        } else {
            return;
        };

        self.send_lobby(id);
        match resume {
            (true, true) => {
                self.send_room(id, shared::encode(&ServerMessage::GameStart));
            }
            (true, false) => {
                self.deliver(conn, shared::encode(&ServerMessage::GameStart));
                self.deliver(conn, shared::encode(&ServerMessage::OpponentDisconnected));
            }
            _ => {}
        }
        if resume.0 {
            self.send_snapshot(id);
        }
        self.room_list_dirty = true;
    }

    fn mark_disconnected(&mut self, conn: ConnId) {
        let id = match self.room_of(conn) {
            Some(id) => id,
            None => return,
        };
        let (notify, in_game) = if let Some(room) = self.rooms.get_mut(&id) {
            let slot = match room.slot_of_conn(conn) {
                Some(s) => s,
                None => return,
            };
            room.members[slot].conn = None;
            room.members[slot].disconnect_at = Some(Instant::now());
            let playing = matches!(room.phase, Phase::Playing);
            let notify = playing && !room.sim.finished;
            if notify {
                room.sim.paused = true;
                room.sim.boards.iter_mut().for_each(|b| b.set_paused(true));
            }
            (notify, playing)
        } else {
            return;
        };

        if notify {
            self.send_room(id, shared::encode(&ServerMessage::OpponentDisconnected));
        }
        if !in_game {
            self.send_lobby(id);
        }
        println!("Déconnexion conn {} (grâce {}s).", conn, GRACE.as_secs());
    }

    fn drop_connection(&mut self, conn: ConnId) {
        self.mark_disconnected(conn);
        self.clients.remove(&conn);
        self.conn_token.remove(&conn);
        self.senders.remove(&conn);
    }

    fn reap_dead(&mut self) {
        while !self.dead.is_empty() {
            for conn in std::mem::take(&mut self.dead) {
                if self.senders.contains_key(&conn) {
                    println!("Conn {} trop lente : fermeture.", conn);
                    self.drop_connection(conn);
                }
            }
        }
    }

    fn leave_current(&mut self, conn: ConnId) {
        let id = match self.room_of(conn) {
            Some(id) => id,
            None => return,
        };
        self.clients.insert(conn, None);
        let slot = match self.rooms.get(&id).and_then(|r| r.slot_of_conn(conn)) {
            Some(s) => s,
            None => return,
        };
        self.remove_member(id, slot);
    }

    fn remove_member(&mut self, id: RoomId, slot: usize) {
        let closed = if let Some(room) = self.rooms.get_mut(&id) {
            if slot >= room.members.len() {
                return;
            }
            let removed = room.members.remove(slot);
            if room.members.is_empty() {
                true
            } else {
                if removed.token == room.host {
                    room.host = room.members[0].token.clone();
                }
                if room.members.len() < 2 && !matches!(room.phase, Phase::Lobby) {
                    room.phase = Phase::Lobby;
                    room.sim.finished = false;
                    room.sim.paused = false;
                }
                false
            }
        } else {
            return;
        };

        if closed {
            self.rooms.remove(&id);
        } else {
            self.send_lobby(id);
        }
        self.room_list_dirty = true;
    }

    fn tick(&mut self, dt: f32, do_broadcast: bool) {
        if self.room_list_dirty && self.last_room_list.elapsed() >= Duration::from_millis(200) {
            self.broadcast_room_list();
            self.room_list_dirty = false;
            self.last_room_list = Instant::now();
        }

        let now = Instant::now();
        let expired: Vec<(RoomId, Token)> = self.rooms.values()
            .flat_map(|r| r.members.iter()
                .filter(|m| m.disconnect_at.is_some_and(|t| now.duration_since(t) >= GRACE))
                .map(move |m| (r.id, m.token.clone())))
            .collect();
        for (id, token) in expired {
            if let Some(slot) = self.rooms.get(&id).and_then(|r| r.members.iter().position(|m| m.token == token)) {
                println!(">>> Grâce expirée : retrait d'un membre de la room {}.", id);
                self.remove_member(id, slot);
            }
        }

        let mut outgoing: Vec<(ConnId, Vec<u8>)> = Vec::new();
        let mut list_changed = false;

        for room in self.rooms.values_mut() {
            if matches!(room.phase, Phase::Lobby) {
                continue;
            }

            let mut start_now = false;
            let mut cd_changed = false;
            if let Phase::CountingDown(t) = &mut room.phase {
                let before = t.ceil() as u8;
                *t -= dt;
                if *t <= 0.0 {
                    start_now = true;
                } else if t.ceil() as u8 != before {
                    cd_changed = true;
                }
            }
            if start_now {
                room.phase = Phase::Playing;
                room.sim.reset_boards(&room.settings);
                let payload = shared::encode(&ServerMessage::GameStart);
                for m in &room.members {
                    if let Some(c) = m.conn {
                        outgoing.push((c, payload.clone()));
                    }
                }
                list_changed = true;
            } else if cd_changed {
                for (i, m) in room.members.iter().enumerate() {
                    if let Some(c) = m.conn {
                        outgoing.push((c, shared::encode(&ServerMessage::Lobby { info: room.lobby_info_for(i) })));
                    }
                }
            }

            if matches!(room.phase, Phase::Playing) {
                let advanced = !room.sim.paused && !room.sim.finished;
                let mut just_finished = false;
                if advanced {
                    let g0 = room.sim.boards[0].tick(dt);
                    let g1 = room.sim.boards[1].tick(dt);
                    room.sim.boards[1].pending_garbage += g0;
                    room.sim.boards[0].pending_garbage += g1;
                    if room.sim.boards[0].state == GameState::GameOver
                        || room.sim.boards[1].state == GameState::GameOver {
                        room.sim.finished = true;
                        just_finished = true;
                    }
                }
                if (do_broadcast && advanced) || just_finished {
                    let upd = shared::encode(&ServerMessage::StateUpdate {
                        p1_board: Box::new(room.sim.boards[0].clone()),
                        p2_board: Box::new(room.sim.boards[1].clone()),
                        p1_ack: room.sim.last_seq[0],
                        p2_ack: room.sim.last_seq[1],
                    });
                    for m in &room.members {
                        if let Some(c) = m.conn {
                            outgoing.push((c, upd.clone()));
                        }
                    }
                }
            }
        }

        for (conn, payload) in outgoing {
            self.deliver(conn, payload);
        }
        if list_changed {
            self.room_list_dirty = true;
        }
    }
}

struct TickProfile {
    enabled: bool,
    budget: Duration,
    sum: Duration,
    max: Duration,
    count: u32,
    since_report: Instant,
}

impl TickProfile {
    fn new() -> TickProfile {
        TickProfile {
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
            println!(
                "[tick] rooms={} avg={:?} max={:?} budget={:?} peak_load={:.1}%",
                rooms, avg, self.max, self.budget, peak_load
            );
        }
        self.sum = Duration::ZERO;
        self.max = Duration::ZERO;
        self.count = 0;
        self.since_report = Instant::now();
    }
}

async fn manager_loop(mut cmd_rx: mpsc::UnboundedReceiver<Command>) {
    let tick_dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let mut ticker = interval(Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64));
    let broadcast_period = Duration::from_secs_f64(1.0 / config::STATE_BROADCAST_HZ as f64);
    let mut mgr = Manager::new();
    let mut last_broadcast = Instant::now();
    let mut profile = TickProfile::new();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let do_broadcast = last_broadcast.elapsed() >= broadcast_period;
                if do_broadcast { last_broadcast = Instant::now(); }
                let t0 = Instant::now();
                mgr.tick(tick_dt, do_broadcast);
                profile.record(t0.elapsed(), mgr.rooms.len());
                mgr.reap_dead();
            }
            Some(cmd) = cmd_rx.recv() => {
                mgr.handle(cmd);
                mgr.reap_dead();
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // tokio-console support (only with `--features console`, see Cargo.toml).
    #[cfg(feature = "console")]
    console_subscriber::init();

    let port = config::SERVER_PORT;
    println!("Serveur Puyo sur ws://0.0.0.0:{}", port);

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
    tokio::spawn(manager_loop(cmd_rx));

    let conn_counter = Arc::new(AtomicU64::new(1));
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || cmd_tx.clone()))
        .and(warp::any().map(move || conn_counter.clone()))
        .map(|ws: warp::ws::Ws, cmd_tx, counter: Arc<AtomicU64>| {
            let conn = counter.fetch_add(1, Ordering::Relaxed);
            ws.on_upgrade(move |socket| handle_connection(socket, cmd_tx, conn))
        });

    warp::serve(ws_route).run((config::SERVER_BIND_ADDRESS, port)).await;
}

async fn handle_connection(
    ws: warp::ws::WebSocket,
    cmd_tx: mpsc::UnboundedSender<Command>,
    conn: ConnId,
) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    println!("Connexion {} ouverte.", conn);

    let _ = cmd_tx.send(Command::Register { conn, sender: to_client_tx });

    let mut send_task = tokio::spawn(async move {
        while let Some(payload) = to_client_rx.recv().await {
            if user_ws_tx.send(warp::ws::Message::binary(payload)).await.is_err() {
                break;
            }
        }
    });

    let cmd_tx_recv = cmd_tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = user_ws_rx.next().await {
            if let Ok(msg) = result {
                if msg.is_binary() {
                    if let Some(client_msg) = shared::decode::<ClientMessage>(msg.as_bytes()) {
                        let cmd = match client_msg {
                            ClientMessage::Hello { player_id } => Command::Hello { conn, token: player_id },
                            ClientMessage::Input { kind, seq } => Command::Input { conn, kind, seq },
                            ClientMessage::TogglePause => Command::TogglePause { conn },
                            ClientMessage::RequestRestart => Command::Restart { conn },
                            ClientMessage::RequestRoomList => Command::RequestRoomList { conn },
                            ClientMessage::CreateRoom { name } => Command::CreateRoom { conn, name },
                            ClientMessage::JoinRoom { id } => Command::JoinRoom { conn, id },
                            ClientMessage::LeaveRoom => Command::LeaveRoom { conn },
                            ClientMessage::SetRoomSetting { index, dir } => Command::SetSetting { conn, index, dir },
                            ClientMessage::ToggleCountdown => Command::ToggleCountdown { conn },
                            ClientMessage::ReturnToLobby => Command::ReturnToLobby { conn },
                        };
                        if cmd_tx_recv.send(cmd).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    let _ = cmd_tx.send(Command::Unregister { conn });
    println!("Connexion {} fermée.", conn);
}

#[cfg(test)]
mod tests;
