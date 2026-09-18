//! Owns the WebSocket lifecycle: dialling, the live link, and reconnection.
//!
//! The rest of the client holds a single `Connection` rather than a socket plus
//! a set of side flags, so states like "reconnecting while a healthy socket is
//! open" cannot be represented and no caller has to remember to clear anything.
//!
//! `Connection` reports what happened through [`ConnEvent`] and never touches
//! screens, sessions or notices — deciding what those mean is the UI's job.

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

/// A first connection fails fast instead — there is no session to recover, and
/// the player is better told the server is unreachable than left waiting.
const INITIAL_WINDOW: f64 = 10.0;

/// Per-client jitter source, so two clients dropped by one server restart do not
/// dial back in lockstep.
///
/// Seeded from the player id rather than the clock: `performance.now()` is
/// deliberately coarsened on the web (often to the millisecond), so a
/// clock-derived fraction would be identical on every wasm client — no jitter at
/// all, and a delay biased permanently low.
fn seed_from(player_id: &str) -> u64 {
    // FNV-1a. `| 1` because xorshift degenerates on a zero state.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in player_id.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h | 1
}

/// xorshift64, returning a fraction in `[0, 1)`. Small and dependency-free;
/// retry spacing does not need statistical quality.
fn next_frac(state: &mut u64) -> f64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// Schedule for a sequence of dials: when the next is due, and when to stop.
/// Times are absolute readings of [`now_secs`], never accumulated deltas.
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

    /// Records that a dial just fired and arms the delay for the one after it.
    /// `frac` is a jitter fraction in `[0, 1)`, supplied by the connection.
    fn record_dial(&mut self, now: f64, frac: f64) {
        self.attempts += 1;
        self.next_dial_at = now + self.backoff * (1.0 - JITTER + 2.0 * JITTER * frac);
        self.backoff = (self.backoff * 2.0).min(BACKOFF_MAX);
    }
}

/// An open socket. Private: nothing outside this module should hold one.
struct Net {
    ws_sender: WsSender,
    ws_receiver: WsReceiver,
}

/// The four states a connection can be in. Exactly one is true at a time.
enum Link {
    Offline,
    Dialing { net: Net, retry: Retry },
    Live { net: Net },
    Waiting { retry: Retry },
}

/// What happened since the last poll.
pub enum ConnEvent {
    Opened { recovered: bool },
    Message(Box<ServerMessage>),
    Retrying,
    GaveUp,
}

pub struct Connection {
    link: Link,
    unreported_drop: Option<String>,
    jitter: u64,
}

impl Connection {
    pub fn new(player_id: &str) -> Self {
        Self {
            link: Link::Offline,
            unreported_drop: None,
            jitter: seed_from(player_id),
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.link, Link::Live { .. })
    }

    pub fn connect(&mut self, now: f64) {
        self.link = Link::Waiting {
            retry: Retry::initial(now),
        };
    }

    pub fn disconnect(&mut self) {
        self.link = Link::Offline;
        self.unreported_drop = None;
    }

    pub fn recovering(&self, now: f64) -> Option<(u32, f64)> {
        let retry = match &self.link {
            Link::Dialing { retry, .. } | Link::Waiting { retry } => retry,
            _ => return None,
        };
        retry
            .recovering
            .then(|| (retry.attempts, (retry.deadline - now).max(0.0)))
    }

    pub fn take_unreported_drop(&mut self) -> Option<String> {
        self.unreported_drop.take()
    }

    pub fn send(&mut self, msg: &ClientMessage) {
        let Link::Live { net } = &mut self.link else {
            return;
        };
        match shared::encode(msg) {
            Ok(bytes) => net.ws_sender.send(WsMessage::Binary(bytes)),
            Err(e) => eprintln!("[ws] encode failed: {e}"),
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
                retry.record_dial(now, next_frac(&mut self.jitter));
                match ewebsock::connect(crate::server_url(), ewebsock::Options::default()) {
                    Ok((ws_sender, ws_receiver)) => Link::Dialing {
                        net: Net { ws_sender, ws_receiver },
                        retry,
                    },
                    Err(e) => {
                        eprintln!("[ws] tentative {} impossible : {e}", retry.attempts);
                        Link::Waiting { retry }
                    }
                }
            }

            Link::Dialing { mut net, retry } => {
                let outcome = drain(&mut net, events);
                match dial_decision(&retry, outcome, now) {
                    DialOutcome::Drop(reason) => self.drop_link(reason, retry, now, events),
                    DialOutcome::GoLive { recovered } => {
                        events.push(ConnEvent::Opened { recovered });
                        Link::Live { net }
                    }
                    DialOutcome::GiveUp => {
                        events.push(ConnEvent::GaveUp);
                        Link::Offline
                    }
                    DialOutcome::KeepWaiting => Link::Dialing { net, retry },
                }
            }

            Link::Live { mut net } => match drain(&mut net, events) {
                Drained::Dropped(reason) => self.drop_link(reason, Retry::recovering(now), now, events),
                _ => Link::Live { net },
            },
        }
    }

    fn drop_link(&mut self, reason: String, retry: Retry, now: f64, events: &mut Vec<ConnEvent>) -> Link {
        events.clear();
        eprintln!("[ws] connexion perdue : {reason}");
        self.unreported_drop = Some(reason);
        if retry.expired(now) {
            events.push(ConnEvent::GaveUp);
            return Link::Offline;
        }
        events.push(ConnEvent::Retrying);
        Link::Waiting { retry }
    }
}

/// What a dialing link should do next, given what its socket reported.
/// Split out from [`Connection::step`] as a pure function so it can be tested
/// without a real socket — the two rules below both shipped broken once because
/// they were only reachable through live I/O.
enum DialOutcome {
    KeepWaiting,
    GoLive { recovered: bool },
    Drop(String),
    GiveUp,
}

fn dial_decision(retry: &Retry, outcome: Drained, now: f64) -> DialOutcome {
    match outcome {
        Drained::Dropped(reason) => DialOutcome::Drop(reason),
        Drained::Opened => DialOutcome::GoLive {
            recovered: retry.recovering,
        },
        Drained::Quiet if retry.expired(now) => DialOutcome::GiveUp,
        Drained::Quiet => DialOutcome::KeepWaiting,
    }
}

/// Outcome of draining a socket's event queue in one poll.
enum Drained {
    Quiet,
    Opened,
    Dropped(String),
}

fn drain(net: &mut Net, events: &mut Vec<ConnEvent>) -> Drained {
    let mut outcome = Drained::Quiet;
    while let Some(event) = net.ws_receiver.try_recv() {
        match event {
            WsEvent::Opened => outcome = Drained::Opened,
            WsEvent::Message(WsMessage::Binary(bytes)) => {
                if let Some(m) = shared::decode::<ServerMessage>(&bytes) {
                    events.push(ConnEvent::Message(Box::new(m)));
                }
            }
            WsEvent::Closed => outcome = Drained::Dropped("fermée par le serveur".to_string()),
            WsEvent::Error(e) => outcome = Drained::Dropped(e),
            _ => {}
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first dial is immediate, then the backoff doubles and saturates.
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
        // frac 0.5 is the midpoint of the jitter range, i.e. no displacement.
        let nominal = [0.2, 0.4, 0.8, 1.6, 3.0, 3.0, 3.0];
        for (got, want) in armed.iter().zip(nominal) {
            assert!((got - want).abs() < 1e-9, "delay {got}, expected {want}");
        }
    }

    /// Jitter must keep every delay inside ±25% of nominal, whatever the seed.
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

    /// Two clients must not dial back in lockstep after one server restart.
    ///
    /// The seed comes from the player id, not the clock: `performance.now()` is
    /// coarsened on the web, so a clock-derived jitter would be identical on
    /// every wasm client.
    #[test]
    fn jitter_decorrelates_two_clients() {
        let mut a = seed_from("player-aaaa");
        let mut b = seed_from("player-bbbb");
        assert_ne!(a, b, "distinct ids must seed differently");

        // Same drop instant, same backoff: only the seed can separate them.
        for i in 0..10 {
            let (fa, fb) = (next_frac(&mut a), next_frac(&mut b));
            assert!((fa - fb).abs() > 1e-9, "identical jitter at step {i}");
        }
    }

    /// Regression: a dial that never opens and never errors — a blackholed SYN —
    /// must still hit the deadline. The deadline used to be checked only between
    /// dials, so such a dial hung forever and the player saw nothing at all.
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

    /// Regression: whether an open followed a drop has to be decided while the
    /// retry still exists. Reading it after the transition to `Live` always said
    /// "not a recovery", so the reconnection notice was never cleared.
    #[test]
    fn open_reports_whether_it_was_a_recovery() {
        let recovered = dial_decision(&Retry::recovering(0.0), Drained::Opened, 1.0);
        assert!(matches!(recovered, DialOutcome::GoLive { recovered: true }));

        let fresh = dial_decision(&Retry::initial(0.0), Drained::Opened, 1.0);
        assert!(matches!(fresh, DialOutcome::GoLive { recovered: false }));
    }

    /// A socket that opens and dies within one poll must be treated as dead,
    /// otherwise the retry restarts and spins.
    #[test]
    fn drop_takes_precedence_over_open() {
        let d = dial_decision(&Retry::recovering(0.0), Drained::Dropped("x".into()), 1.0);
        assert!(matches!(d, DialOutcome::Drop(_)));
    }

    /// A recovery that gives up is the most interesting failure there is, so its
    /// reason must survive the player reconnecting by hand afterwards.
    #[test]
    fn give_up_then_manual_reconnect_still_reports_the_reason() {
        let now = 40.0;
        let mut c = Connection::new("p");
        let stale = Retry::recovering(now - RECOVER_WINDOW - 1.0);
        let link = c.drop_link("read: boom".to_string(), stale, now, &mut Vec::new());
        assert!(matches!(link, Link::Offline), "this drop must give up");

        // The player goes back to the menu and clicks Play again.
        c.connect(now + 30.0);

        assert_eq!(c.take_unreported_drop().as_deref(), Some("read: boom"));
    }

    /// A zero seed would make xorshift produce zeros forever.
    #[test]
    fn seed_is_never_zero() {
        assert_ne!(seed_from(""), 0);
        assert_ne!(seed_from("\0\0\0\0\0\0\0\0"), 0);
    }

    /// A retry is refused until its delay has elapsed, and the window ends.
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

    /// A fresh connect gives up quickly; a recovery gets the long window.
    #[test]
    fn initial_connect_fails_faster_than_recovery() {
        let now = 0.0;
        assert!(Retry::initial(now).deadline < Retry::recovering(now).deadline);
        assert!(!Retry::initial(now).recovering);
        assert!(Retry::recovering(now).recovering);
    }

    /// Only a recovery shows the banner; a first connection stays quiet.
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

    /// Deliberately leaving must not leave a reconnection armed behind it.
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

    /// The drop reason survives until it is reported, and is spent only once.
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

    /// Dropping past the deadline gives up instead of scheduling another dial.
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

    /// Messages that arrive in the same poll as the drop must not be acted on.
    #[test]
    fn drop_discards_messages_read_alongside_it() {
        let now = 2.0;
        let mut c = Connection::new("test-player");
        let mut events = vec![ConnEvent::Message(Box::new(ServerMessage::GameStart))];

        c.drop_link("bye".to_string(), Retry::recovering(now), now, &mut events);

        assert!(matches!(events.as_slice(), [ConnEvent::Retrying]));
    }
}
