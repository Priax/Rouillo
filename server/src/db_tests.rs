use sqlx::PgPool;
use uuid::Uuid;

use crate::db::{self, FriendshipError, MatchRecord};

async fn user(pool: &PgPool, name: &str) -> Uuid {
    db::create_user(pool, name, "password123")
        .await
        .expect("create user")
        .id
}

fn game(ranked: bool, p1: Option<Uuid>, p2: Option<Uuid>, winner_slot: u8) -> MatchRecord {
    MatchRecord {
        ranked,
        duration_secs: 60.0,
        winner_slot: Some(winner_slot),
        user_ids: [p1, p2],
        max_chain: [3, 1],
        total_chains: [2, 1],
        nuisance_sent: [40, 5],
        all_clears: [0, 0],
        pieces_placed: [30, 28],
    }
}

fn series(winner: Uuid, loser: Uuid) -> db::SeriesRecord {
    db::SeriesRecord {
        winner,
        loser,
        winner_delta: 16,
        loser_delta: -16,
    }
}

async fn elo(pool: &PgPool, id: Uuid) -> i32 {
    sqlx::query_scalar("SELECT elo FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_session_finds_its_user_until_it_expires(pool: PgPool) {
    let id = user(&pool, "alice").await;
    let token = db::create_session(&pool, id).await.unwrap();
    assert_eq!(
        db::find_user_by_token(&pool, token).await.unwrap().map(|u| u.id),
        Some(id)
    );
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(db::find_user_by_token(&pool, token).await.unwrap().is_none());
    assert_eq!(db::cleanup_expired_sessions(&pool).await.unwrap(), 1);
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_new_password_keeps_only_the_current_session(pool: PgPool) {
    let id = user(&pool, "alice").await;
    let kept = db::create_session(&pool, id).await.unwrap();
    let other = db::create_session(&pool, id).await.unwrap();
    let hash = db::hash_password("nouveau123").unwrap();
    db::set_password(&pool, id, hash, kept).await.unwrap();
    assert!(db::find_user_by_token(&pool, kept).await.unwrap().is_some());
    assert!(db::find_user_by_token(&pool, other).await.unwrap().is_none());
    let user = db::find_user_by_username(&pool, "alice").await.unwrap().unwrap();
    assert!(db::verify_password("nouveau123", user.password_hash()));
    assert!(!db::verify_password("password123", user.password_hash()));
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_logging_out_everywhere_ends_every_session(pool: PgPool) {
    let id = user(&pool, "alice").await;
    let a = db::create_session(&pool, id).await.unwrap();
    let b = db::create_session(&pool, id).await.unwrap();
    db::delete_user_sessions(&pool, id).await.unwrap();
    assert!(db::find_user_by_token(&pool, a).await.unwrap().is_none());
    assert!(db::find_user_by_token(&pool, b).await.unwrap().is_none());
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_deleted_account_leaves_its_games_to_the_opponent(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    let token = db::create_session(&pool, alice).await.unwrap();
    assert!(db::send_friend_request(&pool, alice, bob).await.is_ok());
    db::record_match_result(&pool, game(false, Some(alice), Some(bob), 1))
        .await
        .unwrap();

    db::delete_user(&pool, alice).await.unwrap();

    assert!(db::find_user_by_token(&pool, token).await.unwrap().is_none());
    assert!(db::list_friends(&pool, bob).await.unwrap().received.is_empty());
    let history = db::get_match_history(&pool, bob, 10, 0).await.unwrap();
    assert_eq!(history.len(), 1, "bob keeps the game");
    assert_eq!(history[0].player1_id, None);
    assert_eq!(history[0].player2_username.as_deref(), Some("bob"));
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_game_with_a_vanished_player_is_still_saved(pool: PgPool) {
    let bob = user(&pool, "bob").await;
    let gone = Uuid::from_u128(42);
    db::record_match_result(&pool, game(true, Some(gone), Some(bob), 2))
        .await
        .unwrap();
    let history = db::get_match_history(&pool, bob, 10, 0).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!((history[0].player1_id, history[0].ranked), (None, true));
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_games_never_move_elo_only_series_do(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    db::record_match_result(&pool, game(false, Some(alice), Some(bob), 1))
        .await
        .unwrap();
    db::record_match_result(&pool, game(true, Some(alice), Some(bob), 1))
        .await
        .unwrap();
    assert_eq!((elo(&pool, alice).await, elo(&pool, bob).await), (1000, 1000));

    db::record_series(&pool, series(alice, bob)).await.unwrap();
    assert_eq!((elo(&pool, alice).await, elo(&pool, bob).await), (1016, 984));
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_casual_and_ranked_win_rates_are_counted_apart(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    db::record_match_result(&pool, game(false, Some(alice), Some(bob), 1))
        .await
        .unwrap();
    db::record_match_result(&pool, game(false, Some(alice), Some(bob), 2))
        .await
        .unwrap();
    let draw = MatchRecord {
        winner_slot: None,
        ..game(false, Some(alice), Some(bob), 1)
    };
    db::record_match_result(&pool, draw).await.unwrap();
    for _ in 0..3 {
        db::record_match_result(&pool, game(true, Some(alice), Some(bob), 1))
            .await
            .unwrap();
    }
    db::record_series(&pool, series(alice, bob)).await.unwrap();
    db::record_series(&pool, series(bob, alice)).await.unwrap();
    db::record_series(&pool, series(alice, bob)).await.unwrap();

    let p = db::get_user_profile(&pool, alice).await.unwrap().unwrap();
    assert_eq!((p.casual_matches, p.casual_wins), (3, 1), "a draw is played, not won");
    assert_eq!(
        (p.ranked_series, p.ranked_series_won),
        (3, 2),
        "ranked counts series, not games"
    );
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_history_pages_do_not_overlap(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    for _ in 0..5 {
        db::record_match_result(&pool, game(false, Some(alice), None, 1))
            .await
            .unwrap();
    }
    let mut seen = Vec::new();
    for page in 0..3 {
        seen.extend(
            db::get_match_history(&pool, alice, 2, page * 2)
                .await
                .unwrap()
                .into_iter()
                .map(|m| m.id),
        );
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5, "every game once, even with equal timestamps");
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_series_against_a_deleted_account_changes_nothing(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    db::record_series(&pool, series(alice, Uuid::from_u128(42)))
        .await
        .unwrap();
    assert_eq!(elo(&pool, alice).await, 1000);
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_only_casual_games_count_towards_ranked(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    let carol = user(&pool, "carol").await;
    for _ in 0..3 {
        db::record_match_result(&pool, game(false, Some(alice), None, 1))
            .await
            .unwrap();
    }
    db::record_match_result(&pool, game(true, Some(alice), Some(bob), 1))
        .await
        .unwrap();
    db::record_match_result(&pool, game(false, Some(bob), Some(carol), 1))
        .await
        .unwrap();
    assert_eq!(db::ranked_profile(&pool, alice).await.unwrap(), Some((1000, 3)));
    assert_eq!(db::ranked_profile(&pool, carol).await.unwrap(), Some((1000, 1)));
    assert_eq!(db::ranked_profile(&pool, Uuid::from_u128(42)).await.unwrap(), None);
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_friendship_runs_from_request_to_removal(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    assert!(matches!(
        db::send_friend_request(&pool, alice, alice).await,
        Err(FriendshipError::SelfRequest)
    ));
    assert!(matches!(
        db::send_friend_request(&pool, alice, Uuid::from_u128(42)).await,
        Err(FriendshipError::UserNotFound)
    ));
    assert!(db::send_friend_request(&pool, alice, bob).await.is_ok());
    assert!(
        matches!(
            db::send_friend_request(&pool, alice, bob).await,
            Err(FriendshipError::AlreadyExists)
        ),
        "one request per pair"
    );
    assert_eq!(db::list_friends(&pool, bob).await.unwrap().received.len(), 1);
    assert!(!db::are_friends(&pool, alice, bob).await.unwrap());
    assert!(
        !db::accept_friend_request(&pool, alice, bob).await.unwrap(),
        "only the target accepts"
    );
    assert!(db::accept_friend_request(&pool, bob, alice).await.unwrap());
    assert!(db::are_friends(&pool, bob, alice).await.unwrap());
    assert!(db::remove_friend(&pool, bob, alice).await.unwrap());
    assert!(!db::are_friends(&pool, alice, bob).await.unwrap());
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_search_treats_wildcards_as_text(pool: PgPool) {
    let me = user(&pool, "me").await;
    user(&pool, "a_b").await;
    user(&pool, "axb").await;
    let found = db::search_users(&pool, "a_", me).await.unwrap();
    let names: Vec<_> = found.iter().map(|u| u.username.as_str()).collect();
    assert_eq!(names, ["a_b"]);
    assert!(db::search_users(&pool, "%", me).await.unwrap().is_empty());
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_profile_counts_wins_from_either_slot(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    db::record_match_result(&pool, game(false, Some(alice), Some(bob), 1))
        .await
        .unwrap();
    db::record_match_result(&pool, game(false, Some(bob), Some(alice), 2))
        .await
        .unwrap();
    db::record_match_result(&pool, game(false, Some(bob), Some(alice), 1))
        .await
        .unwrap();
    let profile = db::get_user_profile(&pool, alice).await.unwrap().unwrap();
    assert_eq!((profile.total_matches, profile.wins), (3, 2));
    let history = db::get_match_history(&pool, alice, 2, 0).await.unwrap();
    assert_eq!(history.len(), 2, "the limit holds");
}

fn login_api(pool: PgPool) -> impl warp::Filter<Extract = impl warp::Reply, Error = std::convert::Infallible> + Clone {
    use warp::Filter;
    crate::auth::routes(pool, tokio::sync::mpsc::channel(8).0).recover(crate::auth::handle_rejection)
}

async fn wrong_logins<F>(api: &F, ip: &str, count: usize) -> Vec<u16>
where
    F: warp::Filter + Clone + Send + Sync + 'static,
    F::Extract: warp::Reply + Send,
{
    let tries = (0..count).map(|_| {
        warp::test::request()
            .method("POST")
            .path("/api/login")
            .remote_addr(format!("{ip}:5000").parse().unwrap())
            .json(&serde_json::json!({ "username": "alice", "password": "wrong-password" }))
            .reply(api)
    });
    futures_util::future::join_all(tries)
        .await
        .iter()
        .map(|r| r.status().as_u16())
        .collect()
}

fn count(statuses: &[u16], code: u16) -> usize {
    statuses.iter().filter(|&&s| s == code).count()
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_simultaneous_logins_cannot_slip_past_the_limit(pool: PgPool) {
    user(&pool, "alice").await;
    let api = login_api(pool);
    let statuses = wrong_logins(&api, "203.0.113.7", 40).await;
    assert_eq!(
        count(&statuses, 401),
        10,
        "only the allowed tries are checked: {statuses:?}"
    );
    assert_eq!(count(&statuses, 429), 30);
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_an_account_caps_failures_from_every_address(pool: PgPool) {
    user(&pool, "alice").await;
    let api = login_api(pool);
    let mut checked = 0;
    for i in 1..=11 {
        checked += count(&wrong_logins(&api, &format!("198.51.100.{i}"), 10).await, 401);
    }
    assert_eq!(checked, 100, "100 failures an hour, whatever the address");
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_crossed_friend_requests_make_friends(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    assert!(db::send_friend_request(&pool, alice, bob).await.is_ok());
    assert!(
        db::send_friend_request(&pool, bob, alice).await.is_ok(),
        "bob's request accepts alice's"
    );
    assert!(db::are_friends(&pool, alice, bob).await.unwrap());
    assert!(matches!(
        db::send_friend_request(&pool, bob, alice).await,
        Err(FriendshipError::AlreadyExists)
    ));
}
