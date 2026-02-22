use {
    chrono::{DateTime, Utc},
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Clone, Deserialize)]
pub struct OsuTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsuUser {
    pub id: u64,
    pub username: String,
    pub avatar_url: String,
    pub is_restricted: bool,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub user: OsuUser,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoMeta {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub uploaded_by_id: u64,
    pub uploaded_by_name: String,
    pub uploaded_at: DateTime<Utc>,
    pub nsfw: bool,
    pub references_id: Option<String>,
}
