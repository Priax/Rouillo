use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;
use warp::{Filter, Rejection, Reply};

use crate::db::{self, DbPool};

#[derive(Debug)]
struct BadRequest(String);
impl warp::reject::Reject for BadRequest {}

#[derive(Debug)]
struct Unauthorized;
impl warp::reject::Reject for Unauthorized {}

#[derive(Debug)]
struct WrongPassword;
impl warp::reject::Reject for WrongPassword {}

#[derive(Debug)]
struct Conflict(String);
impl warp::reject::Reject for Conflict {}

#[derive(Debug)]
struct InternalError;
impl warp::reject::Reject for InternalError {}

fn internal<E: std::fmt::Display>(e: E) -> Rejection {
    error!("{e}");
    warp::reject::custom(InternalError)
}

#[derive(Debug)]
struct TooManyRequests;
impl warp::reject::Reject for TooManyRequests {}

type RateMap = Arc<Mutex<HashMap<String, (u32, Instant)>>>;

fn new_rate_map() -> RateMap {
    Arc::new(Mutex::new(HashMap::new()))
}

fn rate_check(map: &RateMap, key: &str, max: u32, window: Duration) -> bool {
    let mut m = map.lock().unwrap();
    let now = Instant::now();
    match m.get(key) {
        Some((count, since)) if now.duration_since(*since) < window => *count >= max,
        Some(_) => {
            m.remove(key);
            false
        }
        None => false,
    }
}

#[allow(
    clippy::significant_drop_tightening,
    reason = "the lock is needed until the last statement, which writes the entry"
)]
fn rate_record(map: &RateMap, key: &str, window: Duration) {
    let now = Instant::now();
    let mut m = map.lock().unwrap();
    if m.len() > 500 {
        m.retain(|_, (_, since)| now.duration_since(*since) < window);
    }
    let entry = m.entry(key.to_owned()).or_insert((0, now));
    if now.duration_since(entry.1) >= window {
        *entry = (1, now);
    } else {
        entry.0 += 1;
    }
}

fn rate_clear(map: &RateMap, key: &str) {
    map.lock().unwrap().remove(key);
}

type LoginAttempts = RateMap;
const MAX_ATTEMPTS: u32 = 10;
const WINDOW: Duration = Duration::from_mins(15);

fn attempt_key(client: Option<&str>, username: &str) -> String {
    format!("{}|{username}", client.unwrap_or(""))
}
fn is_rate_limited(attempts: &LoginAttempts, key: &str) -> bool {
    rate_check(attempts, key, MAX_ATTEMPTS, WINDOW)
}
fn record_failure(attempts: &LoginAttempts, key: &str) {
    rate_record(attempts, key, WINDOW);
}
fn clear_attempts(attempts: &LoginAttempts, key: &str) {
    rate_clear(attempts, key);
}

type IpLimit = RateMap;
const MAX_LOGIN_FAILURES_PER_IP: u32 = 30;
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
            .ok_or_else(|| warp::reject::custom(Unauthorized))
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
                .ok_or_else(|| warp::reject::custom(Unauthorized))
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
        return Err(warp::reject::custom(BadRequest(
            "Username must be 3-24 alphanumeric characters or underscores".into(),
        )));
    }
    if !validate_password(&body.password) {
        return Err(warp::reject::custom(BadRequest(
            "Password must be at least 8 characters".into(),
        )));
    }
    if let Some(ip) = &client {
        if rate_check(&registrations, ip, MAX_REGISTRATIONS_PER_IP, REGISTER_WINDOW) {
            return Err(warp::reject::custom(TooManyRequests));
        }
        rate_record(&registrations, ip, REGISTER_WINDOW);
    }

    let user = match db::create_user(&pool, &body.username, &body.password).await {
        Ok(u) => u,
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => {
            return Err(warp::reject::custom(Conflict("Username already taken".into())));
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
    attempts: LoginAttempts,
    client: Option<String>,
    ip_failures: IpLimit,
) -> Result<impl Reply, Rejection> {
    let attempt = attempt_key(client.as_deref(), &body.username);
    if is_rate_limited(&attempts, &attempt) {
        return Err(warp::reject::custom(TooManyRequests));
    }
    if let Some(ip) = &client {
        if rate_check(&ip_failures, ip, MAX_LOGIN_FAILURES_PER_IP, WINDOW) {
            return Err(warp::reject::custom(TooManyRequests));
        }
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
        record_failure(&attempts, &attempt);
        if let Some(ip) = &client {
            rate_record(&ip_failures, ip, WINDOW);
        }
        return Err(warp::reject::custom(Unauthorized));
    };

    clear_attempts(&attempts, &attempt);

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
    if rate_check(checks, &key, MAX_ATTEMPTS, WINDOW) {
        return Err(warp::reject::custom(TooManyRequests));
    }
    let hash = user.password_hash().to_owned();
    let ok = db::run_hash(move || db::verify_password(&password, &hash))
        .await
        .map_err(internal)?;
    if !ok {
        rate_record(checks, &key, WINDOW);
        return Err(warp::reject::custom(WrongPassword));
    }
    rate_clear(checks, &key);
    Ok(())
}

async fn handle_change_password(
    (user, token): (db::User, Uuid),
    body: ChangePasswordBody,
    pool: DbPool,
    checks: PasswordChecks,
) -> Result<impl Reply, Rejection> {
    if !validate_password(&body.new) {
        return Err(warp::reject::custom(BadRequest(
            "Password must be at least 8 characters".into(),
        )));
    }
    check_password(&user, body.current, &checks).await?;
    let new = body.new;
    let hash = db::run_hash(move || db::hash_password(&new))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    db::set_password(&pool, user.id, hash, token).await.map_err(internal)?;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn handle_logout_all(user: db::User, pool: DbPool) -> Result<impl Reply, Rejection> {
    db::delete_user_sessions(&pool, user.id).await.map_err(internal)?;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn handle_delete_account(
    user: db::User,
    body: DeleteAccountBody,
    pool: DbPool,
    checks: PasswordChecks,
) -> Result<impl Reply, Rejection> {
    check_password(&user, body.password, &checks).await?;
    db::delete_user(&pool, user.id).await.map_err(internal)?;
    Ok(warp::reply::json(&serde_json::json!({})))
}

async fn handle_me(user: db::User) -> Result<impl Reply, Rejection> {
    Ok(warp::reply::json(&UserProfile {
        id: user.id,
        username: user.username,
        bio: user.bio,
        favorite_music: user.favorite_music,
        avatar_url: user.avatar_url,
        banner_url: user.banner_url,
        elo: user.elo,
        created_at: user.created_at,
    }))
}

async fn handle_patch_me(user: db::User, body: PatchMeBody, pool: DbPool) -> Result<impl Reply, Rejection> {
    if body.bio.as_deref().is_some_and(|s| s.chars().count() > 500) {
        return Err(warp::reject::custom(BadRequest(
            "Bio must be 500 characters or less".into(),
        )));
    }
    if body.favorite_music.as_deref().is_some_and(|s| s.chars().count() > 200) {
        return Err(warp::reject::custom(BadRequest(
            "Favorite music must be 200 characters or less".into(),
        )));
    }

    let updated = db::update_profile(&pool, user.id, body.bio, body.favorite_music)
        .await
        .map_err(internal)?;

    Ok(warp::reply::json(&UserProfile {
        id: updated.id,
        username: updated.username,
        bio: updated.bio,
        favorite_music: updated.favorite_music,
        avatar_url: updated.avatar_url,
        banner_url: updated.banner_url,
        elo: updated.elo,
        created_at: updated.created_at,
    }))
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
    winner_slot: i16,
    player1: PlayerMatchInfo,
    player2: PlayerMatchInfo,
}

#[derive(Deserialize)]
struct MatchHistoryQuery {
    limit: Option<String>,
}

async fn handle_user_profile(user_id: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    let row = db::get_user_profile(&pool, user_id)
        .await
        .map_err(internal)?
        .ok_or_else(warp::reject::not_found)?;

    Ok(warp::reply::json(&row))
}

async fn handle_match_history(user_id: Uuid, query: MatchHistoryQuery, pool: DbPool) -> Result<impl Reply, Rejection> {
    let exists = db::user_exists(&pool, user_id).await.map_err(internal)?;
    if !exists {
        return Err(warp::reject::not_found());
    }
    let limit = match query.limit.as_deref() {
        None => 20i64,
        Some(s) => s
            .parse::<i64>()
            .map_err(|_| warp::reject::custom(BadRequest("'limit' must be a positive integer".into())))?,
    }
    .clamp(1, 100);
    let rows = db::get_match_history(&pool, user_id, limit).await.map_err(internal)?;

    let entries: Vec<MatchEntry> = rows
        .into_iter()
        .map(|r| MatchEntry {
            id: r.id,
            played_at: r.played_at,
            duration_secs: r.duration_secs,
            winner_slot: r.winner_slot,
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
    if rate_check(&limit, &key, MAX_SEARCHES, SEARCH_WINDOW) {
        return Err(warp::reject::custom(TooManyRequests));
    }
    rate_record(&limit, &key, SEARCH_WINDOW);

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
    if rate_check(&limit, &key, MAX_FRIEND_REQS, FRIEND_WINDOW) {
        return Err(warp::reject::custom(TooManyRequests));
    }
    rate_record(&limit, &key, FRIEND_WINDOW);

    match db::send_friend_request(&pool, me.id, body.user_id).await {
        Ok(()) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({})),
            warp::http::StatusCode::CREATED,
        )),
        Err(db::FriendshipError::SelfRequest) => Err(warp::reject::custom(BadRequest(
            "Cannot send a friend request to yourself".into(),
        ))),
        Err(db::FriendshipError::AlreadyExists) => {
            Err(warp::reject::custom(Conflict("Friend request already exists".into())))
        }
        Err(db::FriendshipError::UserNotFound) => Err(warp::reject::not_found()),
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
        Err(warp::reject::not_found())
    }
}

async fn handle_remove_friend(other_id: Uuid, me: db::User, pool: DbPool) -> Result<impl Reply, Rejection> {
    let found = db::remove_friend(&pool, me.id, other_id).await.map_err(internal)?;

    if found {
        Ok(warp::reply::json(&serde_json::json!({})))
    } else {
        Err(warp::reject::not_found())
    }
}

pub async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    let (status, message) = if let Some(e) = err.find::<BadRequest>() {
        (warp::http::StatusCode::BAD_REQUEST, e.0.clone())
    } else if err.find::<Unauthorized>().is_some() {
        (warp::http::StatusCode::UNAUTHORIZED, "Unauthorized".to_string())
    } else if err.find::<WrongPassword>().is_some() {
        (warp::http::StatusCode::FORBIDDEN, "Wrong password".to_string())
    } else if let Some(e) = err.find::<Conflict>() {
        (warp::http::StatusCode::CONFLICT, e.0.clone())
    } else if err.find::<TooManyRequests>().is_some() {
        (
            warp::http::StatusCode::TOO_MANY_REQUESTS,
            "Too many requests, please try again later".to_string(),
        )
    } else if err.find::<InternalError>().is_some() {
        (
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error".to_string(),
        )
    } else if err.find::<warp::body::BodyDeserializeError>().is_some() {
        (warp::http::StatusCode::BAD_REQUEST, "Invalid request body".to_string())
    } else {
        (warp::http::StatusCode::NOT_FOUND, "Not found".to_string())
    };

    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({ "error": message })),
        status,
    ))
}

pub fn routes(pool: DbPool) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let api = warp::path("api");
    let db = pool.clone();
    let pool = with(pool);
    let body_limit = warp::body::content_length_limit(16 * 1024);
    let attempts: LoginAttempts = new_rate_map();
    let friend_limit: FriendLimit = new_rate_map();
    let search_limit: SearchLimit = new_rate_map();
    let registrations: IpLimit = new_rate_map();
    let ip_login_failures: IpLimit = new_rate_map();
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
        .and(with(attempts))
        .and(client_addr())
        .and(with(ip_login_failures))
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
        .and(authed(db.clone()))
        .and(pool.clone())
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
        .and_then(handle_change_password);

    let me_delete = api
        .and(warp::path("me"))
        .and(warp::path("delete"))
        .and(warp::path::end())
        .and(warp::post())
        .and(authed(db.clone()))
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(password_checks)
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
        let api = routes(pool).recover(handle_rejection);
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

    #[test]
    fn ipv4_mapped_ipv6_is_the_ipv4_address() {
        assert_eq!(
            client_key(addr("[::ffff:203.0.113.7]:1"), None),
            Some("203.0.113.7".into())
        );
    }
}
