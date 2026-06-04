use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, Duration, Instant};
use warp::Filter;
use std::sync::{Arc, Mutex};
use rand::Rng;
use shared::{Board, ClientMessage, ServerMessage, InputKind, config};

// Ciblage d'un message diffusé.
// recipient=None + exclude=None  → tous les joueurs
// recipient=Some                 → uniquement ce joueur
#[derive(Clone)]
struct BroadcastMsg {
    recipient: Option<u8>,
    payload: Vec<u8>,
}

fn broadcast_all(tx: &broadcast::Sender<BroadcastMsg>, payload: Vec<u8>) {
    let _ = tx.send(BroadcastMsg { recipient: None, payload });
}

fn send_to_player(tx: &broadcast::Sender<BroadcastMsg>, player_id: u8, payload: Vec<u8>) {
    let _ = tx.send(BroadcastMsg { recipient: Some(player_id), payload });
}

// Attribution des slots : seul état partagé entre connexions. La simulation, elle, est détenue
// exclusivement par la tâche `game_loop` (pas de Mutex sur l'état de jeu).
struct Slots {
    occupied: [bool; 2],
    count: usize,
}

enum GameCommand {
    Join { player_id: u8 },
    Leave { player_id: u8 },
    Input { player_id: u8, kind: InputKind, seq: u32 },
    TogglePause,
    Restart,
}

struct Sim {
    boards: [Board; 2],
    running: bool,   // une partie est en cours (ou finie mais encore affichée)
    paused: bool,
    finished: bool,  // un joueur a perdu : on gèle la simulation
    last_seq: [u32; 2], // dernier seq d'input traité par joueur (ack renvoyé au client)
    last_restart: Option<Instant>,
}

impl Sim {
    fn new() -> Sim {
        Sim {
            boards: Self::fresh_boards(),
            running: false,
            paused: false,
            finished: false,
            last_seq: [0; 2],
            last_restart: None,
        }
    }

    fn fresh_boards() -> [Board; 2] {
        // Même seed pour les deux joueurs → file de pièces identique (équité).
        let seed: u64 = rand::rng().random();
        [
            Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, seed),
            Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, seed),
        ]
    }

    fn reset_boards(&mut self) {
        self.boards = Self::fresh_boards();
        self.boards[0].spawn_piece();
        self.boards[1].spawn_piece();
        self.paused = false;
        self.finished = false;
        self.last_seq = [0; 2];
    }
}

#[tokio::main]
async fn main() {
    let port = config::SERVER_PORT;
    println!("Serveur Puyo sur ws://0.0.0.0:{}", port);

    let (tx, _rx) = broadcast::channel::<BroadcastMsg>(config::CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<GameCommand>();
    let slots = Arc::new(Mutex::new(Slots { occupied: [false; 2], count: 0 }));

    tokio::spawn(game_loop(cmd_rx, tx.clone(), slots.clone()));

    let tx_filter = tx.clone();
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx_filter.clone()))
        .and(warp::any().map(move || cmd_tx.clone()))
        .and(warp::any().map(move || slots.clone()))
        .map(|ws: warp::ws::Ws, tx, cmd_tx, slots| {
            ws.on_upgrade(move |socket| handle_connection(socket, tx, cmd_tx, slots))
        });

    warp::serve(ws_route).run((config::SERVER_BIND_ADDRESS, port)).await;
}

async fn game_loop(
    mut cmd_rx: mpsc::UnboundedReceiver<GameCommand>,
    tx: broadcast::Sender<BroadcastMsg>,
    slots: Arc<Mutex<Slots>>,
) {
    let tick_dt = 1.0 / config::SERVER_TICK_HZ as f32;
    let mut ticker = interval(Duration::from_secs_f64(1.0 / config::SERVER_TICK_HZ as f64));
    let broadcast_period = Duration::from_secs_f64(1.0 / config::STATE_BROADCAST_HZ as f64);
    let mut sim = Sim::new();
    let mut last_broadcast = Instant::now();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if sim.running && !sim.paused && !sim.finished {
                    let g0 = sim.boards[0].tick(tick_dt);
                    let g1 = sim.boards[1].tick(tick_dt);
                    sim.boards[1].pending_garbage += g0;
                    sim.boards[0].pending_garbage += g1;

                    if sim.boards[0].state == shared::GameState::GameOver
                        || sim.boards[1].state == shared::GameState::GameOver {
                        sim.finished = true;
                        println!(">>> Partie terminée.");
                    }
                }

                if sim.running && last_broadcast.elapsed() >= broadcast_period {
                    last_broadcast = Instant::now();
                    broadcast_all(&tx, shared::encode(&ServerMessage::StateUpdate {
                        p1_board: sim.boards[0].clone(),
                        p2_board: sim.boards[1].clone(),
                        p1_ack: sim.last_seq[0],
                        p2_ack: sim.last_seq[1],
                    }));
                }
            }
            Some(cmd) = cmd_rx.recv() => {
                handle_command(&mut sim, cmd, &tx, &slots);
            }
        }
    }
}

fn handle_command(
    sim: &mut Sim,
    cmd: GameCommand,
    tx: &broadcast::Sender<BroadcastMsg>,
    slots: &Arc<Mutex<Slots>>,
) {
    match cmd {
        GameCommand::Join { player_id } => {
            sim.last_seq[(player_id - 1) as usize] = 0;
            let both = { let s = slots.lock().unwrap(); s.occupied[0] && s.occupied[1] };
            if both && !sim.running {
                println!(">>> Lancement Partie !");
                sim.reset_boards();
                sim.running = true;
                broadcast_all(tx, shared::encode(&ServerMessage::GameStart));
            } else if both && sim.running {
                println!(">>> Reconnexion J{} ! Reprise de partie.", player_id);
                sim.paused = false;
                sim.boards.iter_mut().for_each(|b| b.set_paused(false));
                broadcast_all(tx, shared::encode(&ServerMessage::GameStart));
            }
        }
        GameCommand::Leave { player_id } => {
            let (count, occupied) = { let s = slots.lock().unwrap(); (s.count, s.occupied) };
            if count == 0 {
                println!("Salle vide : réinitialisation.");
                sim.running = false;
                sim.paused = false;
                sim.finished = false;
            } else if count == 1 && sim.running {
                println!("Adversaire de J{} parti : mise en pause.", player_id);
                sim.paused = true;
                sim.boards.iter_mut().for_each(|b| b.set_paused(true));
                let present_id = if occupied[0] { 1 } else { 2 };
                send_to_player(tx, present_id, shared::encode(&ServerMessage::OpponentDisconnected));
            }
        }
        GameCommand::Input { player_id, kind, seq } => {
            let idx = (player_id - 1) as usize;
            sim.last_seq[idx] = seq;
            if sim.running && !sim.paused && !sim.finished {
                sim.boards[idx].apply_input(kind);
            }
        }
        GameCommand::TogglePause => {
            if sim.running && !sim.finished {
                sim.paused = !sim.paused;
                let paused = sim.paused;
                sim.boards.iter_mut().for_each(|b| b.set_paused(paused));
                println!("Pause globale : {}", paused);
            }
        }
        GameCommand::Restart => {
            let now = Instant::now();
            let in_cooldown = sim.last_restart
                .map_or(false, |t| now.duration_since(t) < Duration::from_secs(2));
            if sim.running && !in_cooldown {
                sim.last_restart = Some(now);
                sim.reset_boards();
                broadcast_all(tx, shared::encode(&ServerMessage::Restart));
                println!(">>> Redémarrage de la partie.");
            }
        }
    }
}

async fn handle_connection(
    ws: warp::ws::WebSocket,
    tx: broadcast::Sender<BroadcastMsg>,
    cmd_tx: mpsc::UnboundedSender<GameCommand>,
    slots: Arc<Mutex<Slots>>,
) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let mut rx = tx.subscribe();

    let my_id: Option<u8> = {
        let mut s = slots.lock().unwrap();
        if s.count >= 2 {
            None
        } else {
            let slot = s.occupied.iter().position(|&o| !o).expect("slot libre garanti");
            s.occupied[slot] = true;
            s.count += 1;
            Some((slot + 1) as u8)
        }
    };

    let my_id = match my_id {
        None => {
            println!("Salle pleine, connexion refusée.");
            let _ = user_ws_tx.send(warp::ws::Message::binary(shared::encode(&ServerMessage::RoomFull))).await;
            return;
        }
        Some(id) => id,
    };
    println!("J{} connecté.", my_id);

    let _ = user_ws_tx.send(warp::ws::Message::binary(
        shared::encode(&ServerMessage::Welcome { player_id: my_id })
    )).await;
    let _ = cmd_tx.send(GameCommand::Join { player_id: my_id });

    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let for_me = msg.recipient.map_or(true, |r| r == my_id);
                    if for_me {
                        if user_ws_tx.send(warp::ws::Message::binary(msg.payload)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("J{}: {} messages perdus (lag), rattrapage au prochain StateUpdate.", my_id, n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
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
                            ClientMessage::Input { kind, seq } => GameCommand::Input { player_id: my_id, kind, seq },
                            ClientMessage::TogglePause => GameCommand::TogglePause,
                            ClientMessage::RequestRestart => GameCommand::Restart,
                        };
                        if cmd_tx_recv.send(cmd).is_err() { break; }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    {
        let mut s = slots.lock().unwrap();
        if s.count > 0 { s.count -= 1; }
        let slot = (my_id as usize).saturating_sub(1);
        if slot < 2 { s.occupied[slot] = false; }
    }
    let _ = cmd_tx.send(GameCommand::Leave { player_id: my_id });
    println!("J{} déconnecté.", my_id);
}
