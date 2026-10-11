use web_transport_quinn::{ClientBuilder, Session};

use super::connection::commands_until;
use super::*;
use crate::wt::{Config, Offer};

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("a free port")
        .port()
}

async fn wt_server() -> (Offer, mpsc::Receiver<Command>) {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], free_udp_port()).into();
    let config = Config {
        addr,
        url: format!("https://127.0.0.1:{}{}", addr.port(), crate::wt::PATH),
        files: None,
    };
    let offer = crate::wt::start(config, tx, lazy_pool(), crate::ws::Conns::new()).expect("listening");
    (offer, rx)
}

async fn dial(offer: &Offer, version: u32) -> Result<Session, String> {
    let client = ClientBuilder::new()
        .with_server_certificate_hashes(vec![offer.cert_hash.clone().expect("a self-signed hash")])
        .map_err(|e| e.to_string())?;
    let url: url::Url = format!("{}?v={version}", offer.url).parse().expect("a url");
    client.connect(url).await.map_err(|e| e.to_string())
}

async fn write_frame(send: &mut web_transport_quinn::SendStream, msg: &ClientMessage) {
    let bytes = shared::encode(msg).expect("encode");
    send.write_all(&u32::try_from(bytes.len()).unwrap().to_le_bytes())
        .await
        .expect("write");
    send.write_all(&bytes).await.expect("write");
}

async fn read_frame(recv: &mut web_transport_quinn::RecvStream) -> ServerMessage {
    let mut len = [0; 4];
    recv.read_exact(&mut len).await.expect("a length");
    let mut buf = vec![0; u32::from_le_bytes(len) as usize];
    recv.read_exact(&mut buf).await.expect("a payload");
    shared::decode(&buf).expect("a message")
}

fn within<F: std::future::Future>(f: F) -> tokio::time::Timeout<F> {
    tokio::time::timeout(Duration::from_secs(5), f)
}

#[tokio::test]
async fn a_webtransport_client_plays_over_a_stream_and_datagrams() {
    let (offer, mut rx) = wt_server().await;
    let session = within(dial(&offer, shared::PROTOCOL_VERSION))
        .await
        .expect("in time")
        .expect("connected");
    let (mut send, mut recv) = session.open_bi().await.expect("a stream");
    write_frame(&mut send, &ClientMessage::Ping { id: 7 }).await;
    assert!(matches!(
        within(read_frame(&mut recv)).await.expect("a pong"),
        ServerMessage::Pong { id: 7 }
    ));

    write_frame(&mut send, &ClientMessage::RequestRoomList).await;
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    let Some(Command::Register {
        sender,
        states: Some(states),
        ..
    }) = cmds.into_iter().next()
    else {
        panic!("registered without a way for states");
    };

    sender
        .try_send(shared::encode(&ServerMessage::Maintenance).unwrap())
        .unwrap();
    assert!(matches!(
        within(read_frame(&mut recv)).await.expect("a message"),
        ServerMessage::Maintenance
    ));

    let board = |count| bytes::Bytes::from(shared::encode(&ServerMessage::Spectators { count }).unwrap());
    states.try_send(vec![board(3), board(4)]).unwrap();
    for count in [3, 4] {
        let datagram = within(session.read_datagram())
            .await
            .expect("a datagram")
            .expect("read");
        assert!(
            matches!(shared::decode(&datagram), Some(ServerMessage::Spectators { count: c }) if c == count),
            "each board in its own datagram"
        );
    }

    let big = ServerMessage::Chat {
        from: "x".into(),
        text: (0..4000)
            .map(|_| char::from(b'a' + rand::RngExt::random_range(&mut rand::rng(), 0..26u8)))
            .collect(),
        spectator: false,
    };
    let bytes = shared::encode(&big).unwrap();
    assert!(
        bytes.len() > session.max_datagram_size(),
        "setup: {} bytes fit in {}",
        bytes.len(),
        session.max_datagram_size()
    );
    states.try_send(vec![bytes.into()]).unwrap();
    assert!(
        matches!(
            within(read_frame(&mut recv)).await.expect("on the stream"),
            ServerMessage::Chat { .. }
        ),
        "a state too big for a datagram takes the stream"
    );

    let input = |seq| shared::SentInput {
        seq,
        tick: 9,
        kind: InputKind::HardDrop,
    };
    let datagram = |msg: &ClientMessage| bytes::Bytes::from(shared::encode(msg).unwrap());
    session
        .send_datagram(datagram(&ClientMessage::RequestRoomList))
        .unwrap();
    session
        .send_datagram(datagram(&ClientMessage::Inputs {
            inputs: vec![input(1), input(2)],
        }))
        .unwrap();
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::Input { seq: 2, .. })).await;
    assert!(
        matches!(
            cmds.as_slice(),
            [Command::Input { seq: 1, .. }, Command::Input { seq: 2, .. }]
        ),
        "a datagram carries inputs only"
    );

    drop(send);
    session.close(0, b"bye");
    commands_until(&mut rx, |c| matches!(c, Command::Unregister { .. })).await;
}

#[tokio::test]
async fn an_outdated_webtransport_client_is_turned_away() {
    let (offer, mut rx) = wt_server().await;
    assert!(within(dial(&offer, shared::PROTOCOL_VERSION + 1))
        .await
        .expect("in time")
        .is_err());
    assert!(rx.try_recv().is_err(), "it reached the manager");
}

async fn offered(offer: Option<Offer>) -> serde_json::Value {
    use tower::ServiceExt;
    let req = axum::http::Request::get("/api/transport")
        .body(axum::body::Body::empty())
        .unwrap();
    let res = crate::wt::offer_route(offer).oneshot(req).await.unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn the_api_tells_how_to_reach_webtransport() {
    assert!(offered(None).await.is_null(), "no WebTransport here");
    let prod = offered(Some(Offer {
        url: "https://h/wt".into(),
        cert_hash: None,
    }))
    .await;
    assert_eq!(prod, serde_json::json!({ "url": "https://h/wt" }));
    let local = offered(Some(Offer {
        url: "https://127.0.0.1:1/wt".into(),
        cert_hash: Some(vec![1, 2]),
    }))
    .await;
    assert_eq!(local["cert_hash"], serde_json::json!([1, 2]));
}
