use super::*;

#[test]
fn limiter_never_drops_the_fastest_legitimate_player() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let per_sec = 250;
    for i in 0..(per_sec * 60) {
        let now = t0 + Duration::from_secs_f64(i as f64 / per_sec as f64);
        assert_eq!(l.check(now), Verdict::Allow, "legit message {i} throttled");
    }
}

#[test]
fn limiter_refills_over_time() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    for _ in 0..CLIENT_MSG_BURST as usize {
        l.check(t0);
    }
    assert_eq!(l.check(t0), Verdict::Drop);
    assert_eq!(l.check(t0 + Duration::from_millis(100)), Verdict::Allow);
}

#[test]
fn limiter_disconnects_a_sustained_flood() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let mut verdicts = (0..(CLIENT_MSG_BURST as u32 + FLOOD_DROPS_PER_SEC)).map(|_| l.check(t0));
    assert!(verdicts.all(|v| v != Verdict::Disconnect), "not before the threshold");
    assert_eq!(l.check(t0), Verdict::Disconnect);
}

pub(super) type WsClient = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) async fn connect() -> (WsClient, mpsc::Receiver<Command>) {
    connect_to(&format!("/ws?v={}", shared::PROTOCOL_VERSION)).await
}

pub(super) async fn connect_to(path: &str) -> (WsClient, mpsc::Receiver<Command>) {
    let (tx, rx) = mpsc::channel(CMD_CHAN_CAP);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let app = ws_route(tx, lazy_pool()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let (client, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{path}"))
        .await
        .expect("handshake");
    (client, rx)
}

pub(super) async fn closed(client: &mut WsClient) {
    while let Some(msg) = client.next().await {
        if matches!(msg, Ok(WsMessage::Close(_)) | Err(_)) {
            return;
        }
    }
}

#[tokio::test]
async fn an_outdated_client_is_told_so_and_dropped() {
    let other = shared::PROTOCOL_VERSION + 1;
    for path in ["/ws".to_string(), format!("/ws?v={other}")] {
        let (mut client, mut rx) = connect_to(&path).await;
        let msg = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("no reply")
            .expect("stream ended")
            .expect("recv");
        assert_eq!(msg.to_text().ok(), Some(shared::OUTDATED_FRAME), "{path}");
        let close = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("the server kept the connection open");
        assert!(
            matches!(close, Some(Ok(WsMessage::Close(Some(ref f)))) if u16::from(f.code) == 4426),
            "{path}: {close:?}"
        );
        assert!(rx.try_recv().is_err(), "{path}: an outdated client reached the manager");
    }
}

#[tokio::test]
async fn an_up_to_date_client_is_registered() {
    let (_client, mut rx) = connect().await;
    commands_until(&mut rx, |c| matches!(c, Command::Register { .. })).await;
}

pub(super) fn frame(msg: &ClientMessage) -> WsMessage {
    WsMessage::binary(shared::encode(msg).expect("encode"))
}

pub(super) async fn commands_until(rx: &mut mpsc::Receiver<Command>, stop: impl Fn(&Command) -> bool) -> Vec<Command> {
    let mut out = Vec::new();
    loop {
        let cmd = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for a command")
            .expect("channel closed");
        let done = stop(&cmd);
        out.push(cmd);
        if done {
            return out;
        }
    }
}

#[tokio::test]
async fn flooding_client_is_throttled_then_disconnected() {
    let (mut client, mut rx) = connect().await;
    let input = frame(&ClientMessage::Input {
        kind: InputKind::MoveLeft,
        seq: 1,
        tick: 0,
    });
    let started = Instant::now();
    for _ in 0..3_000 {
        if client.send(input.clone()).await.is_err() {
            break;
        }
    }
    tokio::time::timeout(Duration::from_secs(5), closed(&mut client))
        .await
        .expect("server did not close the flooding connection");
    let refill = started.elapsed().as_secs_f64() * CLIENT_MSG_RATE;

    let cmds = commands_until(&mut rx, |c| matches!(c, Command::Unregister { .. })).await;
    let inputs = cmds.iter().filter(|c| matches!(c, Command::Input { .. })).count();
    let most = (CLIENT_MSG_BURST + refill).ceil() as usize;
    assert!(
        inputs <= most,
        "{inputs} inputs reached the manager, at most {most} allowed"
    );
    assert!(
        inputs >= CLIENT_MSG_BURST as usize,
        "only {inputs}: the burst was not honoured"
    );
}

#[tokio::test]
async fn only_the_first_hello_per_connection_counts() {
    let (mut client, mut rx) = connect().await;
    let hello = frame(&ClientMessage::Hello {
        player_id: "p".into(),
        auth_token: None,
        last_disconnect_reason: None,
    });
    for _ in 0..5 {
        client.send(hello.clone()).await.expect("send");
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await.expect("send");
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    let hellos = cmds.iter().filter(|c| matches!(c, Command::Hello { .. })).count();
    assert_eq!(hellos, 1);
}

#[tokio::test]
async fn a_normal_session_gets_everything_through() {
    let (mut client, mut rx) = connect().await;
    for seq in 1..=100 {
        client
            .send(frame(&ClientMessage::Input {
                kind: InputKind::RotateCW,
                seq,
                tick: 0,
            }))
            .await
            .expect("send");
    }
    client.send(frame(&ClientMessage::RequestRoomList)).await.expect("send");
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    assert_eq!(cmds.iter().filter(|c| matches!(c, Command::Input { .. })).count(), 100);
}

#[test]
fn limiter_absorbs_a_network_stall() {
    let t0 = Instant::now();
    let mut l = RateLimiter::new(t0);
    let stalled = 250 * 4;
    for i in 0..stalled {
        assert_eq!(l.check(t0), Verdict::Allow, "message {i} of the backlog dropped");
    }
}

#[test]
fn late_hello_from_a_dead_connection_is_ignored() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    hello(&mut mgr, 2, "B");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, None, "seat stays free for B's real reconnection");
    assert!(b.disconnect_at.is_some(), "grace keeps running");
    assert!(mgr.rooms[&1].sim.paused, "game stays paused");
    assert!(!mgr.clients.contains_key(&2), "no state recreated for the dead socket");
}

#[test]
fn late_commands_from_a_dead_connection_are_ignored() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::Unregister { conn: 1 });
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "ghost".into(),
    });
    assert!(mgr.rooms.is_empty());
    assert!(!mgr.clients.contains_key(&1));
}

#[test]
fn real_reconnection_still_rebinds_the_seat() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, Some(3));
    assert!(!mgr.rooms[&1].sim.paused);
}

#[test]
fn another_account_on_the_same_browser_frees_the_seat_instead_of_taking_it() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    mgr.handle(Command::Unregister { conn: 2 });
    let _rx3 = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "B", 9);
    assert_eq!(mgr.room_of(3), None, "the new account got the old seat");
    assert_eq!(mgr.rooms[&1].members.len(), 1);
    let saved = mgr.take_unsaved_matches();
    assert_eq!(saved.len(), 1, "the abandoned game is recorded");
    assert_eq!(saved[0].winner_slot, Some(1));
}

#[test]
fn another_account_in_a_second_tab_leaves_a_live_seat_alone() {
    let mut mgr = new_mgr();
    let (_rx1, _rx2) = running_game(&mut mgr);
    let _rx3 = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "B", 9);
    assert_eq!(mgr.room_of(3), None);
    assert_eq!(mgr.rooms[&1].members[1].conn, Some(2), "the playing tab lost its seat");
    assert!(mgr.rooms[&1].game_running());
    assert!(mgr.take_unsaved_matches().is_empty());
}

#[test]
fn the_same_account_still_gets_its_seat_back() {
    let mut mgr = new_mgr();
    let _rx1 = reg(&mut mgr, 1);
    hello_as(&mut mgr, 1, "A", 1);
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let _rx2 = reg(&mut mgr, 2);
    hello_as(&mut mgr, 2, "B", 2);
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    mgr.handle(Command::Unregister { conn: 2 });
    let _rx3 = reg(&mut mgr, 3);
    hello_as(&mut mgr, 3, "B", 2);
    assert_eq!(mgr.room_of(3), Some(1));
    assert_eq!(mgr.rooms[&1].members[1].conn, Some(3));
}

#[tokio::test]
async fn a_ping_is_answered_without_reaching_the_manager() {
    let (mut client, mut rx) = connect().await;
    client.send(frame(&ClientMessage::Ping { id: 7 })).await.expect("send");

    let reply = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("no pong came back")
        .expect("stream ended")
        .expect("socket error");
    assert!(matches!(
        shared::decode::<ServerMessage>(&reply.into_data()),
        Some(ServerMessage::Pong { id: 7 })
    ));

    client.send(frame(&ClientMessage::RequestRoomList)).await.expect("send");
    let cmds = commands_until(&mut rx, |c| matches!(c, Command::RequestRoomList { .. })).await;
    assert!(
        matches!(cmds.first(), Some(Command::Register { .. })),
        "expected the socket's own Register first"
    );
    assert_eq!(cmds.len(), 2, "the ping reached the manager");
}

#[tokio::test]
async fn accepted_sockets_inherit_nodelay() {
    let listener = bind_listener(([127, 0, 0, 1], 0).into());
    let addr = listener.local_addr().expect("local_addr");
    let _client = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (accepted, _) = listener.accept().await.expect("accept");
    assert!(
        accepted.nodelay().expect("nodelay"),
        "TCP_NODELAY did not survive accept()"
    );
}

#[tokio::test(start_paused = true)]
async fn a_silent_socket_is_closed() {
    let (mut client, _rx) = connect().await;

    closed(&mut client).await;
}

#[test]
fn logged_reasons_are_bounded_and_single_line() {
    let s = log_safe(&format!("a\nfake log line\r{}", "x".repeat(10_000)));
    assert!(!s.contains('\n') && !s.contains('\r'));
    assert_eq!(s.chars().count(), 100);
}

#[test]
fn ws_connections_are_capped_per_ip_and_freed_on_drop() {
    let conns = IpConns::default();
    let ip = || Some("203.0.113.7".to_string());
    let slots: Vec<_> = (0..MAX_WS_PER_IP)
        .map(|_| IpSlot::take(&conns, ip()).unwrap())
        .collect();
    assert!(IpSlot::take(&conns, ip()).is_err());
    assert!(IpSlot::take(&conns, Some("198.51.100.1".into())).is_ok());
    drop(slots);
    assert!(IpSlot::take(&conns, ip()).is_ok());
    assert!(conns.lock().unwrap().is_empty());
}

#[test]
fn a_second_tab_takes_the_seat_and_tells_the_first() {
    let mut mgr = new_mgr();
    let (_rx1, mut rx2) = running_game(&mut mgr);
    let _rx3 = reg(&mut mgr, 3);
    hello(&mut mgr, 3, "B");
    assert!(has(&drain(&mut rx2), |m| matches!(m, ServerMessage::OpenedElsewhere)));
    assert_eq!(mgr.room_of(2), None, "the first tab left the room");
    assert_eq!(mgr.room_of(3), Some(1));

    let before = mgr.rooms[&1].sim.boards[1].active_piece.clone().map(|p| p.col);
    super::game::press(&mut mgr, 2, 1, 1);
    mgr.tick(0.1, false);
    let after = mgr.rooms[&1].sim.boards[1].active_piece.clone().map(|p| p.col);
    assert_eq!(before, after, "the first tab no longer plays");

    mgr.handle(Command::Unregister { conn: 2 });
    let b = mgr.rooms[&1].members.iter().find(|m| m.token == "B").unwrap();
    assert_eq!(b.conn, Some(3), "closing the first tab keeps the seat");
    assert!(!mgr.rooms[&1].sim.paused);
}
