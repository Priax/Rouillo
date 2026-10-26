use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::QueryRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use shared::{config, ClientMessage, ServerMessage};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;

use crate::api::ClientAddr;
use crate::manager::Command;
use crate::{db, ConnId};

pub const CLIENT_CHAN_CAP: usize = 128;

pub(crate) const CLIENT_SILENCE_TIMEOUT: Duration = Duration::from_secs(300);

const _: () = assert!(CLIENT_SILENCE_TIMEOUT.as_secs() as f64 > 10.0 * config::PING_INTERVAL_SECS);

pub(crate) const MAX_CLIENT_MESSAGE: usize = 64 * 1024;

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

/// What every connection draws from, whatever carries it: its number, and
/// its share of what one address may hold.
#[derive(Clone)]
pub struct Conns {
    pub(crate) counter: Arc<AtomicU64>,
    pub(crate) ips: IpConns,
}

impl Conns {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(1)),
            ips: IpConns::default(),
        }
    }
}

#[derive(Clone)]
struct WsState {
    cmd_tx: mpsc::Sender<Command>,
    pool: db::DbPool,
    conns: Conns,
}

pub fn ws_route(cmd_tx: mpsc::Sender<Command>, pool: db::DbPool, conns: Conns) -> Router {
    let state = WsState { cmd_tx, pool, conns };
    Router::new().route("/ws", get(upgrade)).with_state(state)
}

async fn upgrade(
    State(state): State<WsState>,
    ws: WebSocketUpgrade,
    query: Result<Query<WsQuery>, QueryRejection>,
    ClientAddr(client): ClientAddr,
) -> Response {
    let Ok(slot) = IpSlot::take(&state.conns.ips, client) else {
        warn!("WS refusé: trop de connexions depuis la même IP");
        return (StatusCode::TOO_MANY_REQUESTS, "Too many connections").into_response();
    };
    let conn = state.conns.counter.fetch_add(1, Ordering::Relaxed);
    let ws = ws
        .max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE);
    let version = query.ok().and_then(|Query(q)| q.v);
    if version == Some(shared::PROTOCOL_VERSION) {
        let WsState { cmd_tx, pool, .. } = state;
        ws.on_upgrade(move |socket| async move {
            handle_connection(socket, cmd_tx, conn, pool).await;
            drop(slot);
        })
    } else {
        info!(
            "WS {conn} refusé: protocole {version:?}, attendu {}",
            shared::PROTOCOL_VERSION
        );
        ws.on_upgrade(reject_outdated)
    }
}

#[derive(serde::Deserialize)]
struct WsQuery {
    v: Option<u32>,
}

// Close code for an outdated client, in the range RFC 6455 leaves to applications
// (426: HTTP's Upgrade Required).
const CLOSE_OUTDATED: u16 = 4426;

async fn reject_outdated(ws: WebSocket) {
    let (mut tx, _rx) = ws.split();
    let _ = tx.send(Message::text(shared::OUTDATED_FRAME)).await;
    let _ = tx
        .send(Message::Close(Some(CloseFrame {
            code: CLOSE_OUTDATED,
            reason: shared::OUTDATED_FRAME.into(),
        })))
        .await;
}

/// What a connection does with the messages it receives, whatever carries them.
pub(crate) struct Inbox {
    conn: ConnId,
    cmd_tx: mpsc::Sender<Command>,
    pool: db::DbPool,
    /// Pongs skip the manager, for an honest round trip.
    pong_tx: mpsc::Sender<Vec<u8>>,
    limiter: RateLimiter,
    greeted: bool,
}

impl Inbox {
    pub(crate) fn new(
        conn: ConnId,
        cmd_tx: mpsc::Sender<Command>,
        pool: db::DbPool,
        pong_tx: mpsc::Sender<Vec<u8>>,
    ) -> Self {
        Self {
            conn,
            cmd_tx,
            pool,
            pong_tx,
            limiter: RateLimiter::new(Instant::now()),
            greeted: false,
        }
    }

    /// Handles one message, `None` for one that carries no game message.
    /// Returns whether the connection stays open.
    pub(crate) async fn receive(&mut self, bytes: Option<&[u8]>) -> bool {
        self.take(bytes, false).await
    }

    /// A datagram may only carry inputs sent again.
    pub(crate) async fn receive_datagram(&mut self, bytes: &[u8]) -> bool {
        self.take(Some(bytes), true).await
    }

    async fn take(&mut self, bytes: Option<&[u8]>, datagram: bool) -> bool {
        match self.admit() {
            Verdict::Allow => {}
            Verdict::Drop => return true,
            Verdict::Disconnect => return false,
        }
        match bytes.and_then(shared::decode::<ClientMessage>) {
            Some(msg) if !datagram || matches!(msg, ClientMessage::Inputs { .. }) => self.handle(msg).await,
            _ => true,
        }
    }

    fn admit(&mut self) -> Verdict {
        let verdict = self.limiter.check(Instant::now());
        if verdict == Verdict::Disconnect {
            warn!("Connexion {} flood, fermeture", self.conn);
        }
        verdict
    }

    async fn handle(&mut self, msg: ClientMessage) -> bool {
        let conn = self.conn;
        let cmd = match msg {
            ClientMessage::Hello { .. } if self.greeted => return true,
            ClientMessage::Hello {
                player_id,
                auth_token,
                last_disconnect_reason,
            } => {
                self.greeted = true;
                let session = auth_token.as_deref().and_then(|t| Uuid::parse_str(t).ok());
                let user = match session {
                    Some(token_uuid) => db::find_user_by_token(&self.pool, token_uuid).await.ok().flatten(),
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
                let _ = self.pong_tx.try_send(shared::encode(&ServerMessage::Pong { id }));
                return true;
            }
            ClientMessage::Inputs { inputs } => {
                // Each input counts as a message; the first was counted on arrival.
                for (n, i) in inputs.into_iter().take(shared::MAX_RESENT_INPUTS).enumerate() {
                    match if n == 0 { Verdict::Allow } else { self.admit() } {
                        Verdict::Allow => {}
                        Verdict::Drop => return true,
                        Verdict::Disconnect => return false,
                    }
                    let cmd = Command::Input {
                        conn,
                        kind: i.kind,
                        seq: i.seq,
                        tick: i.tick,
                    };
                    if self.cmd_tx.send(cmd).await.is_err() {
                        return false;
                    }
                }
                return true;
            }
            other => match command(conn, other) {
                Some(cmd) => cmd,
                None => return true,
            },
        };
        self.cmd_tx.send(cmd).await.is_ok()
    }
}

/// The manager's command for a message; greetings and pings are the
/// connection's own business.
fn command(conn: ConnId, msg: ClientMessage) -> Option<Command> {
    Some(match msg {
        ClientMessage::Hello { .. } | ClientMessage::Ping { .. } | ClientMessage::Inputs { .. } => return None,
        ClientMessage::Input { kind, seq, tick } => Command::Input { conn, kind, seq, tick },
        ClientMessage::TogglePause => Command::TogglePause { conn },
        ClientMessage::RequestRestart => Command::Restart { conn },
        ClientMessage::RequestRoomList => Command::RequestRoomList { conn },
        ClientMessage::CreateRoom { name } => Command::CreateRoom { conn, name },
        ClientMessage::JoinRoom { id } => Command::JoinRoom { conn, id },
        ClientMessage::LeaveRoom => Command::LeaveRoom { conn },
        ClientMessage::SetRoomSetting { setting, dir } => Command::SetSetting { conn, setting, dir },
        ClientMessage::ToggleCountdown => Command::ToggleCountdown { conn },
        ClientMessage::ReturnToLobby => Command::ReturnToLobby { conn },
        ClientMessage::InviteFriend { user_id } => Command::InviteFriend {
            conn,
            target_user_id: user_id,
        },
        ClientMessage::JoinQueue => Command::JoinQueue { conn },
        ClientMessage::LeaveQueue => Command::LeaveQueue { conn },
        ClientMessage::AcceptMatch => Command::AcceptMatch { conn },
        ClientMessage::Chat { text } => Command::Chat { conn, text },
        ClientMessage::Spectate { id } => Command::Spectate { conn, id },
        ClientMessage::WatchFriend { user_id } => Command::WatchFriend { conn, user_id },
        ClientMessage::SetTargeting { targeting } => Command::SetTargeting { conn, targeting },
        ClientMessage::SetTeam { team } => Command::SetTeam { conn, team },
        ClientMessage::AddCpu => Command::AddCpu { conn },
        ClientMessage::SetCpu {
            slot,
            difficulty,
            style,
            team,
        } => Command::SetCpu {
            conn,
            slot,
            difficulty,
            style,
            team,
        },
        ClientMessage::RemoveCpu { slot } => Command::RemoveCpu { conn, slot },
        ClientMessage::Follow { slot } => Command::Follow { conn, slot },
    })
}

async fn handle_connection(ws: WebSocket, cmd_tx: mpsc::Sender<Command>, conn: ConnId, pool: db::DbPool) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    let pong_tx = to_client_tx.clone();
    info!("WS {conn} ouverture");

    let _ = cmd_tx
        .send(Command::Register {
            conn,
            sender: to_client_tx,
            states: None,
        })
        .await;

    let mut send_task = tokio::spawn(async move {
        while let Some(payload) = to_client_rx.recv().await {
            if user_ws_tx.send(Message::binary(payload)).await.is_err() {
                break;
            }
        }
    });

    let mut inbox = Inbox::new(conn, cmd_tx.clone(), pool, pong_tx);
    let mut recv_task = tokio::spawn(async move {
        loop {
            let result = match tokio::time::timeout(CLIENT_SILENCE_TIMEOUT, user_ws_rx.next()).await {
                Ok(Some(result)) => result,
                Ok(None) => break,
                Err(_) => {
                    warn!("WS {conn} muet depuis {}s, fermeture", CLIENT_SILENCE_TIMEOUT.as_secs());
                    break;
                }
            };
            let bytes = match &result {
                Ok(Message::Binary(bytes)) => Some(bytes.as_ref()),
                _ => None,
            };
            if !inbox.receive(bytes).await {
                break;
            }
        }
    });

    tokio::select! { _ = (&mut send_task) => recv_task.abort(), _ = (&mut recv_task) => send_task.abort() }

    let _ = cmd_tx.send(Command::Unregister { conn }).await;
    info!("WS {conn} fermeture");
}
