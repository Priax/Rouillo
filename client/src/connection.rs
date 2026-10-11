use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use shared::{config, ClientMessage, ServerMessage};

pub fn now_secs() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now() / 1000.0)
            .unwrap_or(0.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64()
    }
}

const BACKOFF_START: f64 = 0.2;
const BACKOFF_MAX: f64 = 3.0;
const JITTER: f64 = 0.25;

const RECOVER_WINDOW: f64 = (config::RECONNECT_GRACE_SECS - config::RECONNECT_MARGIN_SECS) as f64;

const _: () =
    assert!(config::RECONNECT_MARGIN_SECS > 0 && config::RECONNECT_MARGIN_SECS < config::RECONNECT_GRACE_SECS);

const INITIAL_WINDOW: f64 = 10.0;

fn seed_from(player_id: &str) -> u64 {
    // `| 1` because xorshift degenerates on a zero state.
    shared::fnv1a(player_id.as_bytes()) | 1
}

// xorshift64, returning a fraction in `[0, 1)`. Small and dependency-free;
// retry spacing does not need statistical quality.
fn next_frac(state: &mut u64) -> f64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

#[derive(Clone)]
struct Retry {
    attempts: u32,
    backoff: f64,
    next_dial_at: f64,
    deadline: f64,
    recovering: bool,
}

impl Retry {
    fn initial(now: f64) -> Self {
        Self {
            attempts: 0,
            backoff: BACKOFF_START,
            next_dial_at: now,
            deadline: now + INITIAL_WINDOW,
            recovering: false,
        }
    }

    fn recovering(now: f64) -> Self {
        Self {
            attempts: 0,
            backoff: BACKOFF_START,
            next_dial_at: now,
            deadline: now + RECOVER_WINDOW,
            recovering: true,
        }
    }

    fn expired(&self, now: f64) -> bool {
        now >= self.deadline
    }

    fn due(&self, now: f64) -> bool {
        now >= self.next_dial_at
    }

    fn record_dial(&mut self, now: f64, frac: f64) {
        self.attempts += 1;
        self.next_dial_at = now + self.backoff * (1.0 - JITTER + 2.0 * JITTER * frac);
        self.backoff = (self.backoff * 2.0).min(BACKOFF_MAX);
    }
}

/// A link dialed over WebTransport that has not opened by then is given up
/// for the WebSocket.
const WT_OPEN_TIMEOUT: f64 = 3.0;

/// How long a dial waits for the server to say whether it takes WebTransport.
const OFFER_WAIT: f64 = 2.0;

enum Net {
    Ws { sender: WsSender, receiver: WsReceiver },
    Wt(crate::wt::Link),
}

enum NetEvent {
    Opened,
    Bytes(Vec<u8>),
    Outdated,
    Closed(String),
    Other,
}

impl Net {
    fn send(&mut self, msg: &ClientMessage) {
        let bytes = match shared::encode(msg) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("[net] encode failed: {e}");
                return;
            }
        };
        match self {
            Self::Ws { sender, .. } => sender.send(WsMessage::Binary(bytes)),
            Self::Wt(link) => link.send(bytes),
        }
    }

    fn next(&self) -> Option<NetEvent> {
        Some(match self {
            Self::Ws { receiver, .. } => match receiver.try_recv()? {
                WsEvent::Message(WsMessage::Text(t)) if t == shared::OUTDATED_FRAME => NetEvent::Outdated,
                WsEvent::Opened => NetEvent::Opened,
                WsEvent::Message(WsMessage::Binary(bytes)) => NetEvent::Bytes(bytes),
                WsEvent::Closed => NetEvent::Closed("fermée par le serveur".to_owned()),
                WsEvent::Error(e) => NetEvent::Closed(e),
                WsEvent::Message(_) => NetEvent::Other,
            },
            Self::Wt(link) => match link.try_recv()? {
                crate::wt::Event::Opened => NetEvent::Opened,
                crate::wt::Event::Message(bytes) => NetEvent::Bytes(bytes),
                crate::wt::Event::Closed(reason) => NetEvent::Closed(reason),
            },
        })
    }

    const fn is_wt(&self) -> bool {
        matches!(self, Self::Wt(_))
    }
}

/// Whether the server takes WebTransport, which it says once asked.
enum Transport {
    Unknown,
    Asking { slot: crate::http::HttpSlot, since: f64 },
    Known(Option<crate::wt::Offer>),
}

struct Heartbeat {
    next_ping_at: f64,
    inflight: Option<(u32, f64)>,
    seq: u32,
    rtt_ms: Option<f32>,
}

impl Heartbeat {
    fn new(now: f64) -> Self {
        Self {
            next_ping_at: now,
            inflight: None,
            seq: 0,
            rtt_ms: None,
        }
    }

    fn due(&mut self, now: f64) -> Option<ClientMessage> {
        if now < self.next_ping_at {
            return None;
        }
        self.next_ping_at = now + config::PING_INTERVAL_SECS;
        self.seq = self.seq.wrapping_add(1);
        self.inflight.get_or_insert((self.seq, now));
        Some(ClientMessage::Ping { id: self.seq })
    }

    fn on_pong(&mut self, id: u32, now: f64) {
        if let Some((pending, sent)) = self.inflight {
            if pending == id {
                self.rtt_ms = Some(((now - sent) * 1000.0) as f32);
            }
        }
        self.inflight = None;
    }

    fn timed_out(&self, now: f64) -> bool {
        matches!(self.inflight, Some((_, sent)) if now - sent >= config::PING_TIMEOUT_SECS)
    }
}

enum Link {
    Offline,
    Dialing { net: Net, retry: Retry, since: f64 },
    Live { net: Net },
    Waiting { retry: Retry },
}

pub enum ConnEvent {
    Opened { recovered: bool },
    Message(Box<ServerMessage>),
    Retrying,
    GaveUp,
    Outdated,
}

pub struct Connection {
    link: Link,
    unreported_drop: Option<String>,
    jitter: u64,
    heartbeat: Heartbeat,
    transport: Transport,
    /// WebTransport did not open once: the WebSocket is used from then on.
    wt_failed: bool,
}

impl Connection {
    pub fn new(player_id: &str) -> Self {
        Self {
            link: Link::Offline,
            unreported_drop: None,
            jitter: seed_from(player_id),
            heartbeat: Heartbeat::new(0.0),
            transport: Transport::Unknown,
            wt_failed: false,
        }
    }

    /// What carries the live link, for the debug overlay.
    pub fn via(&self) -> &'static str {
        match &self.link {
            Link::Live { net } if net.is_wt() => "WebTransport",
            Link::Live { .. } => "WebSocket",
            _ => "-",
        }
    }

    pub fn rtt_ms(&self) -> Option<f32> {
        self.heartbeat.rtt_ms
    }

    pub fn is_live(&self) -> bool {
        matches!(self.link, Link::Live { .. })
    }

    pub fn connect(&mut self, now: f64) {
        if matches!(self.transport, Transport::Unknown) {
            let slot = crate::http::new_slot();
            crate::http::get(crate::http::api_url("transport"), None, std::sync::Arc::clone(&slot));
            self.transport = Transport::Asking { slot, since: now };
        }
        self.link = Link::Waiting {
            retry: Retry::initial(now),
        };
    }

    /// The WebTransport offer to dial now, or `Err` while the server has
    /// not answered whether it makes one. An answer that comes too late
    /// serves the next dial; a failed question is asked again on the next
    /// connection.
    fn offer(&mut self, now: f64) -> Result<Option<crate::wt::Offer>, ()> {
        if let Transport::Asking { slot, since } = &self.transport {
            match crate::http::poll(slot) {
                Some(Ok(resp)) if resp.ok => {
                    self.transport = Transport::Known(crate::http::json::<Option<crate::wt::Offer>>(&resp).flatten());
                }
                Some(_) => {
                    self.transport = Transport::Unknown;
                    return Ok(None);
                }
                None if now - since < OFFER_WAIT => return Err(()),
                None => return Ok(None),
            }
        }
        Ok(match &self.transport {
            Transport::Known(Some(offer)) if !self.wt_failed => Some(offer.clone()),
            _ => None,
        })
    }

    pub fn disconnect(&mut self) {
        self.link = Link::Offline;
        self.unreported_drop = None;
        self.heartbeat = Heartbeat::new(0.0);
    }

    pub fn recovering(&self, now: f64) -> Option<(u32, f64)> {
        let (Link::Dialing { retry, .. } | Link::Waiting { retry }) = &self.link else {
            return None;
        };
        retry
            .recovering
            .then(|| (retry.attempts, (retry.deadline - now).max(0.0)))
    }

    pub fn take_unreported_drop(&mut self) -> Option<String> {
        self.unreported_drop.take()
    }

    pub fn send(&mut self, msg: &ClientMessage) {
        if let Link::Live { net } = &mut self.link {
            net.send(msg);
        }
    }

    /// Sends `msg` again in a datagram, on a link that has them.
    pub fn send_datagram(&mut self, msg: &ClientMessage) {
        if let Link::Live { net: Net::Wt(link) } = &mut self.link {
            if let Ok(bytes) = shared::encode(msg) {
                link.send_datagram(bytes);
            }
        }
    }

    pub fn poll(&mut self, now: f64) -> Vec<ConnEvent> {
        let mut events = Vec::new();
        let link = std::mem::replace(&mut self.link, Link::Offline);
        self.link = self.step(link, now, &mut events);
        events
    }

    fn step(&mut self, link: Link, now: f64, events: &mut Vec<ConnEvent>) -> Link {
        match link {
            Link::Offline => Link::Offline,

            Link::Waiting { mut retry } => {
                if retry.expired(now) {
                    events.push(ConnEvent::GaveUp);
                    return Link::Offline;
                }
                if !retry.due(now) {
                    return Link::Waiting { retry };
                }
                let Ok(offer) = self.offer(now) else {
                    return Link::Waiting { retry };
                };
                retry.record_dial(now, next_frac(&mut self.jitter));
                if let Some(offer) = offer {
                    return Link::Dialing {
                        net: Net::Wt(crate::wt::Link::dial(&offer)),
                        retry,
                        since: now,
                    };
                }
                let url = format!("{}?v={}", crate::server_url(), shared::PROTOCOL_VERSION);
                match ewebsock::connect(url, ewebsock::Options::default()) {
                    Ok((sender, receiver)) => Link::Dialing {
                        net: Net::Ws { sender, receiver },
                        retry,
                        since: now,
                    },
                    Err(e) => {
                        eprintln!("[ws] tentative {} impossible: {e}", retry.attempts);
                        Link::Waiting { retry }
                    }
                }
            }

            Link::Dialing { net, mut retry, since } => {
                let outcome = drain(&net, now, &mut self.heartbeat, events);
                let stuck = matches!(outcome, Drained::Quiet) && now - since >= WT_OPEN_TIMEOUT;
                if net.is_wt() && (stuck || matches!(outcome, Drained::Dropped(_))) {
                    eprintln!("[net] WebTransport n'a pas abouti, retour au WebSocket");
                    self.wt_failed = true;
                    retry.next_dial_at = now;
                    return Link::Waiting { retry };
                }
                match dial_decision(&retry, outcome, now) {
                    DialOutcome::Outdated => self.outdated(events),
                    DialOutcome::Drop(reason) => self.drop_link(reason, retry, now, events),
                    DialOutcome::GoLive { recovered } => {
                        events.push(ConnEvent::Opened { recovered });
                        self.heartbeat = Heartbeat::new(now);
                        Link::Live { net }
                    }
                    DialOutcome::GiveUp => {
                        events.push(ConnEvent::GaveUp);
                        Link::Offline
                    }
                    DialOutcome::KeepWaiting => Link::Dialing { net, retry, since },
                }
            }

            Link::Live { mut net } => {
                match drain(&net, now, &mut self.heartbeat, events) {
                    Drained::Outdated => return self.outdated(events),
                    Drained::Dropped(reason) => return self.drop_link(reason, Retry::recovering(now), now, events),
                    Drained::Quiet | Drained::Opened => {}
                }
                if self.heartbeat.timed_out(now) {
                    let reason = format!("silence du serveur pendant {:.0}s", config::PING_TIMEOUT_SECS);
                    return self.drop_link(reason, Retry::recovering(now), now, events);
                }
                if let Some(ping) = self.heartbeat.due(now) {
                    net.send(&ping);
                }
                Link::Live { net }
            }
        }
    }

    fn outdated(&mut self, events: &mut Vec<ConnEvent>) -> Link {
        events.clear();
        eprintln!("[net] refusé: le serveur attend une autre version du protocole");
        self.unreported_drop = None;
        self.heartbeat = Heartbeat::new(0.0);
        events.push(ConnEvent::Outdated);
        Link::Offline
    }

    fn drop_link(&mut self, reason: String, retry: Retry, now: f64, events: &mut Vec<ConnEvent>) -> Link {
        events.clear();
        eprintln!("[net] connexion perdue: {reason}");
        self.unreported_drop = Some(reason);
        if retry.expired(now) {
            events.push(ConnEvent::GaveUp);
            return Link::Offline;
        }
        events.push(ConnEvent::Retrying);
        Link::Waiting { retry }
    }
}

enum DialOutcome {
    KeepWaiting,
    Outdated,
    GoLive { recovered: bool },
    Drop(String),
    GiveUp,
}

fn dial_decision(retry: &Retry, outcome: Drained, now: f64) -> DialOutcome {
    match outcome {
        Drained::Outdated => DialOutcome::Outdated,
        Drained::Dropped(reason) => DialOutcome::Drop(reason),
        Drained::Opened => DialOutcome::GoLive {
            recovered: retry.recovering,
        },
        Drained::Quiet if retry.expired(now) => DialOutcome::GiveUp,
        Drained::Quiet => DialOutcome::KeepWaiting,
    }
}

enum Drained {
    Quiet,
    Opened,
    Dropped(String),
    Outdated,
}

fn drain(net: &Net, now: f64, heartbeat: &mut Heartbeat, events: &mut Vec<ConnEvent>) -> Drained {
    let mut outcome = Drained::Quiet;
    while let Some(event) = net.next() {
        match event {
            NetEvent::Outdated => return Drained::Outdated,
            NetEvent::Opened => outcome = Drained::Opened,
            NetEvent::Bytes(bytes) => match shared::decode::<ServerMessage>(&bytes) {
                Some(ServerMessage::Pong { id }) => heartbeat.on_pong(id, now),
                Some(m) => events.push(ConnEvent::Message(Box::new(m))),
                None => {}
            },
            NetEvent::Closed(reason) => outcome = Drained::Dropped(reason),
            NetEvent::Other => {}
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_pings_once_per_interval() {
        let t0 = 100.0;
        let mut hb = Heartbeat::new(t0);
        assert!(hb.due(t0).is_some(), "a new link must be probed immediately");
        assert!(hb.due(t0).is_none(), "twice in the same poll");
        assert!(hb.due(t0 + config::PING_INTERVAL_SECS - 0.01).is_none());
        assert!(hb.due(t0 + config::PING_INTERVAL_SECS).is_some());
    }

    #[test]
    fn heartbeat_measures_the_round_trip() {
        let t0 = 100.0;
        let mut hb = Heartbeat::new(t0);
        let Some(ClientMessage::Ping { id }) = hb.due(t0) else {
            panic!("no ping was due");
        };
        hb.on_pong(id, t0 + 0.123);
        assert_eq!(hb.rtt_ms.map(f32::round), Some(123.0));
    }

    #[test]
    fn silence_is_timed_from_the_first_unanswered_ping() {
        let t0 = 100.0;
        let mut hb = Heartbeat::new(t0);
        let mut t = t0;
        while t < t0 + config::PING_TIMEOUT_SECS {
            hb.due(t);
            assert!(!hb.timed_out(t), "gave up after only {:.0}s of silence", t - t0);
            t += config::PING_INTERVAL_SECS;
        }
        assert!(hb.timed_out(t0 + config::PING_TIMEOUT_SECS));
    }

    #[test]
    fn a_late_pong_clears_the_silence_without_a_sample() {
        let t0 = 100.0;
        let mut hb = Heartbeat::new(t0);
        hb.due(t0); // id 1, lost on the way
        let Some(ClientMessage::Ping { id }) = hb.due(t0 + config::PING_INTERVAL_SECS) else {
            panic!("no second ping was due");
        };
        assert_eq!(id, 2);
        hb.on_pong(id, t0 + config::PING_INTERVAL_SECS + 0.05);
        assert!(hb.rtt_ms.is_none(), "id 2 was never the ping being timed");
        assert!(!hb.timed_out(t0 + config::PING_TIMEOUT_SECS + 1.0));
    }

    #[test]
    fn an_answered_link_never_times_out() {
        let t0 = 100.0;
        let mut hb = Heartbeat::new(t0);
        let mut t = t0;
        for _ in 0..500 {
            if let Some(ClientMessage::Ping { id }) = hb.due(t) {
                hb.on_pong(id, t + 0.2);
            }
            t += config::PING_INTERVAL_SECS;
            assert!(!hb.timed_out(t));
        }
    }

    #[test]
    fn backoff_doubles_then_saturates() {
        let now = 1_000.0;
        let mut retry = Retry::recovering(now);
        assert!(retry.due(now), "first dial must be immediate");

        let mut armed = Vec::new();
        let mut t = now;
        for _ in 0..7 {
            retry.record_dial(t, 0.5);
            armed.push(retry.next_dial_at - t);
            t = retry.next_dial_at;
        }
        let nominal = [0.2, 0.4, 0.8, 1.6, 3.0, 3.0, 3.0];
        for (got, want) in armed.iter().zip(nominal) {
            assert!((got - want).abs() < 1e-9, "delay {got}, expected {want}");
        }
    }

    #[test]
    fn jitter_stays_within_bounds() {
        let mut state = seed_from("some-player");
        for _ in 0..1_000 {
            let frac = next_frac(&mut state);
            assert!((0.0..1.0).contains(&frac), "frac {frac} out of range");
            let mut retry = Retry::recovering(0.0);
            retry.backoff = 3.0;
            retry.record_dial(0.0, frac);
            assert!(
                retry.next_dial_at >= 3.0 * 0.75 && retry.next_dial_at <= 3.0 * 1.25,
                "delay {} outside ±25% of 3.0",
                retry.next_dial_at
            );
        }
    }

    #[test]
    fn jitter_decorrelates_two_clients() {
        let mut a = seed_from("player-aaaa");
        let mut b = seed_from("player-bbbb");
        assert_ne!(a, b, "distinct ids must seed differently");

        for i in 0..10 {
            let (fa, fb) = (next_frac(&mut a), next_frac(&mut b));
            assert!((fa - fb).abs() > 1e-9, "identical jitter at step {i}");
        }
    }

    #[test]
    fn hanging_dial_gives_up_at_deadline() {
        let retry = Retry::initial(0.0);
        assert!(matches!(
            dial_decision(&retry, Drained::Quiet, INITIAL_WINDOW - 1.0),
            DialOutcome::KeepWaiting
        ));
        assert!(matches!(
            dial_decision(&retry, Drained::Quiet, INITIAL_WINDOW),
            DialOutcome::GiveUp
        ));
    }

    #[test]
    fn open_reports_whether_it_was_a_recovery() {
        let recovered = dial_decision(&Retry::recovering(0.0), Drained::Opened, 1.0);
        assert!(matches!(recovered, DialOutcome::GoLive { recovered: true }));

        let fresh = dial_decision(&Retry::initial(0.0), Drained::Opened, 1.0);
        assert!(matches!(fresh, DialOutcome::GoLive { recovered: false }));
    }

    #[test]
    fn drop_takes_precedence_over_open() {
        let d = dial_decision(&Retry::recovering(0.0), Drained::Dropped("x".into()), 1.0);
        assert!(matches!(d, DialOutcome::Drop(_)));
    }

    #[test]
    fn give_up_then_manual_reconnect_still_reports_the_reason() {
        let now = 40.0;
        let mut c = Connection::new("p");
        let stale = Retry::recovering(now - RECOVER_WINDOW - 1.0);
        let link = c.drop_link("read: boom".to_string(), stale, now, &mut Vec::new());
        assert!(matches!(link, Link::Offline), "this drop must give up");

        c.connect(now + 30.0);

        assert_eq!(c.take_unreported_drop().as_deref(), Some("read: boom"));
    }

    #[test]
    fn seed_is_never_zero() {
        assert_ne!(seed_from(""), 0);
        assert_ne!(seed_from("\0\0\0\0\0\0\0\0"), 0);
    }

    #[test]
    fn schedule_respects_delay_and_deadline() {
        let now = 500.0;
        let mut retry = Retry::recovering(now);
        retry.record_dial(now, 0.5);
        assert!(!retry.due(now + 0.05), "0.05s is inside the 0.2s backoff");
        assert!(retry.due(now + 0.3), "0.3s is past the 0.2s backoff");

        assert!(!retry.expired(now + RECOVER_WINDOW - 1.0));
        assert!(retry.expired(now + RECOVER_WINDOW));
    }

    #[test]
    fn initial_connect_fails_faster_than_recovery() {
        let now = 0.0;
        assert!(Retry::initial(now).deadline < Retry::recovering(now).deadline);
        assert!(!Retry::initial(now).recovering);
        assert!(Retry::recovering(now).recovering);
    }

    #[test]
    fn banner_only_while_recovering() {
        let now = 10.0;
        let mut c = Connection::new("test-player");
        assert_eq!(c.recovering(now), None, "offline shows nothing");

        c.connect(now);
        assert_eq!(c.recovering(now), None, "first connect shows nothing");

        c.link = Link::Waiting {
            retry: Retry::recovering(now),
        };
        let (attempts, left) = c.recovering(now).expect("recovery must show a banner");
        assert_eq!(attempts, 0);
        assert!((left - RECOVER_WINDOW).abs() < 1e-9);
    }

    #[test]
    fn disconnect_clears_everything() {
        let now = 3.0;
        let mut c = Connection::new("test-player");
        c.link = Link::Waiting {
            retry: Retry::recovering(now),
        };
        c.unreported_drop = Some("boom".to_string());

        c.disconnect();

        assert!(matches!(c.link, Link::Offline));
        assert_eq!(c.recovering(now), None);
        assert_eq!(c.take_unreported_drop(), None);
    }

    #[test]
    fn drop_reason_is_reported_once() {
        let now = 7.0;
        let mut c = Connection::new("test-player");
        let mut events = Vec::new();

        let link = c.drop_link("read: boom".to_string(), Retry::recovering(now), now, &mut events);

        assert!(matches!(link, Link::Waiting { .. }));
        assert!(matches!(events.as_slice(), [ConnEvent::Retrying]));
        assert_eq!(c.take_unreported_drop().as_deref(), Some("read: boom"));
        assert_eq!(c.take_unreported_drop(), None, "reported twice");
    }

    #[test]
    fn drop_past_deadline_gives_up() {
        let now = 7.0;
        let mut c = Connection::new("test-player");
        let mut events = Vec::new();
        let stale = Retry::recovering(now - RECOVER_WINDOW - 1.0);

        let link = c.drop_link("gone".to_string(), stale, now, &mut events);

        assert!(matches!(link, Link::Offline));
        assert!(matches!(events.as_slice(), [ConnEvent::GaveUp]));
    }

    #[test]
    fn outdated_goes_offline_without_retrying() {
        let now = 4.0;
        let mut c = Connection::new("test-player");
        let mut events = vec![ConnEvent::Message(Box::new(ServerMessage::Maintenance))];

        let link = c.outdated(&mut events);

        assert!(matches!(link, Link::Offline), "a retry would be refused again");
        assert!(matches!(events.as_slice(), [ConnEvent::Outdated]));
        assert_eq!(c.take_unreported_drop(), None);
        assert!(matches!(
            dial_decision(&Retry::initial(now), Drained::Outdated, now),
            DialOutcome::Outdated
        ));
    }

    #[test]
    fn drop_discards_messages_read_alongside_it() {
        let now = 2.0;
        let mut c = Connection::new("test-player");
        let mut events = vec![ConnEvent::Message(Box::new(ServerMessage::Maintenance))];

        c.drop_link("bye".to_string(), Retry::recovering(now), now, &mut events);

        assert!(matches!(events.as_slice(), [ConnEvent::Retrying]));
    }

    fn asking(c: &mut Connection, since: f64) -> crate::http::HttpSlot {
        let slot = crate::http::new_slot();
        c.transport = Transport::Asking {
            slot: std::sync::Arc::clone(&slot),
            since,
        };
        slot
    }

    fn answered(slot: &crate::http::HttpSlot, status: u16, body: &str) {
        *slot.lock().unwrap() = Some(Ok(ehttp::Response {
            url: String::new(),
            ok: (200..300).contains(&status),
            status,
            status_text: String::new(),
            headers: ehttp::Headers::default(),
            bytes: body.as_bytes().to_vec(),
        }));
    }

    #[test]
    fn a_dial_waits_a_little_for_the_offer_then_goes_without() {
        let mut c = Connection::new("p");
        let slot = asking(&mut c, 0.0);
        assert!(c.offer(OFFER_WAIT - 0.1).is_err(), "too soon to give up on it");
        assert!(matches!(c.offer(OFFER_WAIT), Ok(None)));
        answered(&slot, 200, r#"{"url":"https://h/wt"}"#);
        assert!(
            matches!(c.offer(OFFER_WAIT + 5.0), Ok(Some(o)) if o.url == "https://h/wt"),
            "a late answer serves the next dial"
        );
        c.wt_failed = true;
        assert!(matches!(c.offer(9.0), Ok(None)), "WebTransport failed once");
    }

    #[test]
    fn a_failed_question_is_asked_again_later() {
        let mut c = Connection::new("p");
        let slot = asking(&mut c, 0.0);
        answered(&slot, 502, "Bad Gateway");
        assert!(matches!(c.offer(0.5), Ok(None)));
        assert!(
            matches!(c.transport, Transport::Unknown),
            "a server being restarted is no answer"
        );
        let slot = asking(&mut c, 1.0);
        answered(&slot, 200, "null");
        assert!(matches!(c.offer(1.5), Ok(None)));
        assert!(matches!(c.transport, Transport::Known(None)));
    }
}
