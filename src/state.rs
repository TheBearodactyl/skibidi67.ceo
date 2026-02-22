use {
    crate::models::{Session, VideoMeta},
    dashmap::DashMap,
    std::collections::HashSet,
};

#[derive(Debug, Clone)]
pub struct OsuOAuthConfig {
    pub client_id: u64,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl OsuOAuthConfig {
    pub fn from_env() -> color_eyre::Result<Self> {
        use color_eyre::eyre::WrapErr;
        Ok(Self {
            client_id: std::env::var("OSU_CLIENT_ID")
                .wrap_err("OSU_CLIENT_ID not set")?
                .parse()
                .wrap_err("OSU_CLIENT_ID must be a number")?,
            client_secret: std::env::var("OSU_CLIENT_SECRET")
                .wrap_err("OSU_CLIENT_SECRET not set")?,
            redirect_uri: std::env::var("OSU_REDIRECT_URI").wrap_err("OSU_REDIRECT_URI not set")?,
        })
    }
}

pub struct AppState {
    pub oauth: OsuOAuthConfig,
    pub pending_states: DashMap<String, ()>,
    pub sessions: DashMap<String, Session>,
    pub videos: DashMap<String, VideoMeta>,
    pub video_hashes: DashMap<String, String>,
    pub admin_ids: HashSet<u64>,
    pub upload_dir: String,
}

impl AppState {
    pub fn new(oauth: OsuOAuthConfig, admin_ids: HashSet<u64>, upload_dir: String) -> Self {
        Self {
            oauth,
            pending_states: DashMap::new(),
            sessions: DashMap::new(),
            videos: DashMap::new(),
            video_hashes: DashMap::new(),
            admin_ids,
            upload_dir,
        }
    }

    pub fn is_admin(&self, user_id: u64) -> bool {
        self.admin_ids.contains(&user_id)
    }
}
