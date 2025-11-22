use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast};
use warp::Filter;
use std::sync::{Arc, Mutex};
use rand::Rng;
use shared::{ServerMessage, ClientMessage};

struct GameState {
    player_count: usize,
    seed: u64,
    is_running: bool, 
    is_paused: bool, 
}

#[tokio::main]
async fn main() {
    let port = 8080;
    println!("Serveur Puyo sur ws://0.0.0.0:{}", port);

    let mut rng = rand::rng();
    let game_seed: u64 = rng.random();
    
    let game_state = Arc::new(Mutex::new(GameState {
        player_count: 0,
        seed: game_seed,
        is_running: false,
        is_paused: false, 
    }));

    let (tx, _rx) = broadcast::channel(100);

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx.clone()))
        .and(warp::any().map(move || game_state.clone()))
        .map(|ws: warp::ws::Ws, tx, state| {
            ws.on_upgrade(move |socket| handle_connection(socket, tx, state))
        });

    warp::serve(ws_route).run(([0, 0, 0, 0], port)).await;
}

async fn handle_connection(
    ws: warp::ws::WebSocket, 
    tx: broadcast::Sender<String>, 
    state: Arc<Mutex<GameState>>
) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let mut rx = tx.subscribe();

    let my_id;
    let seed;
    let should_start_game;
    let is_reconnecting;

    {
        let mut gs = state.lock().unwrap();
        gs.player_count += 1;
        my_id = gs.player_count as u8;
        seed = gs.seed;
        
        is_reconnecting = gs.is_running && gs.player_count == 2;
        should_start_game = !gs.is_running && gs.player_count == 2;
        
        if should_start_game {
            gs.is_running = true;
            gs.is_paused = false; 
        }
        println!("J{} connecté. Total: {} (Reco: {})", my_id, gs.player_count, is_reconnecting);
    }

    let welcome_msg = ServerMessage::Welcome { player_id: my_id, random_seed: seed };
    if let Ok(json) = serde_json::to_string(&welcome_msg) {
        let _ = user_ws_tx.send(warp::ws::Message::text(json)).await;
    }

    if should_start_game {
        println!(">>> Lancement Partie !");
        let start_msg = ServerMessage::GameStart;
        let _ = tx.send(serde_json::to_string(&start_msg).unwrap());
    } else if is_reconnecting {
        println!(">>> Reconnexion J{} ! Demande Snapshot...", my_id);
        let req_msg = ServerMessage::RequestSnapshot { requester_id: my_id };
        let _ = tx.send(serde_json::to_string(&req_msg).unwrap());
    }

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if user_ws_tx.send(warp::ws::Message::text(msg)).await.is_err() { break; }
        }
    });
    
    let tx_for_task = tx.clone();
    let state_for_task = state.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = user_ws_rx.next().await {
            if let Ok(msg) = result {
                if let Ok(text) = msg.to_str() {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(text) {
                        match client_msg {
                            ClientMessage::TogglePause => {
                                let new_pause_state;
                                {
                                    let mut gs = state_for_task.lock().unwrap();
                                    gs.is_paused = !gs.is_paused; 
                                    new_pause_state = gs.is_paused;
                                }
                                println!("Pause Globale: {}", new_pause_state);
                                let msg = ServerMessage::GameStateChange { paused: new_pause_state };
                                let _ = tx_for_task.send(serde_json::to_string(&msg).unwrap());
                            },
                            
                            ClientMessage::FullGameState { my_board, opponent_board, scores, requester_id } => {
                                println!("Transfert Snapshot vers J{}...", requester_id);
                                
                                let sync_msg = ServerMessage::SyncState {
                                    my_board: opponent_board,
                                    opponent_board: my_board,
                                    scores: (scores.1, scores.0),
                                    target_player_id: requester_id
                                };
                                let _ = tx_for_task.send(serde_json::to_string(&sync_msg).unwrap());
                            },
                            
                            ClientMessage::PieceLocked { col, rot, axis_color_idx, sat_color_idx } => {
                                let server_msg = ServerMessage::OpponentAction {
                                    player_id: my_id, col, rot, axis_color_idx, sat_color_idx
                                };
                                let _ = tx_for_task.send(serde_json::to_string(&server_msg).unwrap());
                            },
                            ClientMessage::GameOver => {
                                let _ = tx_for_task.send(serde_json::to_string(&ServerMessage::PlayerEliminated { player_id: my_id }).unwrap());
                            },
                            ClientMessage::RequestRestart => {
                                {
                                    let mut gs = state_for_task.lock().unwrap();
                                    gs.is_paused = false;
                                }
                                let new_seed = rand::rng().random();
                                let _ = tx_for_task.send(serde_json::to_string(&ServerMessage::Restart { new_seed }).unwrap());
                            },
                            _ => {}
                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort(), };

    {
        let mut gs = state.lock().unwrap();
        if gs.player_count > 0 { gs.player_count -= 1; }
        println!("Joueur {} déconnecté.", my_id);
        
        if gs.is_running && gs.player_count == 1 {
            println!("Adversaire disparu, envoi OpponentDisconnected.");
            let msg = ServerMessage::OpponentDisconnected;
            let _ = tx.send(serde_json::to_string(&msg).unwrap());
            gs.is_paused = true;
        } else if gs.player_count == 0 {
            gs.is_running = false;
            gs.is_paused = false;
        }
    }
}
