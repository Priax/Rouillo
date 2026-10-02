use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use shared::{config, ClientMessage, ServerMessage};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use warp::{Filter, Reply};

use crate::auth::client_addr;
use crate::manager::Command;
use crate::{db, ConnId};

pub const CLIENT_CHAN_CAP: usize = 128;

const CLIENT_SILENCE_TIMEOUT: Duration = Duration::from_secs(300);

const _: () = assert!(CLIENT_SILENCE_TIMEOUT.as_secs() as f64 > 10.0 * config::PING_INTERVAL_SECS);

const MAX_CLIENT_MESSAGE: usize = 64 * 1024;

pub const CMD_CHAN_CAP: usize = 4096;

pub const CLIENT_MSG_RATE: f64 = 400.0;
pub const CLIENT_MSG_BURST: f64 = 1200.0;

pub const FLOOD_DROPS_PER_SEC: u32 = 1000;

pub struct RateLimiter {
    tokens: f64,
    last: Instant,
    window_start: Instant,
    drops_in_window: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Drop,
    Disconnect,
}

impl RateLimiter {
    pub fn new(now: Instant) -> Self {
        Self {
            tokens: CLIENT_MSG_BURST,
            last: now,
            window_start: now,
            drops_in_window: 0,
        }
    }

    pub fn check(&mut self, now: Instant) -> Verdict {
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * CLIENT_MSG_RATE).min(CLIENT_MSG_BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Verdict::Allow;
        }
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            self.window_start = now;
            self.drops_in_window = 0;
        }
        self.drops_in_window += 1;
        if self.drops_in_window > FLOOD_DROPS_PER_SEC {
            Verdict::Disconnect
        } else {
            Verdict::Drop
        }
    }
}

pub fn bind_listener(addr: SocketAddr) -> tokio::net::TcpListener {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP)).expect("socket()");
    socket.set_tcp_nodelay(true).expect("TCP_NODELAY");
    socket.set_reuse_address(true).expect("SO_REUSEADDR");
    socket.set_nonblocking(true).expect("O_NONBLOCK");
    socket.bind(&addr.into()).expect("bind");
    socket.listen(1024).expect("listen");
    tokio::net::TcpListener::from_std(socket.into()).expect("listener")
}

const MAX_LOGGED_REASON: usize = 100;

pub(crate) fn log_safe(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_LOGGED_REASON).collect()
}

pub const MAX_WS_PER_IP: u32 = 16;

pub(crate) type IpConns = Arc<Mutex<HashMap<String, u32>>>;

pub(crate) struct IpSlot {
    conns: IpConns,
    ip: String,
}

impl IpSlot {
    pub(crate) fn take(conns: &IpConns, ip: Option<String>) -> Result<Option<Self>, ()> {
        let Some(ip) = ip else { return Ok(None) };
        let mut m = conns.lock().unwrap();
        let n = m.entry(ip.clone()).or_insert(0);
        if *n >= MAX_WS_PER_IP {
            return Err(());
        }
        *n += 1;
        drop(m);
        Ok(Some(Self {
            conns: Arc::clone(conns),
            ip,
        }))
    }
}

impl Drop for IpSlot {
    fn drop(&mut self) {
        let mut m = self.conns.lock().unwrap();
        if let Some(n) = m.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.ip);
            }
        }
    }
}

pub fn ws_route(
    cmd_tx: mpsc::Sender<Command>,
    pool: db::DbPool,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let conn_counter = Arc::new(AtomicU64::new(1));
    let ip_conns: IpConns = Arc::default();
    warp::path("ws")
        .and(warp::ws())
        .and(warp::query::<WsQuery>())
        .and(client_addr())
        .and(warp::any().map(move || cmd_tx.clone()))
        .and(warp::any().map(move || Arc::clone(&conn_counter)))
        .and(warp::any().map(move || pool.clone()))
        .and(warp::any().map(move || Arc::clone(&ip_conns)))
        .map(
            |ws: warp::ws::Ws,
             query: WsQuery,
             client: Option<String>,
             cmd_tx,
             counter: Arc<AtomicU64>,
             pool: db::DbPool,
             ip_conns: IpConns| {
                let Ok(slot) = IpSlot::take(&ip_conns, client) else {
                    warn!("WS refusé: trop de connexions depuis la même IP");
                    return warp::reply::with_status("Too many connections", warp::http::StatusCode::TOO_MANY_REQUESTS)
                        .into_response();
                };
                let conn = counter.fetch_add(1, Ordering::Relaxed);
                let ws = ws
                    .max_message_size(MAX_CLIENT_MESSAGE)
                    .max_frame_size(MAX_CLIENT_MESSAGE);
                if query.v == Some(shared::PROTOCOL_VERSION) {
                    ws.on_upgrade(move |socket| async move {
                        handle_connection(socket, cmd_tx, conn, pool).await;
                        drop(slot);
                    })
                    .into_response()
                } else {
                    info!(
                        "WS {conn} refusé: protocole {:?}, attendu {}",
                        query.v,
                        shared::PROTOCOL_VERSION
                    );
                    ws.on_upgrade(reject_outdated).into_response()
                }
            },
        )
}

#[derive(serde::Deserialize)]
struct WsQuery {
    v: Option<u32>,
}

// Close code for an outdated client, in the range RFC 6455 leaves to applications
// (426: HTTP's Upgrade Required).
const CLOSE_OUTDATED: u16 = 4426;

async fn reject_outdated(ws: warp::ws::WebSocket) {
    let (mut tx, _rx) = ws.split();
    let _ = tx.send(warp::ws::Message::text(shared::OUTDATED_FRAME)).await;
    let _ = tx
        .send(warp::ws::Message::close_with(CLOSE_OUTDATED, shared::OUTDATED_FRAME))
        .await;
}

async fn handle_connection(ws: warp::ws::WebSocket, cmd_tx: mpsc::Sender<Command>, conn: ConnId, pool: db::DbPool) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    let pong_tx = to_client_tx.clone();
    info!("WS {conn} ouverture");

    let _ = cmd_tx
        .send(Command::Register {
            conn,
            sender: to_client_tx,
        })
        .await;

    let mut send_task = tokio::spawn(async move {
        while let Some(payload) = to_client_rx.recv().await {
            if user_ws_tx.send(warp::ws::Message::binary(payload)).await.is_err() {
                break;
            }
        }
    });

    let cmd_tx_recv = cmd_tx.clone();
    let mut recv_task = tokio::spawn(async move {
        let mut limiter = RateLimiter::new(Instant::now());
        let mut greeted = false;
        loop {
            let result = match tokio::time::timeout(CLIENT_SILENCE_TIMEOUT, user_ws_rx.next()).await {
                Ok(Some(result)) => result,
                Ok(None) => break,
                Err(_) => {
                    warn!("WS {conn} muet depuis {}s, fermeture", CLIENT_SILENCE_TIMEOUT.as_secs());
                    break;
                }
            };
            match limiter.check(Instant::now()) {
                Verdict::Allow => {}
                Verdict::Drop => continue,
                Verdict::Disconnect => {
                    warn!("WS {conn} flood, fermeture");
                    break;
                }
            }
            if let Ok(msg) = result {
                if msg.is_binary() {
                    if let Some(client_msg) = shared::decode::<ClientMessage>(msg.as_bytes()) {
                        let cmd = match client_msg {
                            ClientMessage::Hello { .. } if greeted => continue,
                            ClientMessage::Hello {
                                player_id,
                                auth_token,
                                last_disconnect_reason,
                            } => {
                                greeted = true;
                                let session = auth_token.as_deref().and_then(|t| Uuid::parse_str(t).ok());
                                let user = match session {
                                    Some(token_uuid) => db::find_user_by_token(&pool, token_uuid).await.ok().flatten(),
                                    None => None,
                                };
                                Command::Hello {
                                    conn,
                                    token: player_id,
                                    session: session.filter(|_| user.is_some()),
                                    user_id: user.as_ref().map(|u| u.id),
                                    username: user.map(|u| u.username),
                                    last_disconnect_reason: last_disconnect_reason.as_deref().map(log_safe),
                                }
                            }
                            ClientMessage::Ping { id } => {
                                if let Ok(bytes) = shared::encode(&ServerMessage::Pong { id }) {
                                    let _ = pong_tx.try_send(bytes);
                                }
                                continue;
                            }
                            ClientMessage::Input { kind, seq, tick } => Command::Input { conn, kind, seq, tick },
                            ClientMessage::TogglePause => Command::TogglePause { conn },
                            ClientMessage::RequestRestart => Command::Restart { conn },
                            ClientMessage::RequestRoomList => Command::RequestRoomList { conn },
                            ClientMessage::CreateRoom { name } => Command::CreateRoom { conn, name },
                            ClientMessage::JoinRoom { id } => Command::JoinRoom { conn, id },
                            ClientMessage::LeaveRoom => Command::LeaveRoom { conn },
                            ClientMessage::SetRoomSetting { index, dir } => Command::SetSetting { conn, index, dir },
                            ClientMessage::ToggleCountdown => Command::ToggleCountdown { conn },
                            ClientMessage::ReturnToLobby => Command::ReturnToLobby { conn },
                            ClientMessage::InviteFriend { user_id } => Command::InviteFriend {
                                conn,
                                target_user_id: user_id,
                            },
                            ClientMessage::JoinQueue => Command::JoinQueue { conn },
                            ClientMessage::LeaveQueue => Command::LeaveQueue { conn },
                            ClientMessage::AcceptMatch => Command::AcceptMatch { conn },
                        };
                        if cmd_tx_recv.send(cmd).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    let _ = cmd_tx.send(Command::Unregister { conn }).await;
    info!("WS {conn} fermeture");
}
