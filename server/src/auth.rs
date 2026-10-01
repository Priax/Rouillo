use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;
use warp::http::StatusCode;
use warp::{Filter, Rejection, Reply};

use crate::db::{self, DbPool};
use crate::manager::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiError {
    BadUsername,
    BadPassword,
    BadQuery,
    BioTooLong,
    MusicTooLong,
    SelfFriendRequest,
    Unauthorized,
    BadCredentials,
    WrongPassword,
    NotFound,
    UsernameTaken,
    FriendRequestExists,
    TooManyRequests,
    InvalidBody,
    Internal,
}

impl warp::reject::Reject for ApiError {}

impl ApiError {
    const fn status(self) -> StatusCode {
        match self {
            Self::BadUsername
            | Self::BadPassword
            | Self::BadQuery
            | Self::BioTooLong
            | Self::MusicTooLong
            | Self::SelfFriendRequest
            | Self::InvalidBody => StatusCode::BAD_REQUEST,
            Self::Unauthorized | Self::BadCredentials => StatusCode::UNAUTHORIZED,
            Self::WrongPassword => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::UsernameTaken | Self::FriendRequestExists => StatusCode::CONFLICT,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::BadUsername => "bad_username",
            Self::BadPassword => "bad_password",
            Self::BadQuery => "bad_query",
            Self::BioTooLong => "bio_too_long",
            Self::MusicTooLong => "music_too_long",
            Self::SelfFriendRequest => "self_friend_request",
            Self::Unauthorized => "unauthorized",
            Self::BadCredentials => "bad_credentials",
            Self::WrongPassword => "wrong_password",
            Self::NotFound => "not_found",
            Self::UsernameTaken => "username_taken",
            Self::FriendRequestExists => "friend_request_exists",
            Self::TooManyRequests => "too_many_requests",
            Self::InvalidBody => "invalid_body",
            Self::Internal => "internal",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::BadUsername => "Username must be 3-24 alphanumeric characters or underscores",
            Self::BadPassword => "Password must be 8 to 1024 bytes long",
            Self::BadQuery => "'limit' and 'offset' must be non-negative integers",
            Self::BioTooLong => "Bio must be 500 characters or less",
            Self::MusicTooLong => "Favorite music must be 200 characters or less",
            Self::SelfFriendRequest => "Cannot send a friend request to yourself",
            Self::Unauthorized => "Unauthorized",
            Self::BadCredentials => "Wrong username or password",
            Self::WrongPassword => "Wrong password",
            Self::NotFound => "Not found",
            Self::UsernameTaken => "Username already taken",
            Self::FriendRequestExists => "Friend request already exists",
            Self::TooManyRequests => "Too many requests, please try again later",
            Self::InvalidBody => "Invalid request body",
            Self::Internal => "Internal server error",
        }
    }
}

fn reject(e: ApiError) -> Rejection {
    warp::reject::custom(e)
}

fn internal<E: std::fmt::Display>(e: E) -> Rejection {
    error!("{e}");
    reject(ApiError::Internal)
}

type RateMap = Arc<Mutex<HashMap<String, (u32, Instant)>>>;

fn new_rate_map() -> RateMap {
    Arc::new(Mutex::new(HashMap::new()))
}

fn rate_take(map: &RateMap, key: &str, max: u32, window: Duration) -> bool {
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

fn rate_refund(map: &RateMap, key: &str) {
    if let Some(entry) = map.lock().unwrap().get_mut(key) {
        entry.0 = entry.0.saturating_sub(1);
    }
}

fn rate_clear(map: &RateMap, key: &str) {
    map.lock().unwrap().remove(key);
}

const MAX_ATTEMPTS: u32 = 10;
const WINDOW: Duration = Duration::from_mins(15);
const MAX_LOGIN_FAILURES_PER_IP: u32 = 30;
const MAX_FAILURES_PER_ACCOUNT: u32 = 100;
const ACCOUNT_WINDOW: Duration = Duration::from_hours(1);

#[derive(Clone, Default)]
struct LoginLimits {
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
}

fn reserve(counters: &[(&RateMap, &str, u32, Duration)]) -> bool {
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

fn attempt_key(client: Option<&str>, username: &str) -> String {
    format!("{}|{username}", client.unwrap_or(""))
}

type IpLimit = RateMap;
const MAX_REGISTRATIONS_PER_IP: u32 = 10;
const REGISTER_WINDOW: Duration = Duration::from_secs(3600);

fn client_key(peer: Option<SocketAddr>, forwarded_for: Option<&str>) -> Option<String> {
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

pub fn client_addr() -> impl Filter<Extract = (Option<String>,), Error = std::convert::Infallible> + Clone {
    warp::addr::remote().and(warp::header::headers_cloned()).map(
        |peer: Option<SocketAddr>, headers: warp::http::HeaderMap| {
            let xff = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
            client_key(peer, xff)
        },
    )
}

type PasswordChecks = RateMap;

type FriendLimit = RateMap;
const MAX_FRIEND_REQS: u32 = 30;
const FRIEND_WINDOW: Duration = Duration::from_secs(600);

type SearchLimit = RateMap;
const MAX_SEARCHES: u32 = 60;
const SEARCH_WINDOW: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct RegisterBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct PatchMeBody {
    bio: Option<String>,
    favorite_music: Option<String>,
}

#[derive(Deserialize)]
struct ChangePasswordBody {
    current: String,
    new: String,
}

#[derive(Deserialize)]
struct DeleteAccountBody {
    password: String,
}

#[derive(Serialize)]
struct AuthResponse {
    token: Uuid,
    user_id: Uuid,
    username: String,
    elo: i32,
}

#[derive(Serialize)]
struct UserProfile {
    id: Uuid,
    username: String,
    bio: Option<String>,
    favorite_music: Option<String>,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    elo: i32,
    created_at: DateTime<Utc>,
}

fn with<T: Clone + Send + 'static>(value: T) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || value.clone())
}

fn bearer_token() -> impl Filter<Extract = (Uuid,), Error = Rejection> + Clone {
    warp::header::optional::<String>("authorization").and_then(|h: Option<String>| async move {
        h.as_deref()
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|t| Uuid::parse_str(t).ok())
            .ok_or_else(|| reject(ApiError::Unauthorized))
    })
}

fn authed(pool: DbPool) -> impl Filter<Extract = (db::User,), Error = Rejection> + Clone {
    authed_session(pool).map(|(user, _token): (db::User, Uuid)| user)
}

fn authed_session(pool: DbPool) -> impl Filter<Extract = ((db::User, Uuid),), Error = Rejection> + Clone {
    bearer_token()
        .and(with(pool))
        .and_then(|token: Uuid, pool: DbPool| async move {
            db::find_user_by_token(&pool, token)
                .await
                .map_err(internal)?
                .map(|user| (user, token))
                .ok_or_else(|| reject(ApiError::Unauthorized))
        })
}

fn validate_username(u: &str) -> bool {
    let n = u.chars().count();
    (3..=24).contains(&n) && u.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn validate_password(p: &str) -> bool {
    let n = p.len();
    (8..=1024).contains(&n)
}

async fn handle_register(
    body: RegisterBody,
    pool: DbPool,
    client: Option<String>,
    registrations: IpLimit,
) -> Result<impl Reply, Rejection> {
    if !validate_username(&body.username) {
        return Err(reject(ApiError::BadUsername));
    }
    if !validate_password(&body.password) {
        return Err(reject(ApiError::BadPassword));
    }
    if let Some(ip) = &client {
        if !rate_take(&registrations, ip, MAX_REGISTRATIONS_PER_IP, REGISTER_WINDOW) {
            return Err(reject(ApiError::TooManyRequests));
        }
    }

    let user = match db::create_user(&pool, &body.username, &body.password).await {
        Ok(u) => u,
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => {
            return Err(reject(ApiError::UsernameTaken));
        }
        Err(e) => return Err(internal(e)),
    };

    let token = db::create_session(&pool, user.id).await.map_err(internal)?;

    Ok(warp::reply::with_status(
        warp::reply::json(&AuthResponse {
            token,
            user_id: user.id,
            username: user.username,
            elo: user.elo,
        }),
        warp::http::StatusCode::CREATED,
    ))
}

async fn handle_login(
    body: LoginBody,
    pool: DbPool,
    limits: LoginLimits,
    client: Option<String>,
) -> Result<impl Reply, Rejection> {
    if !validate_username(&body.username) {
        return Err(reject(ApiError::BadCredentials));
    }
    let pair = attempt_key(client.as_deref(), &body.username);
    let counters = limits.counters(&pair, client.as_deref(), &body.username);
    if !reserve(&counters) {
        return Err(reject(ApiError::TooManyRequests));
    }

    let user = db::find_user_by_username(&pool, &body.username)
        .await
        .map_err(internal)?;

    let hash = user
        .as_ref()
        .map_or_else(|| db::dummy_hash().to_owned(), |u| u.password_hash().to_owned());
    let password = body.password.clone();
    let ok = db::run_hash(move || db::verify_password(&password, &hash))
        .await
        .map_err(internal)?;

    let (Some(user), true) = (user, ok) else {
        return Err(reject(ApiError::BadCredentials));
    };

    for &(map, key, ..) in &counters {
        rate_refund(map, key);
    }
    rate_clear(&limits.pairs, &pair);

    let token = db::create_session(&pool, user.id).await.map_err(internal)?;

    Ok(warp::reply::json(&AuthResponse {
        token,
        user_id: user.id,
        username: user.username,
        elo: user.elo,
    }))
}

async fn handle_logout(token: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    db::delete_session(&pool, token).await.map_err(internal)?;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn check_password(user: &db::User, password: String, checks: &PasswordChecks) -> Result<(), Rejection> {
    let key = user.id.to_string();
    if !rate_take(checks, &key, MAX_ATTEMPTS, WINDOW) {
        return Err(reject(ApiError::TooManyRequests));
    }
    let hash = user.password_hash().to_owned();
    let ok = db::run_hash(move || db::verify_password(&password, &hash))
        .await
        .map_err(internal)?;
    if !ok {
        return Err(reject(ApiError::WrongPassword));
    }
    rate_clear(checks, &key);
    Ok(())
}

async fn handle_change_password(
    (user, token): (db::User, Uuid),
    body: ChangePasswordBody,
    pool: DbPool,
    checks: PasswordChecks,
    cmd_tx: mpsc::Sender<Command>,
) -> Result<impl Reply, Rejection> {
    if !validate_password(&body.new) {
        return Err(reject(ApiError::BadPassword));
    }
    check_password(&user, body.current, &checks).await?;
    let new = body.new;
    let hash = db::run_hash(move || db::hash_password(&new))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    db::set_password(&pool, user.id, hash, token).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn handle_logout_all(
    (user, token): (db::User, Uuid),
    pool: DbPool,
    cmd_tx: mpsc::Sender<Command>,
) -> Result<impl Reply, Rejection> {
    db::delete_user_sessions(&pool, user.id).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn handle_delete_account(
    (user, token): (db::User, Uuid),
    body: DeleteAccountBody,
    pool: DbPool,
    checks: PasswordChecks,
    cmd_tx: mpsc::Sender<Command>,
) -> Result<impl Reply, Rejection> {
    check_password(&user, body.password, &checks).await?;
    db::delete_user(&pool, user.id).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn revoke(cmd_tx: &mpsc::Sender<Command>, user_id: Uuid, keep: Uuid) {
    let _ = cmd_tx
        .send(Command::Revoke {
            user_id,
            keep: Some(keep),
        })
        .await;
}

impl From<db::User> for UserProfile {
    fn from(user: db::User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            bio: user.bio,
            favorite_music: user.favorite_music,
            avatar_url: user.avatar_url,
            banner_url: user.banner_url,
            elo: user.elo,
            created_at: user.created_at,
        }
    }
}

async fn handle_me(user: db::User) -> Result<impl Reply, Rejection> {
    Ok(warp::reply::json(&UserProfile::from(user)))
}

async fn handle_patch_me(user: db::User, body: PatchMeBody, pool: DbPool) -> Result<impl Reply, Rejection> {
    if body.bio.as_deref().is_some_and(|s| s.chars().count() > 500) {
        return Err(reject(ApiError::BioTooLong));
    }
    if body.favorite_music.as_deref().is_some_and(|s| s.chars().count() > 200) {
        return Err(reject(ApiError::MusicTooLong));
    }

    let updated = db::update_profile(&pool, user.id, body.bio, body.favorite_music)
        .await
        .map_err(internal)?;

    Ok(warp::reply::json(&UserProfile::from(updated)))
}

#[derive(Serialize)]
struct PlayerMatchInfo {
    user_id: Option<Uuid>,
    username: Option<String>,
    max_chain: i16,
    total_chains: i16,
    nuisance_sent: i32,
    nuisance_received: i32,
    all_clears: i16,
    pieces_placed: i32,
}

#[derive(Serialize)]
struct MatchEntry {
    id: Uuid,
    played_at: DateTime<Utc>,
    duration_secs: f64,
    winner_slot: Option<i16>,
    ranked: bool,
    player1: PlayerMatchInfo,
    player2: PlayerMatchInfo,
}

#[derive(Deserialize)]
struct MatchHistoryQuery {
    limit: Option<String>,
    offset: Option<String>,
}

fn int_param(value: Option<&str>, default: i64) -> Result<i64, Rejection> {
    value.map_or(Ok(default), |s| {
        s.parse::<i64>()
            .ok()
            .filter(|&n| n >= 0)
            .ok_or_else(|| reject(ApiError::BadQuery))
    })
}

async fn handle_user_profile(user_id: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    let row = db::get_user_profile(&pool, user_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| reject(ApiError::NotFound))?;

    Ok(warp::reply::json(&row))
}

async fn handle_match_history(user_id: Uuid, query: MatchHistoryQuery, pool: DbPool) -> Result<impl Reply, Rejection> {
    let exists = db::user_exists(&pool, user_id).await.map_err(internal)?;
    if !exists {
        return Err(reject(ApiError::NotFound));
    }
    let limit = int_param(query.limit.as_deref(), 20)?.clamp(1, 100);
    let offset = int_param(query.offset.as_deref(), 0)?;
    let rows = db::get_match_history(&pool, user_id, limit, offset)
        .await
        .map_err(internal)?;

    let entries: Vec<MatchEntry> = rows
        .into_iter()
        .map(|r| MatchEntry {
            id: r.id,
            played_at: r.played_at,
            duration_secs: r.duration_secs,
            winner_slot: r.winner_slot,
            ranked: r.ranked,
            player1: PlayerMatchInfo {
                user_id: r.player1_id,
                username: r.player1_username,
                max_chain: r.p1_max_chain.unwrap_or(0),
                total_chains: r.p1_total_chains.unwrap_or(0),
                nuisance_sent: r.p1_nuisance_sent.unwrap_or(0),
                nuisance_received: r.p1_nuisance_received.unwrap_or(0),
                all_clears: r.p1_all_clears.unwrap_or(0),
                pieces_placed: r.p1_pieces_placed.unwrap_or(0),
            },
            player2: PlayerMatchInfo {
                user_id: r.player2_id,
                username: r.player2_username,
                max_chain: r.p2_max_chain.unwrap_or(0),
                total_chains: r.p2_total_chains.unwrap_or(0),
                nuisance_sent: r.p2_nuisance_sent.unwrap_or(0),
                nuisance_received: r.p2_nuisance_received.unwrap_or(0),
                all_clears: r.p2_all_clears.unwrap_or(0),
                pieces_placed: r.p2_pieces_placed.unwrap_or(0),
            },
        })
        .collect();

    Ok(warp::reply::json(&entries))
}

#[derive(Deserialize)]
struct SendFriendRequestBody {
    user_id: Uuid,
}

#[derive(Serialize)]
struct FriendListResponse {
    friends: Vec<db::FriendEntry>,
    sent: Vec<db::FriendEntry>,
    received: Vec<db::FriendEntry>,
}

#[derive(Deserialize)]
struct UserSearchQuery {
    q: Option<String>,
}

async fn handle_search_users(
    me: db::User,
    query: UserSearchQuery,
    pool: DbPool,
    limit: SearchLimit,
) -> Result<impl Reply, Rejection> {
    let key = me.id.to_string();
    if !rate_take(&limit, &key, MAX_SEARCHES, SEARCH_WINDOW) {
        return Err(reject(ApiError::TooManyRequests));
    }

    let q = query.q.as_deref().unwrap_or("").trim().to_owned();
    if q.len() < 2 {
        return Ok(warp::reply::json(&Vec::<db::UserSearchEntry>::new()));
    }

    let results = db::search_users(&pool, &q, me.id).await.map_err(internal)?;
    Ok(warp::reply::json(&results))
}

async fn handle_list_friends(me: db::User, pool: DbPool) -> Result<impl Reply, Rejection> {
    let list = db::list_friends(&pool, me.id).await.map_err(internal)?;

    Ok(warp::reply::json(&FriendListResponse {
        friends: list.friends,
        sent: list.sent,
        received: list.received,
    }))
}

async fn handle_send_friend_request(
    me: db::User,
    body: SendFriendRequestBody,
    pool: DbPool,
    limit: FriendLimit,
) -> Result<impl Reply, Rejection> {
    let key = me.id.to_string();
    if !rate_take(&limit, &key, MAX_FRIEND_REQS, FRIEND_WINDOW) {
        return Err(reject(ApiError::TooManyRequests));
    }

    match db::send_friend_request(&pool, me.id, body.user_id).await {
        Ok(()) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({})),
            warp::http::StatusCode::CREATED,
        )),
        Err(db::FriendshipError::SelfRequest) => Err(reject(ApiError::SelfFriendRequest)),
        Err(db::FriendshipError::AlreadyExists) => Err(reject(ApiError::FriendRequestExists)),
        Err(db::FriendshipError::UserNotFound) => Err(reject(ApiError::NotFound)),
        Err(db::FriendshipError::Db(e)) => Err(internal(e)),
    }
}

async fn handle_accept_friend(requester_id: Uuid, me: db::User, pool: DbPool) -> Result<impl Reply, Rejection> {
    let found = db::accept_friend_request(&pool, me.id, requester_id)
        .await
        .map_err(internal)?;

    if found {
        Ok(warp::reply::json(&serde_json::json!({})))
    } else {
        Err(reject(ApiError::NotFound))
    }
}

async fn handle_remove_friend(other_id: Uuid, me: db::User, pool: DbPool) -> Result<impl Reply, Rejection> {
    let found = db::remove_friend(&pool, me.id, other_id).await.map_err(internal)?;

    if found {
        Ok(warp::reply::json(&serde_json::json!({})))
    } else {
        Err(reject(ApiError::NotFound))
    }
}

pub async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    let e = if let Some(&e) = err.find::<ApiError>() {
        e
    } else if err.find::<warp::body::BodyDeserializeError>().is_some()
        || err.find::<warp::reject::PayloadTooLarge>().is_some()
        || err.find::<warp::reject::LengthRequired>().is_some()
        || err.find::<warp::reject::UnsupportedMediaType>().is_some()
    {
        ApiError::InvalidBody
    } else {
        ApiError::NotFound
    };
    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({ "error": e.message(), "code": e.code() })),
        e.status(),
    ))
}

pub fn routes(
    pool: DbPool,
    cmd_tx: mpsc::Sender<Command>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let cmd_tx = with(cmd_tx);
    let api = warp::path("api");
    let db = pool.clone();
    let pool = with(pool);
    let body_limit = warp::body::content_length_limit(16 * 1024);
    let login_limits = LoginLimits::default();
    let friend_limit: FriendLimit = new_rate_map();
    let search_limit: SearchLimit = new_rate_map();
    let registrations: IpLimit = new_rate_map();
    let password_checks = with(new_rate_map());

    let register = api
        .and(warp::path("register"))
        .and(warp::path::end())
        .and(warp::post())
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(client_addr())
        .and(with(registrations))
        .and_then(handle_register);

    let login = api
        .and(warp::path("login"))
        .and(warp::path::end())
        .and(warp::post())
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(with(login_limits))
        .and(client_addr())
        .and_then(handle_login);

    let friend_limit = with(friend_limit);

    let logout = api
        .and(warp::path("logout"))
        .and(warp::path::end())
        .and(warp::post())
        .and(bearer_token())
        .and(pool.clone())
        .and_then(handle_logout);

    let logout_all = api
        .and(warp::path("logout-all"))
        .and(warp::path::end())
        .and(warp::post())
        .and(authed_session(db.clone()))
        .and(pool.clone())
        .and(cmd_tx.clone())
        .and_then(handle_logout_all);

    let me_password = api
        .and(warp::path("me"))
        .and(warp::path("password"))
        .and(warp::path::end())
        .and(warp::post())
        .and(authed_session(db.clone()))
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(password_checks.clone())
        .and(cmd_tx.clone())
        .and_then(handle_change_password);

    let me_delete = api
        .and(warp::path("me"))
        .and(warp::path("delete"))
        .and(warp::path::end())
        .and(warp::post())
        .and(authed_session(db.clone()))
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(password_checks)
        .and(cmd_tx)
        .and_then(handle_delete_account);

    let me_get = api
        .and(warp::path("me"))
        .and(warp::path::end())
        .and(warp::get())
        .and(authed(db.clone()))
        .and_then(handle_me);

    let me_patch = api
        .and(warp::path("me"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(authed(db.clone()))
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and_then(handle_patch_me);

    let users_search = api
        .and(warp::path("users"))
        .and(warp::path("search"))
        .and(warp::path::end())
        .and(warp::get())
        .and(authed(db.clone()))
        .and(warp::query::<UserSearchQuery>())
        .and(pool.clone())
        .and(with(search_limit))
        .and_then(handle_search_users);

    let user_profile = api
        .and(warp::path("users"))
        .and(warp::path::param::<Uuid>())
        .and(warp::path::end())
        .and(warp::get())
        .and(pool.clone())
        .and_then(handle_user_profile);

    let match_history = api
        .and(warp::path("users"))
        .and(warp::path::param::<Uuid>())
        .and(warp::path("matches"))
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query::<MatchHistoryQuery>())
        .and(pool.clone())
        .and_then(handle_match_history);

    let friends_get = api
        .and(warp::path("friends"))
        .and(warp::path::end())
        .and(warp::get())
        .and(authed(db.clone()))
        .and(pool.clone())
        .and_then(handle_list_friends);

    let friends_post = api
        .and(warp::path("friends"))
        .and(warp::path::end())
        .and(warp::post())
        .and(authed(db.clone()))
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(friend_limit)
        .and_then(handle_send_friend_request);

    let friends_accept = api
        .and(warp::path("friends"))
        .and(warp::path::param::<Uuid>())
        .and(warp::path("accept"))
        .and(warp::path::end())
        .and(warp::post())
        .and(authed(db.clone()))
        .and(pool.clone())
        .and_then(handle_accept_friend);

    let friends_delete = api
        .and(warp::path("friends"))
        .and(warp::path::param::<Uuid>())
        .and(warp::path::end())
        .and(warp::delete())
        .and(authed(db))
        .and(pool)
        .and_then(handle_remove_friend);

    register
        .or(login)
        .or(logout)
        .or(logout_all)
        .or(me_password)
        .or(me_delete)
        .or(me_get)
        .or(me_patch)
        .or(users_search)
        .or(user_profile)
        .or(match_history)
        .or(friends_get)
        .or(friends_post)
        .or(friends_accept)
        .or(friends_delete)
}

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

    #[tokio::test]
    async fn registrations_are_limited_per_forwarded_client() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(50))
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let api = routes(pool, mpsc::channel(8).0).recover(handle_rejection);
        let register = |from: &'static str, n: u32| {
            warp::test::request()
                .method("POST")
                .path("/api/register")
                .remote_addr("127.0.0.1:40000".parse().unwrap())
                .header("x-forwarded-for", from)
                .json(&serde_json::json!({ "username": format!("user_{n}"), "password": "password123" }))
        };
        for n in 0..MAX_REGISTRATIONS_PER_IP {
            let res = register("203.0.113.7", n).reply(&api).await;
            assert_ne!(res.status(), 429, "attempt {n} limited too early");
        }
        let res = register("203.0.113.7", 999).reply(&api).await;
        assert_eq!(res.status(), 429);
        let res = register("198.51.100.1, 203.0.113.7", 1000).reply(&api).await;
        assert_eq!(res.status(), 429);
        let res = register("198.51.100.1", 1001).reply(&api).await;
        assert_ne!(res.status(), 429);
    }

    #[tokio::test]
    async fn errors_carry_a_stable_code() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let api = routes(pool, mpsc::channel(8).0).recover(handle_rejection);
        let res = warp::test::request()
            .method("POST")
            .path("/api/register")
            .json(&serde_json::json!({ "username": "a", "password": "password123" }))
            .reply(&api)
            .await;
        assert_eq!(res.status(), 400);
        let body: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        assert_eq!(body["code"], "bad_username");
        assert!(body["error"].is_string(), "old clients still read 'error'");
    }

    async fn post(path: &str, body: String) -> (u16, String) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let api = routes(pool, mpsc::channel(8).0).recover(handle_rejection);
        let res = warp::test::request()
            .method("POST")
            .path(path)
            .header("content-type", "application/json")
            .body(body)
            .reply(&api)
            .await;
        let json: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        (
            res.status().as_u16(),
            json["code"].as_str().unwrap_or_default().to_owned(),
        )
    }

    #[tokio::test]
    async fn a_username_that_cannot_exist_fails_without_a_lookup() {
        let body = serde_json::json!({ "username": "x".repeat(5000), "password": "password123" }).to_string();
        assert_eq!(post("/api/login", body).await, (401, "bad_credentials".to_owned()));
    }

    #[tokio::test]
    async fn an_oversized_body_is_an_invalid_request() {
        let body = serde_json::json!({ "username": "alice", "password": "p".repeat(20_000) }).to_string();
        assert_eq!(post("/api/login", body).await, (400, "invalid_body".to_owned()));
    }

    #[test]
    fn ipv4_mapped_ipv6_is_the_ipv4_address() {
        assert_eq!(
            client_key(addr("[::ffff:203.0.113.7]:1"), None),
            Some("203.0.113.7".into())
        );
    }
}
