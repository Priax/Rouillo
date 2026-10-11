//! Games over WebTransport: a reliable stream for the messages, as on the
//! WebSocket, and datagrams for the game states, where a lost one is
//! replaced by the next.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::routing::get;
use axum::{Json, Router};
use bytes::Bytes;
use rustls::crypto::ring::sign::any_supported_type;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration, Instant};
use tracing::{error, info, warn};
use web_transport_quinn::{quinn, Request, Session};

use crate::manager::Command;
use crate::ws::{Conns, Inbox, IpSlot, CLIENT_CHAN_CAP, CLIENT_SILENCE_TIMEOUT, MAX_CLIENT_MESSAGE};
use crate::{db, ConnId};

pub const PATH: &str = "/wt";

/// A few states may wait for the network; older ones are worth nothing.
const STATE_CHAN_CAP: usize = 4;

const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// A certificate and key read from files is read again once they change,
/// which a renewal does.
const RECHECK: Duration = Duration::from_secs(60);

/// A self-signed certificate is only accepted by browsers for two weeks,
/// and only through its hash.
const SELF_SIGNED_DAYS: i64 = 13;

/// How a client reaches the WebTransport server, as the API tells it.
#[derive(Clone, Serialize)]
pub struct Offer {
    pub url: String,
    /// The SHA-256 of a self-signed certificate, which a browser accepts
    /// only when told this hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_hash: Option<Vec<u8>>,
}

pub struct Config {
    pub addr: SocketAddr,
    pub url: String,
    pub files: Option<(PathBuf, PathBuf)>,
}

impl Config {
    /// From `WT_ADDR` (the UDP address to listen on), `WT_URL` (what
    /// clients dial) and `WT_CERT` and `WT_KEY` (PEM files). Without the
    /// files, a certificate is made up: for a local test.
    pub fn from_env() -> Option<Self> {
        let addr: SocketAddr = std::env::var("WT_ADDR")
            .ok()?
            .parse()
            .expect("WT_ADDR must be an address");
        let url = std::env::var("WT_URL").unwrap_or_else(|_| format!("https://127.0.0.1:{}{PATH}", addr.port()));
        let files = std::env::var("WT_CERT").ok().map(|cert| {
            (
                PathBuf::from(cert),
                PathBuf::from(std::env::var("WT_KEY").expect("WT_KEY with WT_CERT")),
            )
        });
        Some(Self { addr, url, files })
    }
}

pub fn offer_route(offer: Option<Offer>) -> Router {
    Router::new().route(
        "/api/transport",
        get(move || {
            let offer = offer.clone();
            async move { Json(offer) }
        }),
    )
}

/// Starts listening, and returns what clients must be told.
pub fn start(config: Config, cmd_tx: mpsc::Sender<Command>, pool: db::DbPool, conns: Conns) -> Option<Offer> {
    let (resolver, cert_hash): (Arc<dyn ResolvesServerCert>, _) = match config.files {
        Some((cert, key)) => match FileCert::load(cert, key) {
            Ok(resolver) => (Arc::new(resolver), None),
            Err(e) => {
                error!("WebTransport désactivé, certificat illisible: {e}");
                return None;
            }
        },
        None => {
            let (key, hash) = self_signed();
            (Arc::new(Fixed(Arc::new(key))), Some(hash))
        }
    };
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    tls.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).expect("a QUIC TLS config");
    let endpoint = match quinn::Endpoint::server(quinn::ServerConfig::with_crypto(Arc::new(crypto)), config.addr) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            error!("WebTransport désactivé, écoute impossible sur {}: {e}", config.addr);
            return None;
        }
    };
    info!("WebTransport sur {} ({})", config.addr, config.url);
    let mut server = web_transport_quinn::Server::new(endpoint);
    tokio::spawn(async move {
        while let Some(request) = server.accept().await {
            tokio::spawn(handle_request(request, cmd_tx.clone(), pool.clone(), conns.clone()));
        }
    });
    Some(Offer {
        url: config.url,
        cert_hash,
    })
}

async fn handle_request(request: Request, cmd_tx: mpsc::Sender<Command>, pool: db::DbPool, conns: Conns) {
    let version = request
        .url
        .query_pairs()
        .find(|(k, _)| k == "v")
        .and_then(|(_, v)| v.parse::<u32>().ok());
    if request.url.path() != PATH || version != Some(shared::PROTOCOL_VERSION) {
        let _ = request
            .reject(web_transport_quinn::http::StatusCode::UPGRADE_REQUIRED)
            .await;
        return;
    }
    let ip = crate::api::client_key(Some(request.conn().remote_address()), None);
    let Ok(slot) = IpSlot::take(&conns.ips, ip) else {
        warn!("WT refusé: trop de connexions depuis la même IP");
        let _ = request
            .reject(web_transport_quinn::http::StatusCode::TOO_MANY_REQUESTS)
            .await;
        return;
    };
    let session = match request.ok().await {
        Ok(session) => session,
        Err(e) => {
            warn!("WT poignée de main ratée: {e}");
            return;
        }
    };
    let conn = conns.counter.fetch_add(1, Ordering::Relaxed);
    if let Err(e) = serve(&session, cmd_tx, pool, conn).await {
        info!("WT {conn}: {e}");
    }
    session.close(0, b"");
    drop(slot);
}

async fn serve(session: &Session, cmd_tx: mpsc::Sender<Command>, pool: db::DbPool, conn: ConnId) -> Result<(), String> {
    let (mut send, mut recv) = timeout(OPEN_TIMEOUT, session.accept_bi())
        .await
        .map_err(|_| "pas de flux ouvert".to_owned())?
        .map_err(|e| e.to_string())?;
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    let (states_tx, mut states_rx) = mpsc::channel::<Vec<Bytes>>(STATE_CHAN_CAP);
    let mut inbox = Inbox::new(conn, cmd_tx.clone(), pool, to_client_tx.clone());
    info!("WT {conn} ouverture");
    let _ = cmd_tx
        .send(Command::Register {
            conn,
            sender: to_client_tx,
            states: Some(states_tx),
        })
        .await;

    let outgoing = session.clone();
    let sending = async move {
        loop {
            let open = tokio::select! {
                Some(p) = to_client_rx.recv() => write_frame(&mut send, &p).await,
                Some(boards) = states_rx.recv() => {
                    let mut open = true;
                    for board in boards {
                        let fits = board.len() <= outgoing.max_datagram_size();
                        if !(fits && outgoing.send_datagram(board.clone()).is_ok()) {
                            open &= write_frame(&mut send, &board).await;
                        }
                    }
                    open
                }
                else => false,
            };
            if !open {
                return;
            }
        }
    };
    let (frames_tx, mut frames) = mpsc::channel::<Vec<u8>>(CLIENT_CHAN_CAP);
    let reading = async move {
        loop {
            let mut len = [0; 4];
            if recv.read_exact(&mut len).await.is_err() {
                return;
            }
            let len = u32::from_le_bytes(len) as usize;
            if len > MAX_CLIENT_MESSAGE {
                warn!("WT {conn} message trop long ({len} octets), fermeture");
                return;
            }
            let mut buf = vec![0; len];
            if recv.read_exact(&mut buf).await.is_err() || frames_tx.send(buf).await.is_err() {
                return;
            }
        }
    };
    let incoming = session.clone();
    let receiving = async move {
        loop {
            let open = match timeout(CLIENT_SILENCE_TIMEOUT, async {
                tokio::select! {
                    frame = frames.recv() => match frame {
                        Some(frame) => inbox.receive(Some(&frame)).await,
                        None => false,
                    },
                    datagram = incoming.read_datagram() => match datagram {
                        Ok(bytes) => inbox.receive_datagram(&bytes).await,
                        Err(_) => false,
                    },
                }
            })
            .await
            {
                Ok(open) => open,
                Err(_) => {
                    warn!("WT {conn} muet depuis {}s, fermeture", CLIENT_SILENCE_TIMEOUT.as_secs());
                    false
                }
            };
            if !open {
                return;
            }
        }
    };
    tokio::select! {
        () = sending => {}
        () = reading => {}
        () = receiving => {}
        _ = session.closed() => {}
    }
    let _ = cmd_tx.send(Command::Unregister { conn }).await;
    info!("WT {conn} fermeture");
    Ok(())
}

/// A message on the stream, its length first.
async fn write_frame(send: &mut web_transport_quinn::SendStream, payload: &[u8]) -> bool {
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX).to_le_bytes();
    send.write_all(&len).await.is_ok() && send.write_all(payload).await.is_ok()
}

#[derive(Debug)]
struct Fixed(Arc<CertifiedKey>);

impl ResolvesServerCert for Fixed {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }
}

fn self_signed() -> (CertifiedKey, Vec<u8>) {
    let mut params =
        rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned(), "localhost".to_owned()]).expect("certificate names");
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(SELF_SIGNED_DAYS);
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("a P-256 key");
    let cert = params.self_signed(&key).expect("a self-signed certificate");
    let hash = ring::digest::digest(&ring::digest::SHA256, cert.der())
        .as_ref()
        .to_vec();
    let der = PrivateKeyDer::try_from(key.serialize_der()).expect("the key in DER");
    let signing = any_supported_type(&der).expect("a supported key");
    (CertifiedKey::new(vec![cert.der().clone()], signing), hash)
}

/// A certificate read from PEM files, read again when they change.
#[derive(Debug)]
struct FileCert {
    cert: PathBuf,
    key: PathBuf,
    current: Mutex<Loaded>,
}

#[derive(Debug)]
struct Loaded {
    key: Arc<CertifiedKey>,
    modified: Option<SystemTime>,
    checked: Instant,
}

impl FileCert {
    fn load(cert: PathBuf, key: PathBuf) -> Result<Self, String> {
        let loaded = Loaded {
            key: Arc::new(read_pair(&cert, &key)?),
            modified: modified(&cert),
            checked: Instant::now(),
        };
        Ok(Self {
            cert,
            key,
            current: Mutex::new(loaded),
        })
    }
}

impl ResolvesServerCert for FileCert {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current()
    }
}

impl FileCert {
    fn current(&self) -> Option<Arc<CertifiedKey>> {
        let mut current = self.current.lock().ok()?;
        if current.checked.elapsed() >= RECHECK {
            current.checked = Instant::now();
            let now = modified(&self.cert);
            if now != current.modified {
                match read_pair(&self.cert, &self.key) {
                    Ok(key) => {
                        info!("WebTransport: nouveau certificat chargé");
                        current.key = Arc::new(key);
                        current.modified = now;
                    }
                    Err(e) => warn!("WebTransport: certificat renouvelé illisible, l'ancien reste: {e}"),
                }
            }
        }
        Some(Arc::clone(&current.key))
    }
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn read_pair(cert: &PathBuf, key: &PathBuf) -> Result<CertifiedKey, String> {
    let chain = CertificateDer::pem_file_iter(cert)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|e| format!("{}: {e}", cert.display()))?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("{}: {e}", key.display()))?;
    let signing = any_supported_type(&key).map_err(|e| e.to_string())?;
    Ok(CertifiedKey::new(chain, signing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pair(dir: &std::path::Path) -> Vec<u8> {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("a key");
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
            .expect("params")
            .self_signed(&key)
            .expect("a certificate");
        std::fs::write(dir.join("cert.pem"), cert.pem()).expect("write");
        std::fs::write(dir.join("key.pem"), key.serialize_pem()).expect("write");
        cert.der().to_vec()
    }

    #[test]
    fn a_renewed_certificate_is_picked_up() {
        let dir = std::env::temp_dir().join(format!("puyo-wt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a dir");
        let first = write_pair(&dir);
        let files = FileCert::load(dir.join("cert.pem"), dir.join("key.pem")).expect("loaded");
        let der = |f: &FileCert| f.current().expect("a key").cert[0].to_vec();
        assert_eq!(der(&files), first);

        std::thread::sleep(Duration::from_millis(20));
        let second = write_pair(&dir);
        assert_eq!(der(&files), first, "read again before the recheck");
        files.current.lock().unwrap().checked -= RECHECK;
        assert_eq!(der(&files), second);

        std::fs::write(dir.join("key.pem"), "garbage").expect("write");
        std::fs::write(dir.join("cert.pem"), "garbage").expect("write");
        files.current.lock().unwrap().checked -= RECHECK;
        assert_eq!(der(&files), second, "a broken renewal keeps the old one");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
