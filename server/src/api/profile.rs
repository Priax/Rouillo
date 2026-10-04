use super::*;

#[derive(Deserialize)]
pub(super) struct PatchMeBody {
    bio: Option<String>,
    favorite_music: Option<String>,
}

pub(super) async fn upload(
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

pub(super) async fn handle_avatar_upload(
    State(api): State<Api>,
    Authed(user): Authed,
    body: Result<Bytes, BytesRejection>,
) -> Result<impl IntoResponse, ApiError> {
    upload(api, user, Kind::Avatar, body).await
}

pub(super) async fn handle_banner_upload(
    State(api): State<Api>,
    Authed(user): Authed,
    body: Result<Bytes, BytesRejection>,
) -> Result<impl IntoResponse, ApiError> {
    upload(api, user, Kind::Banner, body).await
}

pub(super) async fn remove_image(api: &Api, user: &db::User, kind: Kind) -> Result<Json<UserProfile>, ApiError> {
    let user = db::delete_image(&api.pool, user.id, kind).await.map_err(internal)?;
    Ok(Json(UserProfile::from(user)))
}

pub(super) async fn handle_avatar_delete(
    State(api): State<Api>,
    Authed(user): Authed,
) -> Result<impl IntoResponse, ApiError> {
    remove_image(&api, &user, Kind::Avatar).await
}

pub(super) async fn handle_banner_delete(
    State(api): State<Api>,
    Authed(user): Authed,
) -> Result<impl IntoResponse, ApiError> {
    remove_image(&api, &user, Kind::Banner).await
}

pub(super) async fn serve_image(api: &Api, user_id: Uuid, kind: Kind) -> Result<Response, ApiError> {
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

pub(super) async fn handle_avatar_get(
    State(api): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    serve_image(&api, user_id, Kind::Avatar).await
}

pub(super) async fn handle_banner_get(
    State(api): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    serve_image(&api, user_id, Kind::Banner).await
}

pub(super) async fn handle_me(Authed(user): Authed) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(UserProfile::from(user)))
}

/// Profile text without control characters: the bio keeps its line breaks,
/// the favorite music is one line.
pub(super) fn printable(text: &str, lines: bool) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\n' if lines => '\n',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .filter(|&c| c == '\n' || !c.is_control())
        .collect()
}

pub(super) async fn handle_patch_me(
    State(Api { pool, .. }): State<Api>,
    Authed(user): Authed,
    JsonBody(mut body): JsonBody<PatchMeBody>,
) -> Result<impl IntoResponse, ApiError> {
    body.bio = body.bio.map(|s| printable(&s, true));
    body.favorite_music = body.favorite_music.map(|s| printable(&s, false));
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
pub(super) struct PlayerMatchInfo {
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
pub(super) struct MatchEntry {
    id: Uuid,
    played_at: DateTime<Utc>,
    duration_secs: f64,
    winner_slot: Option<i16>,
    ranked: bool,
    player1: PlayerMatchInfo,
    player2: PlayerMatchInfo,
}

#[derive(Deserialize)]
pub(super) struct MatchHistoryQuery {
    limit: Option<String>,
    offset: Option<String>,
}

pub(super) fn int_param(value: Option<&str>, default: i64) -> Result<i64, ApiError> {
    value.map_or(Ok(default), |s| {
        s.parse::<i64>().ok().filter(|&n| n >= 0).ok_or(ApiError::BadQuery)
    })
}

pub(super) async fn handle_user_profile(
    State(Api { pool, .. }): State<Api>,
    PathParam(user_id): PathParam<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let row = db::get_user_profile(&pool, user_id)
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(row))
}

pub(super) async fn handle_match_history(
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
pub(super) struct SoloBestBody {
    score: i32,
}

/// Records a solo score: the account keeps its best, which it returns.
pub(super) async fn handle_solo_best(
    State(Api { pool, .. }): State<Api>,
    Authed(user): Authed,
    JsonBody(body): JsonBody<SoloBestBody>,
) -> Result<impl IntoResponse, ApiError> {
    let best = db::raise_solo_best(&pool, user.id, body.score)
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "solo_best": best })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_text_loses_its_control_characters() {
        assert_eq!(printable("a\r\nb\tc\u{7}d\u{1b}[2J", true), "a\nb cd[2J");
        assert_eq!(printable("one\ntwo", false), "one two");
    }
}
