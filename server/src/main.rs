use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use warp::Filter;
use std::sync::{Arc, Mutex};
use rand::Rng;
use shared::{ClientMessage, ServerMessage, config};

// recipient=None + exclude=None  → tous les joueurs
// recipient=None + exclude=Some  → tous sauf exclu (= l'adversaire)
// recipient=Some + exclude=None  → uniquement ce joueur
#[derive(Clone)]
struct BroadcastMsg {
    recipient: Option<u8>,
    exclude: Option<u8>,
    payload: Vec<u8>,
}

fn broadcast_all(tx: &broadcast::Sender<BroadcastMsg>, payload: Vec<u8>) {
    let _ = tx.send(BroadcastMsg { recipient: None, exclude: None, payload });
}

fn send_to_opponent(tx: &broadcast::Sender<BroadcastMsg>, my_id: u8, payload: Vec<u8>) {
    let _ = tx.send(BroadcastMsg { recipient: None, exclude: Some(my_id), payload });
}

struct GameState {
    player_count: usize,
    slots: [bool; 2],
    seed: u64,
    is_running: bool,
    is_paused: bool,
    last_restart: Option<std::time::Instant>,
}

#[tokio::main]
async fn main() {
    let port = config::SERVER_PORT;
    println!("Serveur Puyo sur ws://0.0.0.0:{}", port);

    let game_seed: u64 = rand::rng().random();

    let game_state = Arc::new(Mutex::new(GameState {
        player_count: 0,
        slots: [false; 2],
        seed: game_seed,
        is_running: false,
        is_paused: false,
        last_restart: None,
    }));

    let (tx, _rx) = broadcast::channel::<BroadcastMsg>(config::CHANNEL_CAPACITY);

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx.clone()))
        .and(warp::any().map(move || game_state.clone()))
        .map(|ws: warp::ws::Ws, tx, state| {
            ws.on_upgrade(move |socket| handle_connection(socket, tx, state))
        });

    warp::serve(ws_route).run((config::SERVER_BIND_ADDRESS, port)).await;
}

async fn handle_connection(
    ws: warp::ws::WebSocket,
    tx: broadcast::Sender<BroadcastMsg>,
    state: Arc<Mutex<GameState>>,
) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let mut rx = tx.subscribe();

    // Vérification + attribution du slot en un seul verrou pour éviter le TOCTOU
    let join_result: Option<(u8, u64, bool, bool)> = {
        let mut gs = state.lock().unwrap();
        if gs.player_count >= 2 {
            None
        } else {
            let slot = match gs.slots.iter().position(|&occupied| !occupied) {
                Some(s) => s,
                None => {
                    eprintln!("État incohérent : aucun slot disponible malgré player_count < 2.");
                    return;
                }
            };
            gs.slots[slot] = true;
            gs.player_count += 1;
            let my_id = (slot + 1) as u8;
            let seed = gs.seed;
            let is_reconnecting = gs.is_running && gs.player_count == 2;
            let should_start_game = !gs.is_running && gs.player_count == 2;
            if should_start_game {
                gs.is_running = true;
                gs.is_paused = false;
            } else if is_reconnecting {
                gs.is_paused = false;
            }
            println!("J{} connecté (slot {}). Total: {} (Reco: {})", my_id, slot + 1, gs.player_count, is_reconnecting);
            Some((my_id, seed, should_start_game, is_reconnecting))
        }
    };

    let (my_id, seed, should_start_game, is_reconnecting) = match join_result {
        None => {
            println!("Salle pleine, connexion refusée.");
            let _ = user_ws_tx.send(warp::ws::Message::binary(shared::encode(&ServerMessage::RoomFull))).await;
            return;
        }
        Some(v) => v,
    };

    let _ = user_ws_tx.send(warp::ws::Message::binary(
        shared::encode(&ServerMessage::Welcome { player_id: my_id, random_seed: seed })
    )).await;

    if should_start_game {
        println!(">>> Lancement Partie !");
        broadcast_all(&tx, shared::encode(&ServerMessage::GameStart));
    } else if is_reconnecting {
        println!(">>> Reconnexion J{} ! Reprise de partie...", my_id);
        broadcast_all(&tx, shared::encode(&ServerMessage::GameStart));
        // RequestSnapshot va uniquement chez le fournisseur (pas l'echo vers le demandeur)
        send_to_opponent(&tx, my_id, shared::encode(&ServerMessage::RequestSnapshot { requester_id: my_id }));
    }

    let tx_for_send = tx.clone();
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let for_me = msg.recipient.map_or(true, |r| r == my_id)
                        && msg.exclude.map_or(true, |e| e != my_id);
                    if for_me {
                        if user_ws_tx.send(warp::ws::Message::binary(msg.payload)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("J{}: {} messages perdus (broadcast lag). Demande de resync.", my_id, n);
                    send_to_opponent(&tx_for_send, my_id, shared::encode(&ServerMessage::RequestSnapshot { requester_id: my_id }));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let tx_for_task = tx.clone();
    let state_for_task = state.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = user_ws_rx.next().await {
            if let Ok(msg) = result {
                if msg.is_binary() {
                    if let Some(client_msg) = shared::decode::<ClientMessage>(msg.as_bytes()) {
                        match client_msg {
                            ClientMessage::TogglePause => {
                                let new_pause_state = {
                                    let mut gs = state_for_task.lock().unwrap();
                                    gs.is_paused = !gs.is_paused;
                                    gs.is_paused
                                };
                                println!("Pause Globale: {}", new_pause_state);
                                broadcast_all(&tx_for_task, shared::encode(&ServerMessage::GameStateChange { paused: new_pause_state }));
                            }

                            ClientMessage::FullGameState { my_board, opponent_board, requester_id, my_piece_seq } => {
                                println!("Transfert Snapshot vers J{}...", requester_id);
                                let sync_msg = ServerMessage::SyncState {
                                    my_board: opponent_board,
                                    opponent_board: my_board,
                                    target_player_id: requester_id,
                                    opponent_piece_seq: my_piece_seq,
                                };
                                broadcast_all(&tx_for_task, shared::encode(&sync_msg));
                            }

                            ClientMessage::RequestSync => {
                                send_to_opponent(&tx_for_task, my_id, shared::encode(&ServerMessage::RequestSnapshot { requester_id: my_id }));
                            }

                            ClientMessage::PieceLocked { col, rot, axis_type, sat_type, seq } => {
                                if col < 0 || col >= config::GRID_WIDTH as i32 || rot > 3
                                    || axis_type == shared::PuyoType::Garbage || sat_type == shared::PuyoType::Garbage {
                                    eprintln!("J{} action invalide ignorée: col={} rot={}", my_id, col, rot);
                                } else {
                                    send_to_opponent(&tx_for_task, my_id, shared::encode(&ServerMessage::OpponentAction {
                                        player_id: my_id, col, rot, axis_type, sat_type, seq,
                                    }));
                                }
                            }

                            ClientMessage::BoardSync { cells, pending_garbage, score, next_types, next_next_types } => {
                                send_to_opponent(&tx_for_task, my_id, shared::encode(&ServerMessage::OpponentBoardSync {
                                    player_id: my_id, cells, pending_garbage, score, next_types, next_next_types,
                                }));
                            }

                            ClientMessage::GarbageSent { amount } => {
                                send_to_opponent(&tx_for_task, my_id, shared::encode(&ServerMessage::GarbageAttack { attacker_id: my_id, amount }));
                            }

                            ClientMessage::GameOver => {
                                send_to_opponent(&tx_for_task, my_id, shared::encode(&ServerMessage::PlayerEliminated { player_id: my_id }));
                            }

                            ClientMessage::RequestRestart => {
                                let maybe_seed = {
                                    let mut gs = state_for_task.lock().unwrap();
                                    let now = std::time::Instant::now();
                                    let in_cooldown = gs.last_restart
                                        .map_or(false, |t| now.duration_since(t) < std::time::Duration::from_secs(2));
                                    if in_cooldown {
                                        None
                                    } else {
                                        let seed: u64 = rand::rng().random();
                                        gs.seed = seed;
                                        gs.is_paused = false;
                                        gs.last_restart = Some(now);
                                        Some(seed)
                                    }
                                };
                                if let Some(new_seed) = maybe_seed {
                                    broadcast_all(&tx_for_task, shared::encode(&ServerMessage::Restart { new_seed }));
                                }
                            }

                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    {
        let mut gs = state.lock().unwrap();
        if gs.player_count > 0 { gs.player_count -= 1; }
        let slot = (my_id as usize).saturating_sub(1);
        if slot < 2 { gs.slots[slot] = false; }
        println!("Joueur {} déconnecté.", my_id);

        if gs.is_running && gs.player_count == 1 {
            println!("Adversaire disparu, envoi OpponentDisconnected.");
            send_to_opponent(&tx, my_id, shared::encode(&ServerMessage::OpponentDisconnected));
            gs.is_paused = true;
        } else if gs.player_count == 0 {
            gs.is_running = false;
            gs.is_paused = false;
            gs.slots = [false; 2];
            gs.seed = rand::rng().random();
        }
    }
}
