use super::*;

pub(super) type RateMap = Arc<Mutex<HashMap<String, (u32, Instant)>>>;

pub(super) fn new_rate_map() -> RateMap {
    Arc::new(Mutex::new(HashMap::new()))
}

pub(super) fn rate_take(map: &RateMap, key: &str, max: u32, window: Duration) -> bool {
    let now = Instant::now();
    let mut m = map.lock().unwrap();
    if m.len() > 500 {
        m.retain(|_, (_, since)| now.duration_since(*since) < window);
    }
    let entry = m.entry(key.to_owned()).or_insert((0, now));
    if now.duration_since(entry.1) >= window {
        *entry = (0, now);
    }
    if entry.0 >= max {
        return false;
    }
    entry.0 += 1;
    true
}

pub(super) fn rate_refund(map: &RateMap, key: &str) {
    if let Some(entry) = map.lock().unwrap().get_mut(key) {
        entry.0 = entry.0.saturating_sub(1);
    }
}

pub(super) fn rate_clear(map: &RateMap, key: &str) {
    map.lock().unwrap().remove(key);
}

pub(super) const MAX_ATTEMPTS: u32 = 10;
pub(super) const WINDOW: Duration = Duration::from_mins(15);
pub(super) const MAX_LOGIN_FAILURES_PER_IP: u32 = 30;
pub(super) const MAX_FAILURES_PER_ACCOUNT: u32 = 100;
pub(super) const ACCOUNT_WINDOW: Duration = Duration::from_hours(1);

#[derive(Clone, Default)]
pub(super) struct LoginLimits {
    pairs: RateMap,
    ips: RateMap,
    accounts: RateMap,
}

impl LoginLimits {
    fn counters<'a>(
        &'a self,
        pair: &'a str,
        ip: Option<&'a str>,
        username: &'a str,
    ) -> Vec<(&'a RateMap, &'a str, u32, Duration)> {
        let mut counters = vec![
            (&self.pairs, pair, MAX_ATTEMPTS, WINDOW),
            (&self.accounts, username, MAX_FAILURES_PER_ACCOUNT, ACCOUNT_WINDOW),
        ];
        if let Some(ip) = ip {
            counters.push((&self.ips, ip, MAX_LOGIN_FAILURES_PER_IP, WINDOW));
        }
        counters
    }

    /// Counts a login attempt, unless one of its counters is full.
    pub(super) fn try_attempt(&self, pair: &str, ip: Option<&str>, account: &str) -> bool {
        reserve(&self.counters(pair, ip, account))
    }

    /// A successful login gives its attempt back and clears the pair's failures.
    pub(super) fn succeeded(&self, pair: &str, ip: Option<&str>, account: &str) {
        for (map, key, ..) in self.counters(pair, ip, account) {
            rate_refund(map, key);
        }
        rate_clear(&self.pairs, pair);
    }
}

pub(super) fn reserve(counters: &[(&RateMap, &str, u32, Duration)]) -> bool {
    for (i, &(map, key, max, window)) in counters.iter().enumerate() {
        if !rate_take(map, key, max, window) {
            for &(map, key, ..) in &counters[..i] {
                rate_refund(map, key);
            }
            return false;
        }
    }
    true
}

pub(super) fn attempt_key(client: Option<&str>, username: &str) -> String {
    format!("{}|{username}", client.unwrap_or(""))
}

pub(super) type IpLimit = RateMap;
pub(super) const MAX_REGISTRATIONS_PER_IP: u32 = 10;
pub(super) const REGISTER_WINDOW: Duration = Duration::from_secs(3600);

pub(super) fn client_key(peer: Option<SocketAddr>, forwarded_for: Option<&str>) -> Option<String> {
    let peer = peer?.ip();
    let ip = if peer.is_loopback() {
        forwarded_for
            .and_then(|h| h.rsplit(',').next())
            .and_then(|last| last.trim().parse::<IpAddr>().ok())
            .unwrap_or(peer)
    } else {
        peer
    };
    Some(match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
    })
}

pub struct ClientAddr(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for ClientAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        let xff = parts.headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
        Ok(Self(client_key(peer, xff)))
    }
}

pub(super) type PasswordChecks = RateMap;

pub(super) type FriendLimit = RateMap;
pub(super) const MAX_FRIEND_REQS: u32 = 30;
pub(super) const FRIEND_WINDOW: Duration = Duration::from_secs(600);

pub(super) type UploadLimit = RateMap;
pub(super) const MAX_UPLOADS: u32 = 20;
pub(super) const UPLOAD_WINDOW: Duration = Duration::from_hours(1);

pub(super) type SearchLimit = RateMap;
pub(super) const MAX_SEARCHES: u32 = 60;
pub(super) const SEARCH_WINDOW: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Option<SocketAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn direct_peer_is_the_client_and_its_forwarded_header_is_ignored() {
        assert_eq!(
            client_key(addr("203.0.113.7:5000"), Some("198.51.100.1")),
            Some("203.0.113.7".into())
        );
    }

    #[test]
    fn behind_the_proxy_only_the_last_forwarded_entry_counts() {
        assert_eq!(
            client_key(addr("127.0.0.1:40000"), Some("1.2.3.4, 203.0.113.7")),
            Some("203.0.113.7".into())
        );
        assert_eq!(
            client_key(addr("[::1]:40000"), Some("203.0.113.7")),
            Some("203.0.113.7".into())
        );
    }

    #[test]
    fn unparseable_or_missing_forwarded_header_falls_back_to_the_peer() {
        assert_eq!(
            client_key(addr("127.0.0.1:1"), Some("garbage")),
            Some("127.0.0.1".into())
        );
        assert_eq!(client_key(addr("127.0.0.1:1"), None), Some("127.0.0.1".into()));
        assert_eq!(client_key(None, Some("203.0.113.7")), None);
    }

    #[test]
    fn ipv6_is_keyed_by_its_64_prefix() {
        let a = client_key(addr("127.0.0.1:1"), Some("2001:db8:1:2:aaaa::1"));
        let b = client_key(addr("127.0.0.1:1"), Some("2001:db8:1:2:bbbb::9"));
        assert_eq!(a, Some("2001:db8:1:2::/64".into()));
        assert_eq!(a, b);
        assert_ne!(a, client_key(addr("127.0.0.1:1"), Some("2001:db8:1:3::1")));
    }

    #[test]
    fn ipv4_mapped_ipv6_is_the_ipv4_address() {
        assert_eq!(
            client_key(addr("[::ffff:203.0.113.7]:1"), None),
            Some("203.0.113.7".into())
        );
    }
}
