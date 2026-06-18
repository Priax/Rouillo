use std::collections::HashMap;
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

fn rate_record(map: &RateMap, key: &str, window: Duration) {
    let mut m = map.lock().unwrap();
    let now = Instant::now();
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
const WINDOW: Duration = Duration::from_secs(900);

fn is_rate_limited(attempts: &LoginAttempts, username: &str) -> bool {
    rate_check(attempts, username, MAX_ATTEMPTS, WINDOW)
}
fn record_failure(attempts: &LoginAttempts, username: &str) {
    rate_record(attempts, username, WINDOW);
}
fn clear_attempts(attempts: &LoginAttempts, username: &str) {
    rate_clear(attempts, username);
}

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

fn with_pool(pool: DbPool) -> impl Filter<Extract = (DbPool,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || pool.clone())
}

fn with_attempts(
    attempts: LoginAttempts,
) -> impl Filter<Extract = (LoginAttempts,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || attempts.clone())
}

fn with_friend_limit(
    limit: FriendLimit,
) -> impl Filter<Extract = (FriendLimit,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || limit.clone())
}

fn with_search_limit(
    limit: SearchLimit,
) -> impl Filter<Extract = (SearchLimit,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || limit.clone())
}

fn bearer_token() -> impl Filter<Extract = (Uuid,), Error = Rejection> + Clone {
    warp::header::optional::<String>("authorization").and_then(|h: Option<String>| async move {
        h.as_deref()
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|t| Uuid::parse_str(t).ok())
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

async fn handle_register(body: RegisterBody, pool: DbPool) -> Result<impl Reply, Rejection> {
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

async fn handle_login(body: LoginBody, pool: DbPool, attempts: LoginAttempts) -> Result<impl Reply, Rejection> {
    if is_rate_limited(&attempts, &body.username) {
        return Err(warp::reject::custom(TooManyRequests));
    }

    let user = db::find_user_by_username(&pool, &body.username)
        .await
        .map_err(internal)?;

    let hash = user
        .as_ref()
        .map(|u| u.password_hash().to_owned())
        .unwrap_or_else(|| db::dummy_hash().to_owned());
    let password = body.password.clone();
    let ok = db::run_hash(move || db::verify_password(&password, &hash))
        .await
        .map_err(internal)?;

    let user = match (user, ok) {
        (Some(u), true) => u,
        _ => {
            record_failure(&attempts, &body.username);
            return Err(warp::reject::custom(Unauthorized));
        }
    };

    clear_attempts(&attempts, &body.username);

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

async fn handle_me(token: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    let user = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

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

async fn handle_patch_me(token: Uuid, body: PatchMeBody, pool: DbPool) -> Result<impl Reply, Rejection> {
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

    let user = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

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
struct PublicProfile {
    id: Uuid,
    username: String,
    bio: Option<String>,
    favorite_music: Option<String>,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    elo: i32,
    created_at: DateTime<Utc>,
    total_matches: i64,
    wins: i64,
    all_time_max_chain: i32,
    total_nuisance_sent: i64,
    total_all_clears: i64,
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

    Ok(warp::reply::json(&PublicProfile {
        id: row.id,
        username: row.username,
        bio: row.bio,
        favorite_music: row.favorite_music,
        avatar_url: row.avatar_url,
        banner_url: row.banner_url,
        elo: row.elo,
        created_at: row.created_at,
        total_matches: row.total_matches,
        wins: row.wins,
        all_time_max_chain: row.all_time_max_chain,
        total_nuisance_sent: row.total_nuisance_sent,
        total_all_clears: row.total_all_clears,
    }))
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
    token: Uuid,
    query: UserSearchQuery,
    pool: DbPool,
    limit: SearchLimit,
) -> Result<impl Reply, Rejection> {
    let me = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

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

async fn handle_list_friends(token: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    let me = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

    let list = db::list_friends(&pool, me.id).await.map_err(internal)?;

    Ok(warp::reply::json(&FriendListResponse {
        friends: list.friends,
        sent: list.sent,
        received: list.received,
    }))
}

async fn handle_send_friend_request(
    token: Uuid,
    body: SendFriendRequestBody,
    pool: DbPool,
    limit: FriendLimit,
) -> Result<impl Reply, Rejection> {
    let me = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

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

async fn handle_accept_friend(requester_id: Uuid, token: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    let me = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

    let found = db::accept_friend_request(&pool, me.id, requester_id)
        .await
        .map_err(internal)?;

    if found {
        Ok(warp::reply::json(&serde_json::json!({})))
    } else {
        Err(warp::reject::not_found())
    }
}

async fn handle_remove_friend(other_id: Uuid, token: Uuid, pool: DbPool) -> Result<impl Reply, Rejection> {
    let me = db::find_user_by_token(&pool, token)
        .await
        .map_err(internal)?
        .ok_or_else(|| warp::reject::custom(Unauthorized))?;

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
    let pool = with_pool(pool);
    let body_limit = warp::body::content_length_limit(16 * 1024);
    let attempts: LoginAttempts = new_rate_map();
    let friend_limit: FriendLimit = new_rate_map();
    let search_limit: SearchLimit = new_rate_map();

    let register = api
        .and(warp::path("register"))
        .and(warp::path::end())
        .and(warp::post())
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and_then(handle_register);

    let login = api
        .and(warp::path("login"))
        .and(warp::path::end())
        .and(warp::post())
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and(with_attempts(attempts))
        .and_then(handle_login);

    let friend_limit = with_friend_limit(friend_limit);

    let logout = api
        .and(warp::path("logout"))
        .and(warp::path::end())
        .and(warp::post())
        .and(bearer_token())
        .and(pool.clone())
        .and_then(handle_logout);

    let me_get = api
        .and(warp::path("me"))
        .and(warp::path::end())
        .and(warp::get())
        .and(bearer_token())
        .and(pool.clone())
        .and_then(handle_me);

    let me_patch = api
        .and(warp::path("me"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(bearer_token())
        .and(body_limit)
        .and(warp::body::json())
        .and(pool.clone())
        .and_then(handle_patch_me);

    let users_search = api
        .and(warp::path("users"))
        .and(warp::path("search"))
        .and(warp::path::end())
        .and(warp::get())
        .and(bearer_token())
        .and(warp::query::<UserSearchQuery>())
        .and(pool.clone())
        .and(with_search_limit(search_limit))
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
        .and(bearer_token())
        .and(pool.clone())
        .and_then(handle_list_friends);

    let friends_post = api
        .and(warp::path("friends"))
        .and(warp::path::end())
        .and(warp::post())
        .and(bearer_token())
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
        .and(bearer_token())
        .and(pool.clone())
        .and_then(handle_accept_friend);

    let friends_delete = api
        .and(warp::path("friends"))
        .and(warp::path::param::<Uuid>())
        .and(warp::path::end())
        .and(warp::delete())
        .and(bearer_token())
        .and(pool.clone())
        .and_then(handle_remove_friend);

    register
        .or(login)
        .or(logout)
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
