use super::*;

#[derive(Deserialize)]
pub(super) struct SendFriendRequestBody {
    user_id: Uuid,
}

#[derive(Serialize)]
pub(super) struct FriendListResponse {
    friends: Vec<db::FriendEntry>,
    sent: Vec<db::FriendEntry>,
    received: Vec<db::FriendEntry>,
}

#[derive(Deserialize)]
pub(super) struct LeaderboardQuery {
    me: Option<Uuid>,
}

pub(super) async fn handle_leaderboard(
    State(Api { pool, .. }): State<Api>,
    Params(query): Params<LeaderboardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let rows = db::leaderboard(&pool, query.me).await.map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub(super) struct UserSearchQuery {
    q: Option<String>,
}

pub(super) async fn handle_search_users(
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

pub(super) async fn handle_list_friends(
    State(Api { pool, cmd_tx, .. }): State<Api>,
    Authed(me): Authed,
) -> Result<impl IntoResponse, ApiError> {
    let mut list = db::list_friends(&pool, me.id).await.map_err(internal)?;
    let users = list.friends.iter().map(|f| f.user_id).collect();
    let (reply, answer) = tokio::sync::oneshot::channel();
    let _ = cmd_tx.send(Command::Playing { users, reply }).await;
    let playing = tokio::time::timeout(Duration::from_secs(1), answer)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    for friend in &mut list.friends {
        friend.playing = playing.contains(&friend.user_id);
    }

    Ok(Json(FriendListResponse {
        friends: list.friends,
        sent: list.sent,
        received: list.received,
    }))
}

pub(super) async fn handle_send_friend_request(
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

pub(super) async fn handle_accept_friend(
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

pub(super) async fn handle_remove_friend(
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
