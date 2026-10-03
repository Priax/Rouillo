use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, JsonRejection, PathRejection, QueryRejection};
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

use crate::db::{self, DbPool};
use crate::images::{self, ImageError, Kind};
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
    BadImage,
    ImageTooLarge,
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message(), "code": self.code() }));
        (self.status(), body).into_response()
    }
}

impl From<JsonRejection> for ApiError {
    fn from(_: JsonRejection) -> Self {
        Self::InvalidBody
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        Self::NotFound
    }
}

impl From<QueryRejection> for ApiError {
    fn from(_: QueryRejection) -> Self {
        Self::NotFound
    }
}

#[derive(FromRequest)]
#[from_request(via(Json), rejection(ApiError))]
struct JsonBody<T>(T);

#[derive(FromRequestParts)]
#[from_request(via(Path), rejection(ApiError))]
struct PathParam<T>(T);

#[derive(FromRequestParts)]
#[from_request(via(Query), rejection(ApiError))]
struct Params<T>(T);

pub async fn not_found() -> Response {
    ApiError::NotFound.into_response()
}

impl ApiError {
    const fn status(self) -> StatusCode {
        match self {
            Self::BadUsername
            | Self::BadPassword
            | Self::BadQuery
            | Self::BioTooLong
            | Self::MusicTooLong
            | Self::SelfFriendRequest
            | Self::InvalidBody
            | Self::BadImage => StatusCode::BAD_REQUEST,
            Self::ImageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
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
            Self::BadImage => "bad_image",
            Self::ImageTooLarge => "image_too_large",
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
            Self::BadImage => "The image must be a PNG, JPEG, WebP or GIF picture",
            Self::ImageTooLarge => "The image must weigh 5 MB and measure 6000 pixels at most",
            Self::Internal => "Internal server error",
        }
    }
}

fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    error!("{e}");
    ApiError::Internal
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

pub struct ClientAddr(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for ClientAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        let xff = parts.headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
        Ok(Self(client_key(peer, xff)))
    }
}

type PasswordChecks = RateMap;

type FriendLimit = RateMap;
const MAX_FRIEND_REQS: u32 = 30;
const FRIEND_WINDOW: Duration = Duration::from_secs(600);

type UploadLimit = RateMap;
const MAX_UPLOADS: u32 = 20;
const UPLOAD_WINDOW: Duration = Duration::from_hours(1);

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
struct RenameBody {
    username: String,
    password: String,
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
    avatar_url: Option<String>,
}

impl AuthResponse {
    fn new(user: db::User, token: Uuid) -> Self {
        Self {
            token,
            user_id: user.id,
            username: user.username,
            elo: user.elo,
            avatar_url: user.avatar_url,
        }
    }
}

fn done() -> Json<serde_json::Value> {
    Json(serde_json::json!({}))
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

#[derive(Clone)]
struct Api {
    pool: DbPool,
    cmd_tx: mpsc::Sender<Command>,
    login_limits: LoginLimits,
    registrations: IpLimit,
    password_checks: PasswordChecks,
    friend_limit: FriendLimit,
    search_limit: SearchLimit,
    upload_limit: UploadLimit,
}

struct Bearer(Uuid);

impl<S: Send + Sync> FromRequestParts<S> for Bearer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|t| Uuid::parse_str(t).ok())
            .map(Self)
            .ok_or(ApiError::Unauthorized)
    }
}

struct Session(db::User, Uuid);

impl FromRequestParts<Api> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, api: &Api) -> Result<Self, ApiError> {
        let Bearer(token) = Bearer::from_request_parts(parts, api).await?;
        db::find_user_by_token(&api.pool, token)
            .await
            .map_err(internal)?
            .map(|user| Self(user, token))
            .ok_or(ApiError::Unauthorized)
    }
}

struct Authed(db::User);

impl FromRequestParts<Api> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, api: &Api) -> Result<Self, ApiError> {
        let Session(user, _) = Session::from_request_parts(parts, api).await?;
        Ok(Self(user))
    }
}

fn validate_password(p: &str) -> bool {
    let n = p.len();
    (8..=1024).contains(&n)
}

async fn handle_register(
    State(Api {
        pool, registrations, ..
    }): State<Api>,
    ClientAddr(client): ClientAddr,
    JsonBody(body): JsonBody<RegisterBody>,
) -> Result<impl IntoResponse, ApiError> {
    if !shared::valid_username(&body.username) {
        return Err(ApiError::BadUsername);
    }
    if !validate_password(&body.password) {
        return Err(ApiError::BadPassword);
    }
    if let Some(ip) = &client {
        if !rate_take(&registrations, ip, MAX_REGISTRATIONS_PER_IP, REGISTER_WINDOW) {
            return Err(ApiError::TooManyRequests);
        }
    }

    let user = match db::create_user(&pool, &body.username, &body.password).await {
        Ok(u) => u,
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => {
            return Err(ApiError::UsernameTaken);
        }
        Err(e) => return Err(internal(e)),
    };

    let token = db::create_session(&pool, user.id).await.map_err(internal)?;

    Ok((StatusCode::CREATED, Json(AuthResponse::new(user, token))))
}

async fn handle_login(
    State(Api {
        pool,
        login_limits: limits,
        ..
    }): State<Api>,
    ClientAddr(client): ClientAddr,
    JsonBody(body): JsonBody<LoginBody>,
) -> Result<impl IntoResponse, ApiError> {
    if !shared::valid_username(&body.username) {
        return Err(ApiError::BadCredentials);
    }
    let account = body.username.to_lowercase();
    let pair = attempt_key(client.as_deref(), &account);
    let counters = limits.counters(&pair, client.as_deref(), &account);
    if !reserve(&counters) {
        return Err(ApiError::TooManyRequests);
    }

    let user = db::find_user_by_username(&pool, &body.username)
        .await
        .map_err(internal)?;

    let hash = user
        .as_ref()
        .map_or_else(|| db::dummy_hash().to_owned(), |u| u.password_hash().to_owned());
    let password = body.password.clone();
    let ok = db::run_heavy(move || db::verify_password(&password, &hash))
        .await
        .map_err(internal)?;

    let (Some(user), true) = (user, ok) else {
        return Err(ApiError::BadCredentials);
    };

    for &(map, key, ..) in &counters {
        rate_refund(map, key);
    }
    rate_clear(&limits.pairs, &pair);

    let token = db::create_session(&pool, user.id).await.map_err(internal)?;

    Ok(Json(AuthResponse::new(user, token)))
}

async fn handle_logout(
    State(Api { pool, .. }): State<Api>,
    Bearer(token): Bearer,
) -> Result<impl IntoResponse, ApiError> {
    db::delete_session(&pool, token).await.map_err(internal)?;
    Ok(done())
}

async fn check_password(user: &db::User, password: String, checks: &PasswordChecks) -> Result<(), ApiError> {
    let key = user.id.to_string();
    if !rate_take(checks, &key, MAX_ATTEMPTS, WINDOW) {
        return Err(ApiError::TooManyRequests);
    }
    let hash = user.password_hash().to_owned();
    let ok = db::run_heavy(move || db::verify_password(&password, &hash))
        .await
        .map_err(internal)?;
    if !ok {
        return Err(ApiError::WrongPassword);
    }
    rate_clear(checks, &key);
    Ok(())
}

async fn handle_change_password(
    State(Api {
        pool,
        password_checks: checks,
        cmd_tx,
        ..
    }): State<Api>,
    Session(user, token): Session,
    JsonBody(body): JsonBody<ChangePasswordBody>,
) -> Result<impl IntoResponse, ApiError> {
    if !validate_password(&body.new) {
        return Err(ApiError::BadPassword);
    }
    check_password(&user, body.current, &checks).await?;
    let new = body.new;
    let hash = db::run_heavy(move || db::hash_password(&new))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    db::set_password(&pool, user.id, hash, token).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(done())
}

async fn handle_rename(
    State(Api {
        pool,
        password_checks: checks,
        cmd_tx,
        ..
    }): State<Api>,
    Authed(user): Authed,
    JsonBody(body): JsonBody<RenameBody>,
) -> Result<impl IntoResponse, ApiError> {
    if !shared::valid_username(&body.username) {
        return Err(ApiError::BadUsername);
    }
    check_password(&user, body.password, &checks).await?;
    let renamed = match db::rename_user(&pool, user.id, &body.username).await {
        Ok(u) => u,
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => {
            return Err(ApiError::UsernameTaken);
        }
        Err(e) => return Err(internal(e)),
    };
    let _ = cmd_tx
        .send(Command::Rename {
            user_id: renamed.id,
            username: renamed.username.clone(),
        })
        .await;
    Ok(Json(UserProfile::from(renamed)))
}

async fn upload(
    api: Api,
    user: db::User,
    kind: Kind,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<UserProfile>, ApiError> {
    let body = body.map_err(|e| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::ImageTooLarge
        } else {
            ApiError::InvalidBody
        }
    })?;
    if !rate_take(&api.upload_limit, &user.id.to_string(), MAX_UPLOADS, UPLOAD_WINDOW) {
        return Err(ApiError::TooManyRequests);
    }
    let image = db::run_heavy(move || images::process(kind, &body))
        .await
        .map_err(internal)?
        .map_err(|e| match e {
            ImageError::Unreadable => ApiError::BadImage,
            ImageError::TooBig => ApiError::ImageTooLarge,
        })?;
    let user = db::set_image(&api.pool, user.id, kind, image).await.map_err(internal)?;
    Ok(Json(UserProfile::from(user)))
}

async fn handle_avatar_upload(
    State(api): State<Api>,
    Authed(user): Authed,
    body: Result<Bytes, BytesRejection>,
) -> Result<impl IntoResponse, ApiError> {
    upload(api, user, Kind::Avatar, body).await
}

async fn handle_banner_upload(
    State(api): State<Api>,
    Authed(user): Authed,
    body: Result<Bytes, BytesRejection>,
) -> Result<impl IntoResponse, ApiError> {
    upload(api, user, Kind::Banner, body).await
}

async fn remove_image(api: &Api, user: &db::User, kind: Kind) -> Result<Json<UserProfile>, ApiError> {
    let user = db::delete_image(&api.pool, user.id, kind).await.map_err(internal)?;
    Ok(Json(UserProfile::from(user)))
}

async fn handle_avatar_delete(State(api): State<Api>, Authed(user): Authed) -> Result<impl IntoResponse, ApiError> {
    remove_image(&api, &user, Kind::Avatar).await
}

async fn handle_banner_delete(State(api): State<Api>, Authed(user): Authed) -> Result<impl IntoResponse, ApiError> {
    remove_image(&api, &user, Kind::Banner).await
}

async fn serve_image(api: &Api, user_id: Uuid, kind: Kind) -> Result<Response, ApiError> {
    let data = db::get_image(&api.pool, user_id, kind)
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound)?;
    let headers = [
        (header::CONTENT_TYPE, kind.content_type()),
        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
    ];
    Ok((headers, data).into_response())
}

async fn handle_avatar_get(
    State(api): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    serve_image(&api, user_id, Kind::Avatar).await
}

async fn handle_banner_get(
    State(api): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    serve_image(&api, user_id, Kind::Banner).await
}

async fn handle_logout_all(
    State(Api { pool, cmd_tx, .. }): State<Api>,
    Session(user, token): Session,
) -> Result<impl IntoResponse, ApiError> {
    db::delete_user_sessions(&pool, user.id).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(done())
}

async fn handle_delete_account(
    State(Api {
        pool,
        password_checks: checks,
        cmd_tx,
        ..
    }): State<Api>,
    Session(user, token): Session,
    JsonBody(body): JsonBody<DeleteAccountBody>,
) -> Result<impl IntoResponse, ApiError> {
    check_password(&user, body.password, &checks).await?;
    db::delete_user(&pool, user.id).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(done())
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

async fn handle_me(Authed(user): Authed) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(UserProfile::from(user)))
}

async fn handle_patch_me(
    State(Api { pool, .. }): State<Api>,
    Authed(user): Authed,
    JsonBody(body): JsonBody<PatchMeBody>,
) -> Result<impl IntoResponse, ApiError> {
    if body.bio.as_deref().is_some_and(|s| s.chars().count() > 500) {
        return Err(ApiError::BioTooLong);
    }
    if body.favorite_music.as_deref().is_some_and(|s| s.chars().count() > 200) {
        return Err(ApiError::MusicTooLong);
    }

    let updated = db::update_profile(&pool, user.id, body.bio, body.favorite_music)
        .await
        .map_err(internal)?;

    Ok(Json(UserProfile::from(updated)))
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

fn int_param(value: Option<&str>, default: i64) -> Result<i64, ApiError> {
    value.map_or(Ok(default), |s| {
        s.parse::<i64>().ok().filter(|&n| n >= 0).ok_or(ApiError::BadQuery)
    })
}

async fn handle_user_profile(
    State(Api { pool, .. }): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let row = db::get_user_profile(&pool, user_id)
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(row))
}

async fn handle_match_history(
    State(Api { pool, .. }): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
    Params(query): Params<MatchHistoryQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let exists = db::user_exists(&pool, user_id).await.map_err(internal)?;
    if !exists {
        return Err(ApiError::NotFound);
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

    Ok(Json(entries))
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
struct LeaderboardQuery {
    me: Option<Uuid>,
}

async fn handle_leaderboard(
    State(Api { pool, .. }): State<Api>,
    Params(query): Params<LeaderboardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let rows = db::leaderboard(&pool, query.me).await.map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct UserSearchQuery {
    q: Option<String>,
}

async fn handle_search_users(
    State(Api {
        pool,
        search_limit: limit,
        ..
    }): State<Api>,
    Authed(me): Authed,
    Params(query): Params<UserSearchQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let key = me.id.to_string();
    if !rate_take(&limit, &key, MAX_SEARCHES, SEARCH_WINDOW) {
        return Err(ApiError::TooManyRequests);
    }

    let q = query.q.as_deref().unwrap_or("").trim().to_owned();
    if q.len() < 2 {
        return Ok(Json(Vec::<db::UserSearchEntry>::new()));
    }

    let results = db::search_users(&pool, &q, me.id).await.map_err(internal)?;
    Ok(Json(results))
}

async fn handle_list_friends(
    State(Api { pool, .. }): State<Api>,
    Authed(me): Authed,
) -> Result<impl IntoResponse, ApiError> {
    let list = db::list_friends(&pool, me.id).await.map_err(internal)?;

    Ok(Json(FriendListResponse {
        friends: list.friends,
        sent: list.sent,
        received: list.received,
    }))
}

async fn handle_send_friend_request(
    State(Api {
        pool,
        friend_limit: limit,
        ..
    }): State<Api>,
    Authed(me): Authed,
    JsonBody(body): JsonBody<SendFriendRequestBody>,
) -> Result<impl IntoResponse, ApiError> {
    let key = me.id.to_string();
    if !rate_take(&limit, &key, MAX_FRIEND_REQS, FRIEND_WINDOW) {
        return Err(ApiError::TooManyRequests);
    }

    match db::send_friend_request(&pool, me.id, body.user_id).await {
        Ok(()) => Ok((StatusCode::CREATED, done())),
        Err(db::FriendshipError::SelfRequest) => Err(ApiError::SelfFriendRequest),
        Err(db::FriendshipError::AlreadyExists) => Err(ApiError::FriendRequestExists),
        Err(db::FriendshipError::UserNotFound) => Err(ApiError::NotFound),
        Err(db::FriendshipError::Db(e)) => Err(internal(e)),
    }
}

async fn handle_accept_friend(
    State(Api { pool, .. }): State<Api>,
    PathParam(requester_id): PathParam<Uuid>,
    Authed(me): Authed,
) -> Result<impl IntoResponse, ApiError> {
    let found = db::accept_friend_request(&pool, me.id, requester_id)
        .await
        .map_err(internal)?;

    if found {
        Ok(done())
    } else {
        Err(ApiError::NotFound)
    }
}

async fn handle_remove_friend(
    State(Api { pool, .. }): State<Api>,
    PathParam(other_id): PathParam<Uuid>,
    Authed(me): Authed,
) -> Result<impl IntoResponse, ApiError> {
    let found = db::remove_friend(&pool, me.id, other_id).await.map_err(internal)?;

    if found {
        Ok(done())
    } else {
        Err(ApiError::NotFound)
    }
}

pub fn routes(pool: DbPool, cmd_tx: mpsc::Sender<Command>) -> Router {
    let api = Api {
        pool,
        cmd_tx,
        login_limits: LoginLimits::default(),
        registrations: new_rate_map(),
        password_checks: new_rate_map(),
        friend_limit: new_rate_map(),
        search_limit: new_rate_map(),
        upload_limit: new_rate_map(),
    };
    Router::new()
        .route("/api/register", post(handle_register))
        .route("/api/login", post(handle_login))
        .route("/api/logout", post(handle_logout))
        .route("/api/logout-all", post(handle_logout_all))
        .route("/api/me", get(handle_me).patch(handle_patch_me))
        .route("/api/me/password", post(handle_change_password))
        .route("/api/me/username", post(handle_rename))
        .route(
            "/api/me/avatar",
            put(handle_avatar_upload)
                .delete(handle_avatar_delete)
                .layer(DefaultBodyLimit::max(images::MAX_UPLOAD)),
        )
        .route(
            "/api/me/banner",
            put(handle_banner_upload)
                .delete(handle_banner_delete)
                .layer(DefaultBodyLimit::max(images::MAX_UPLOAD)),
        )
        .route("/api/users/{id}/avatar", get(handle_avatar_get))
        .route("/api/users/{id}/banner", get(handle_banner_get))
        .route("/api/me/delete", post(handle_delete_account))
        .route("/api/leaderboard", get(handle_leaderboard))
        .route("/api/users/search", get(handle_search_users))
        .route("/api/users/{id}", get(handle_user_profile))
        .route("/api/users/{id}/matches", get(handle_match_history))
        .route(
            "/api/friends",
            get(handle_list_friends).post(handle_send_friend_request),
        )
        .route("/api/friends/{id}", delete(handle_remove_friend))
        .route("/api/friends/{id}/accept", post(handle_accept_friend))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(api)
}

#[cfg(test)]
pub(crate) mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    pub(crate) fn post_json(path: &str, peer: Option<&str>, forwarded: Option<&str>, body: String) -> Request<Body> {
        let mut req = Request::post(path).header("content-type", "application/json");
        if let Some(f) = forwarded {
            req = req.header("x-forwarded-for", f);
        }
        let mut req = req.body(Body::from(body)).unwrap();
        if let Some(p) = peer {
            req.extensions_mut()
                .insert(ConnectInfo(p.parse::<SocketAddr>().unwrap()));
        }
        req
    }

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
        let api = routes(pool, mpsc::channel(8).0);
        let register = |from: &'static str, n: u32| {
            let body = serde_json::json!({ "username": format!("user_{n}"), "password": "password123" });
            let req = post_json("/api/register", Some("127.0.0.1:40000"), Some(from), body.to_string());
            api.clone().oneshot(req)
        };
        for n in 0..MAX_REGISTRATIONS_PER_IP {
            let res = register("203.0.113.7", n).await.unwrap();
            assert_ne!(res.status(), 429, "attempt {n} limited too early");
        }
        let res = register("203.0.113.7", 999).await.unwrap();
        assert_eq!(res.status(), 429);
        let res = register("198.51.100.1, 203.0.113.7", 1000).await.unwrap();
        assert_eq!(res.status(), 429);
        let res = register("198.51.100.1", 1001).await.unwrap();
        assert_ne!(res.status(), 429);
    }

    #[tokio::test]
    async fn errors_carry_a_stable_code() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let body = serde_json::json!({ "username": "a", "password": "password123" }).to_string();
        let res = routes(pool, mpsc::channel(8).0)
            .oneshot(post_json("/api/register", None, None, body))
            .await
            .unwrap();
        assert_eq!(res.status(), 400);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "bad_username");
        assert!(body["error"].is_string(), "old clients still read 'error'");
    }

    async fn post(path: &str, body: String) -> (u16, String) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let res = routes(pool, mpsc::channel(8).0)
            .oneshot(post_json(path, None, None, body))
            .await
            .unwrap();
        let status = res.status().as_u16();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json["code"].as_str().unwrap_or_default().to_owned())
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
