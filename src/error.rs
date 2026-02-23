use {
    rocket::{
        Request,
        http::Status,
        response::{self, Responder},
        serde::json::Json,
    },
    serde::Serialize,
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Not authenticated — please log in via /auth/login")]
    NotAuthenticated,

    #[error("Forbidden — admin privileges required")]
    Forbidden,

    #[error("OAuth state mismatch (possible CSRF attack)")]
    OAuthStateMismatch,

    #[error("OAuth token exchange failed: {0}")]
    OAuthTokenExchange(String),

    #[error("Failed to fetch osu! user info: {0}")]
    OsuUserFetch(String),

    #[error("Upload exceeds 100 MB limit")]
    FileTooLarge,

    #[error("Duplicate video — identical content already exists as video '{0}'")]
    DuplicateVideo(String),

    #[error("Invalid file type — only video files are accepted")]
    InvalidFileType,

    #[error("Video not found")]
    VideoNotFound,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn status(&self) -> Status {
        match self {
            AppError::NotAuthenticated => Status::Unauthorized,
            AppError::Forbidden => Status::Forbidden,
            AppError::OAuthStateMismatch => Status::BadRequest,
            AppError::VideoNotFound => Status::NotFound,
            AppError::FileTooLarge => Status::PayloadTooLarge,
            AppError::DuplicateVideo(_) => Status::Conflict,
            AppError::InvalidFileType => Status::UnsupportedMediaType,
            _ => Status::InternalServerError,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    message: &'a str,
}

impl<'r> Responder<'r, 'static> for AppError {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        let status = self.status();
        let body = serde_json::json!({
            "error": status.reason().unwrap_or("error"),
            "message": self.to_string(),
        });
        rocket::response::status::Custom(status, Json(body)).respond_to(req)
    }
}

pub type AppResult<T> = Result<T, AppError>;
