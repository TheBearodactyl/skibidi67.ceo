use {
    chrono::{DateTime, Utc},
    serde::{Deserialize, Serialize},
};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
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

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GithubTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubUser {
    pub id: u64,
    pub login: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformUser {
    pub provider: String,
    pub id: u64,
    pub username: String,
    pub avatar_url: String,
}

impl PlatformUser {
    pub fn from_osu(u: &OsuUser) -> Self {
        Self {
            provider: "osu".to_owned(),
            id: u.id,
            username: u.username.clone(),
            avatar_url: u.avatar_url.clone(),
        }
    }

    pub fn from_github(u: &GithubUser) -> Self {
        Self {
            provider: "github".to_owned(),
            id: u.id,
            username: u.login.clone(),
            avatar_url: u.avatar_url.clone(),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    pub user: PlatformUser,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMeta {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub tlsh_hash: Option<String>,
    #[serde(default)]
    pub uploaded_by_provider: String,
    pub uploaded_by_id: u64,
    pub uploaded_by_name: String,
    pub uploaded_at: DateTime<Utc>,
    pub nsfw: bool,
    #[serde(default)]
    pub unlisted: bool,
    #[serde(default = "default_true")]
    pub comments_disabled: bool,
    pub references_id: Option<String>,
    #[serde(default)]
    pub original_extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub video_id: String,
    #[serde(default)]
    pub author_provider: String,
    pub author_id: u64,
    pub author_name: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}
