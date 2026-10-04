use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApiError {
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
