use std::sync::OnceLock;
use std::time::Duration;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

pub type DbPool = PgPool;

static DUMMY_HASH: OnceLock<String> = OnceLock::new();

const MAX_CONCURRENT_HASH_OPS: usize = 4;
static HASH_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

fn hash_semaphore() -> &'static Semaphore {
    HASH_SEMAPHORE.get_or_init(|| Semaphore::new(MAX_CONCURRENT_HASH_OPS))
}

/// Acquires a hash permit then runs `f` on a blocking thread.
/// At most `MAX_CONCURRENT_HASH_OPS` argon2 operations run in parallel across
/// both registration and login.
pub async fn run_hash<F, T>(f: F) -> Result<T, sqlx::Error>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _permit = hash_semaphore()
        .acquire()
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))
}

pub fn dummy_hash() -> &'static str {
    DUMMY_HASH.get_or_init(|| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(b"dummy_placeholder_never_matches_any_real_password", &salt)
            .expect("argon2 dummy hash init failed")
            .to_string()
    })
}

pub async fn cleanup_expired_sessions(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

#[derive(sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    password_hash: String,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub elo: i32,
    pub created_at: DateTime<Utc>,
}

impl User {
    pub(crate) fn password_hash(&self) -> &str {
        &self.password_hash
    }
}

pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}

pub async fn create_user(pool: &DbPool, username: &str, password: &str) -> Result<User, sqlx::Error> {
    let password = password.to_owned();
    let hash = run_hash(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
    })
    .await?
    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    sqlx::query_as::<_, User>("INSERT INTO users (username, password_hash) VALUES ($1, $2) RETURNING *")
        .bind(username)
        .bind(hash)
        .fetch_one(pool)
        .await
}

pub async fn find_user_by_username(pool: &DbPool, username: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

pub async fn create_session(pool: &DbPool, user_id: Uuid) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>("INSERT INTO sessions (user_id) VALUES ($1) RETURNING token")
        .bind(user_id)
        .fetch_one(pool)
        .await
}

pub async fn find_user_by_token(pool: &DbPool, token: Uuid) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT u.* FROM users u \
         JOIN sessions s ON s.user_id = u.id \
         WHERE s.token = $1 AND s.expires_at > now()",
    )
    .bind(token)
    .fetch_optional(pool)
    .await
}

pub async fn delete_session(pool: &DbPool, token: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(token)
        .execute(pool)
        .await?;
    Ok(())
}

pub struct MatchRecord {
    pub duration_secs: f64,
    pub winner_slot: u8,
    pub user_ids: [Option<Uuid>; 2],
    pub max_chain: [u32; 2],
    pub total_chains: [u32; 2],
    pub nuisance_sent: [u32; 2],
    pub all_clears: [u32; 2],
    pub pieces_placed: [u32; 2],
}

pub async fn record_match_result(pool: &DbPool, rec: MatchRecord) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    let match_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO matches (duration_secs, player1_id, player2_id, winner_slot) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(rec.duration_secs)
    .bind(rec.user_ids[0])
    .bind(rec.user_ids[1])
    .bind(rec.winner_slot as i16)
    .fetch_one(&mut *tx)
    .await?;

    for i in 0..2usize {
        sqlx::query(
            "INSERT INTO match_stats \
             (match_id, user_id, slot, max_chain, total_chains, \
              nuisance_sent, nuisance_received, all_clears, pieces_placed) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(match_id)
        .bind(rec.user_ids[i])
        .bind((i + 1) as i16)
        .bind(rec.max_chain[i] as i16)
        .bind(rec.total_chains[i] as i16)
        .bind(rec.nuisance_sent[i] as i32)
        .bind(rec.nuisance_sent[1 - i] as i32)
        .bind(rec.all_clears[i] as i16)
        .bind(rec.pieces_placed[i] as i32)
        .execute(&mut *tx)
        .await?;
    }

    if let (Some(uid0), Some(uid1)) = (rec.user_ids[0], rec.user_ids[1]) {
        let elo0 = sqlx::query_scalar::<_, i32>("SELECT elo FROM users WHERE id = $1 FOR UPDATE")
            .bind(uid0)
            .fetch_one(&mut *tx)
            .await?;
        let elo1 = sqlx::query_scalar::<_, i32>("SELECT elo FROM users WHERE id = $1 FOR UPDATE")
            .bind(uid1)
            .fetch_one(&mut *tx)
            .await?;
        debug_assert!(
            rec.winner_slot >= 1,
            "winner_slot must be 1 or 2, got {}",
            rec.winner_slot
        );
        let [new0, new1] = compute_elo(elo0, elo1, rec.winner_slot as usize - 1);
        sqlx::query("UPDATE users SET elo = $2 WHERE id = $1")
            .bind(uid0)
            .bind(new0)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE users SET elo = $2 WHERE id = $1")
            .bind(uid1)
            .bind(new1)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await
}

fn compute_elo(elo0: i32, elo1: i32, winner: usize) -> [i32; 2] {
    const K: f64 = 32.0;
    let expected0 = 1.0 / (1.0 + 10f64.powf((elo1 - elo0) as f64 / 400.0));
    let actual0 = if winner == 0 { 1.0 } else { 0.0 };
    let delta = (K * (actual0 - expected0)).round() as i32;
    [(elo0 + delta).max(0), (elo1 - delta).max(0)]
}

pub async fn update_profile(
    pool: &DbPool,
    user_id: Uuid,
    bio: Option<String>,
    favorite_music: Option<String>,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "UPDATE users SET \
         bio = CASE WHEN $2 IS NOT NULL THEN $2 ELSE bio END, \
         favorite_music = CASE WHEN $3 IS NOT NULL THEN $3 ELSE favorite_music END \
         WHERE id = $1 RETURNING *",
    )
    .bind(user_id)
    .bind(bio)
    .bind(favorite_music)
    .fetch_one(pool)
    .await
}

#[derive(sqlx::FromRow)]
pub struct UserProfileRow {
    pub id: Uuid,
    pub username: String,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub elo: i32,
    pub created_at: DateTime<Utc>,
    pub total_matches: i64,
    pub wins: i64,
    pub all_time_max_chain: i32,
    pub total_nuisance_sent: i64,
    pub total_all_clears: i64,
}

pub async fn user_exists(pool: &DbPool, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(user_id)
        .fetch_one(pool)
        .await
}

pub async fn get_user_profile(pool: &DbPool, user_id: Uuid) -> Result<Option<UserProfileRow>, sqlx::Error> {
    sqlx::query_as::<_, UserProfileRow>(
        "SELECT u.id, u.username, u.bio, u.favorite_music, u.avatar_url, u.banner_url, u.elo, u.created_at,
                COUNT(DISTINCT m.id)::bigint AS total_matches,
                COUNT(DISTINCT CASE WHEN
                    (m.winner_slot = 1 AND m.player1_id = u.id) OR
                    (m.winner_slot = 2 AND m.player2_id = u.id)
                    THEN m.id END)::bigint AS wins,
                COALESCE(MAX(ms.max_chain)::int, 0) AS all_time_max_chain,
                COALESCE(SUM(ms.nuisance_sent)::bigint, 0) AS total_nuisance_sent,
                COALESCE(SUM(ms.all_clears)::bigint, 0) AS total_all_clears
         FROM users u
         LEFT JOIN matches m ON m.player1_id = u.id OR m.player2_id = u.id
         LEFT JOIN match_stats ms ON ms.match_id = m.id AND ms.user_id = u.id
         WHERE u.id = $1
         GROUP BY u.id",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

#[derive(sqlx::FromRow)]
pub struct MatchRow {
    pub id: Uuid,
    pub played_at: DateTime<Utc>,
    pub duration_secs: f64,
    pub winner_slot: i16,
    pub player1_id: Option<Uuid>,
    pub player2_id: Option<Uuid>,
    pub player1_username: Option<String>,
    pub player2_username: Option<String>,
    pub p1_max_chain: Option<i16>,
    pub p1_total_chains: Option<i16>,
    pub p1_nuisance_sent: Option<i32>,
    pub p1_nuisance_received: Option<i32>,
    pub p1_all_clears: Option<i16>,
    pub p1_pieces_placed: Option<i32>,
    pub p2_max_chain: Option<i16>,
    pub p2_total_chains: Option<i16>,
    pub p2_nuisance_sent: Option<i32>,
    pub p2_nuisance_received: Option<i32>,
    pub p2_all_clears: Option<i16>,
    pub p2_pieces_placed: Option<i32>,
}

pub enum FriendshipError {
    AlreadyExists,
    UserNotFound,
    SelfRequest,
    Db(sqlx::Error),
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct FriendEntry {
    pub user_id: Uuid,
    pub username: String,
    pub elo: i32,
}

pub struct FriendList {
    pub friends: Vec<FriendEntry>,
    pub sent: Vec<FriendEntry>,
    pub received: Vec<FriendEntry>,
}

pub async fn send_friend_request(pool: &DbPool, requester: Uuid, target: Uuid) -> Result<(), FriendshipError> {
    if requester == target {
        return Err(FriendshipError::SelfRequest);
    }
    match sqlx::query("INSERT INTO friendships (user_id, friend_id) VALUES ($1, $2)")
        .bind(requester)
        .bind(target)
        .execute(pool)
        .await
    {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => Err(FriendshipError::AlreadyExists),
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23503") => Err(FriendshipError::UserNotFound),
        Err(e) => Err(FriendshipError::Db(e)),
    }
}

pub async fn accept_friend_request(pool: &DbPool, me: Uuid, requester: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE friendships SET status = 'accepted' \
         WHERE user_id = $1 AND friend_id = $2 AND status = 'pending'",
    )
    .bind(requester)
    .bind(me)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn remove_friend(pool: &DbPool, me: Uuid, other: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "DELETE FROM friendships \
         WHERE (user_id = $1 AND friend_id = $2) OR (user_id = $2 AND friend_id = $1)",
    )
    .bind(me)
    .bind(other)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_friends(pool: &DbPool, me: Uuid) -> Result<FriendList, sqlx::Error> {
    let friends = sqlx::query_as::<_, FriendEntry>(
        "SELECT u.id AS user_id, u.username, u.elo
         FROM friendships f
         JOIN users u ON u.id = CASE WHEN f.user_id = $1 THEN f.friend_id ELSE f.user_id END
         WHERE (f.user_id = $1 OR f.friend_id = $1) AND f.status = 'accepted'",
    )
    .bind(me)
    .fetch_all(pool)
    .await?;

    let sent = sqlx::query_as::<_, FriendEntry>(
        "SELECT u.id AS user_id, u.username, u.elo
         FROM friendships f
         JOIN users u ON u.id = f.friend_id
         WHERE f.user_id = $1 AND f.status = 'pending'",
    )
    .bind(me)
    .fetch_all(pool)
    .await?;

    let received = sqlx::query_as::<_, FriendEntry>(
        "SELECT u.id AS user_id, u.username, u.elo
         FROM friendships f
         JOIN users u ON u.id = f.user_id
         WHERE f.friend_id = $1 AND f.status = 'pending'",
    )
    .bind(me)
    .fetch_all(pool)
    .await?;

    Ok(FriendList {
        friends,
        sent,
        received,
    })
}

pub async fn get_match_history(pool: &DbPool, user_id: Uuid, limit: i64) -> Result<Vec<MatchRow>, sqlx::Error> {
    sqlx::query_as::<_, MatchRow>(
        "SELECT m.id, m.played_at, m.duration_secs, m.winner_slot,
                m.player1_id, m.player2_id,
                u1.username AS player1_username,
                u2.username AS player2_username,
                ms1.max_chain         AS p1_max_chain,
                ms1.total_chains      AS p1_total_chains,
                ms1.nuisance_sent     AS p1_nuisance_sent,
                ms1.nuisance_received AS p1_nuisance_received,
                ms1.all_clears        AS p1_all_clears,
                ms1.pieces_placed     AS p1_pieces_placed,
                ms2.max_chain         AS p2_max_chain,
                ms2.total_chains      AS p2_total_chains,
                ms2.nuisance_sent     AS p2_nuisance_sent,
                ms2.nuisance_received AS p2_nuisance_received,
                ms2.all_clears        AS p2_all_clears,
                ms2.pieces_placed     AS p2_pieces_placed
         FROM matches m
         LEFT JOIN users u1 ON u1.id = m.player1_id
         LEFT JOIN users u2 ON u2.id = m.player2_id
         LEFT JOIN match_stats ms1 ON ms1.match_id = m.id AND ms1.slot = 1
         LEFT JOIN match_stats ms2 ON ms2.match_id = m.id AND ms2.slot = 2
         WHERE m.player1_id = $1 OR m.player2_id = $1
         ORDER BY m.played_at DESC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}
