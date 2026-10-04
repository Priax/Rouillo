use super::*;

#[derive(Deserialize)]
pub(super) struct RegisterBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub(super) struct LoginBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub(super) struct ChangePasswordBody {
    current: String,
    new: String,
}

#[derive(Deserialize)]
pub(super) struct RenameBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub(super) struct DeleteAccountBody {
    password: String,
}

#[derive(Serialize)]
pub(super) struct AuthResponse {
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

pub(super) fn validate_password(p: &str) -> bool {
    let n = p.len();
    (8..=1024).contains(&n)
}

pub(super) async fn handle_register(
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

pub(super) async fn handle_login(
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
    if !limits.try_attempt(&pair, client.as_deref(), &account) {
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

    limits.succeeded(&pair, client.as_deref(), &account);

    let token = db::create_session(&pool, user.id).await.map_err(internal)?;

    Ok(Json(AuthResponse::new(user, token)))
}

pub(super) async fn handle_logout(
    State(Api { pool, .. }): State<Api>,
    Bearer(token): Bearer,
) -> Result<impl IntoResponse, ApiError> {
    db::delete_session(&pool, token).await.map_err(internal)?;
    Ok(done())
}

pub(super) async fn check_password(user: &db::User, password: String, checks: &PasswordChecks) -> Result<(), ApiError> {
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

pub(super) async fn handle_change_password(
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

pub(super) async fn handle_rename(
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

pub(super) async fn handle_logout_all(
    State(Api { pool, cmd_tx, .. }): State<Api>,
    Session(user, token): Session,
) -> Result<impl IntoResponse, ApiError> {
    db::delete_user_sessions(&pool, user.id).await.map_err(internal)?;
    revoke(&cmd_tx, user.id, token).await;
    Ok(done())
}

pub(super) async fn handle_delete_account(
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

pub(super) async fn revoke(cmd_tx: &mpsc::Sender<Command>, user_id: Uuid, keep: Uuid) {
    let _ = cmd_tx
        .send(Command::Revoke {
            user_id,
            keep: Some(keep),
        })
        .await;
}
