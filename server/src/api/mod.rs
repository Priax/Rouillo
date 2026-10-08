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

mod account;
mod error;
mod limits;
mod profile;
mod social;

use account::*;
use error::*;
pub use limits::ClientAddr;
use limits::*;
use profile::*;
use social::*;

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

fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    error!("{e}");
    ApiError::Internal
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
    solo_best: i32,
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
            solo_best: user.solo_best,
        }
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
        .route("/api/me/solo-best", post(handle_solo_best))
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
        .route("/api/version", get(handle_version))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(api)
}

/// The game version clients should run. The deploy waits for its signed
/// binaries, so a client told about it can always download them.
async fn handle_version() -> impl IntoResponse {
    Json(serde_json::json!({ "version": env!("CARGO_PKG_VERSION") }))
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

    #[tokio::test]
    async fn the_version_is_the_servers_own() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let res = routes(pool, mpsc::channel(8).0)
            .oneshot(Request::get("/api/version").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
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
}
