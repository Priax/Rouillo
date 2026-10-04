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
    let profile = |elo, casual| db::RankedProfile {
        elo,
        casual,
        avatar_url: None,
    };
    assert_eq!(db::ranked_profile(&pool, alice).await.unwrap(), Some(profile(1000, 3)));
    assert_eq!(db::ranked_profile(&pool, carol).await.unwrap(), Some(profile(1000, 1)));
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

fn login_api(pool: PgPool) -> axum::Router {
    crate::api::routes(pool, tokio::sync::mpsc::channel(8).0)
}

async fn wrong_logins(api: &axum::Router, ip: &str, count: usize) -> Vec<u16> {
    wrong_logins_as(api, ip, "alice", count).await
}

async fn wrong_logins_as(api: &axum::Router, ip: &str, username: &str, count: usize) -> Vec<u16> {
    use tower::ServiceExt;
    let body = serde_json::json!({ "username": username, "password": "wrong-password" }).to_string();
    let peer = format!("{ip}:5000");
    let tries = (0..count).map(|_| {
        let req = crate::api::tests::post_json("/api/login", Some(&peer), None, body.clone());
        api.clone().oneshot(req)
    });
    futures_util::future::join_all(tries)
        .await
        .into_iter()
        .map(|r| r.unwrap().status().as_u16())
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
async fn db_usernames_are_unique_whatever_their_case(pool: PgPool) {
    user(&pool, "Alice").await;
    let taken = db::create_user(&pool, "aLICE", "password123").await;
    assert!(
        matches!(&taken, Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505")),
        "a second Alice was created"
    );
    let found = db::find_user_by_username(&pool, "ALICE").await.unwrap();
    assert_eq!(found.map(|u| u.username).as_deref(), Some("Alice"));
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_a_player_can_rename_with_their_password(pool: PgPool) {
    use tower::ServiceExt;
    let alice = user(&pool, "alice").await;
    user(&pool, "Bob").await;
    let token = db::create_session(&pool, alice).await.unwrap();
    let api = login_api(pool.clone());
    let rename = |name: &str, password: &str| {
        let body = serde_json::json!({ "username": name, "password": password }).to_string();
        let mut req = crate::api::tests::post_json("/api/me/username", None, None, body);
        req.headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        api.clone().oneshot(req)
    };
    let status = |r: axum::response::Response| r.status().as_u16();
    assert_eq!(status(rename("Alicia", "wrong-password").await.unwrap()), 403);
    assert_eq!(
        status(rename("bob", "password123").await.unwrap()),
        409,
        "Bob in another case"
    );
    assert_eq!(status(rename("a b", "password123").await.unwrap()), 400);
    assert_eq!(
        status(rename("Alice", "password123").await.unwrap()),
        200,
        "own name, new case"
    );
    assert_eq!(status(rename("Alicia", "password123").await.unwrap()), 200);
    let renamed = db::find_user_by_username(&pool, "alicia").await.unwrap().unwrap();
    assert_eq!((renamed.id, renamed.username.as_str()), (alice, "Alicia"));
    assert!(db::verify_password("password123", renamed.password_hash()));
}

fn noisy_png(w: u32, h: u32) -> Vec<u8> {
    let mut seed = 7u32;
    let img = image::RgbaImage::from_fn(w, h, |_, _| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let [a, b, c, _] = seed.to_le_bytes();
        image::Rgba([a, b, c, 255])
    });
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_avatars_are_uploaded_served_and_removed(pool: PgPool) {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let alice = user(&pool, "alice").await;
    let token = db::create_session(&pool, alice).await.unwrap();
    let api = login_api(pool.clone());
    let send = |method: &str, path: &str, body: Vec<u8>, authed: bool| {
        let mut req = Request::builder().method(method).uri(path);
        if authed {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        api.clone().oneshot(req.body(Body::from(body)).unwrap())
    };
    let json = |bytes: &[u8]| serde_json::from_slice::<serde_json::Value>(bytes).unwrap();
    let read = |r: axum::response::Response| async move {
        let status = r.status().as_u16();
        (status, axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap())
    };

    let picture = noisy_png(300, 300);
    assert!(picture.len() > 64 * 1024, "setup: bigger than the JSON limit");
    let (status, body) = read(send("PUT", "/api/me/avatar", picture, true).await.unwrap()).await;
    assert_eq!(status, 200, "{body:?}");
    let url = json(&body)["avatar_url"].as_str().unwrap().to_owned();
    assert!(url.starts_with(&format!("/api/users/{alice}/avatar?v=")), "{url}");

    let (status, body) = read(send("GET", &url, Vec::new(), false).await.unwrap()).await;
    assert_eq!(status, 200);
    assert_eq!(image::guess_format(&body).unwrap(), image::ImageFormat::Png);

    let (status, body) = read(send("PUT", "/api/me/avatar", b"nope".to_vec(), true).await.unwrap()).await;
    assert_eq!((status, json(&body)["code"].as_str()), (400, Some("bad_image")));
    let (status, body) = read(send("PUT", "/api/me/banner", vec![0; 6 << 20], true).await.unwrap()).await;
    assert_eq!((status, json(&body)["code"].as_str()), (413, Some("image_too_large")));
    let (status, _) = read(send("PUT", "/api/me/avatar", noisy_png(10, 10), false).await.unwrap()).await;
    assert_eq!(status, 401);

    let (status, body) = read(send("DELETE", "/api/me/avatar", Vec::new(), true).await.unwrap()).await;
    assert_eq!((status, json(&body)["avatar_url"].is_null()), (200, true));
    let (status, _) = read(send("GET", &url, Vec::new(), false).await.unwrap()).await;
    assert_eq!(status, 404);
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_case_variants_share_one_failure_budget(pool: PgPool) {
    user(&pool, "alice").await;
    let api = login_api(pool);
    let mut statuses = Vec::new();
    for name in ["alice", "ALICE", "Alice", "aLiCe"] {
        statuses.extend(wrong_logins_as(&api, "203.0.113.9", name, 5).await);
    }
    assert_eq!(count(&statuses, 401), 10, "{statuses:?}");
}

#[sqlx::test]
#[ignore = "needs Postgres: DATABASE_URL, run with --ignored"]
async fn db_the_leaderboard_ranks_ranked_players_by_elo(pool: PgPool) {
    let alice = user(&pool, "alice").await;
    let bob = user(&pool, "bob").await;
    let carol = user(&pool, "carol").await;
    let casual = user(&pool, "casual_only").await;
    db::record_series(&pool, series(alice, bob)).await.unwrap();
    db::record_series(&pool, series(alice, carol)).await.unwrap();
    db::record_match_result(&pool, game(false, Some(casual), Some(alice), 1))
        .await
        .unwrap();
    let rows = db::leaderboard(&pool, None).await.unwrap();
    let order: Vec<_> = rows.iter().map(|r| (r.rank, r.username.as_str())).collect();
    assert_eq!(order[0], (1, "alice"));
    assert_eq!(rows[0].series_won, 2);
    assert!(rows.iter().all(|r| r.user_id != casual), "never played ranked");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].rank, rows[2].rank, "bob and carol share a rank");
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
