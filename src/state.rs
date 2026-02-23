use {
    crate::models::{Session, VideoMeta},
    dashmap::DashMap,
    std::{
        collections::{HashMap, HashSet},
        path::Path,
    },
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

#[derive(Debug, Clone)]
pub struct GithubOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl GithubOAuthConfig {
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("GITHUB_CLIENT_ID").ok()?;
        let client_secret = std::env::var("GITHUB_CLIENT_SECRET").ok()?;
        let redirect_uri = std::env::var("GITHUB_REDIRECT_URI").ok()?;
        Some(Self {
            client_id,
            client_secret,
            redirect_uri,
        })
    }
}

pub struct AppState {
    pub oauth: OsuOAuthConfig,
    pub github_oauth: Option<GithubOAuthConfig>,
    pub pending_states: DashMap<String, ()>,
    pub sessions: DashMap<String, Session>,
    pub videos: DashMap<String, VideoMeta>,
    pub video_hashes: DashMap<String, String>,
    pub video_tlsh: DashMap<String, String>,
    pub admin_ids: HashMap<String, HashSet<u64>>,
    pub upload_dir: String,
}

impl AppState {
    pub fn new(
        oauth: OsuOAuthConfig,
        github_oauth: Option<GithubOAuthConfig>,
        admin_ids: HashMap<String, HashSet<u64>>,
        upload_dir: String,
    ) -> Self {
        let videos: DashMap<String, VideoMeta> = DashMap::new();
        let video_hashes: DashMap<String, String> = DashMap::new();
        let video_tlsh: DashMap<String, String> = DashMap::new();

        if let Ok(entries) = std::fs::read_dir(&upload_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let stem = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s.to_owned(),
                    None => continue,
                };

                if !stem.ends_with(".meta") {
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<VideoMeta>(&json) {
                        Ok(meta) => {
                            if meta.references_id.is_none() {
                                video_hashes.insert(meta.sha256.clone(), meta.id.clone());
                                if let Some(ref tlsh_hex) = meta.tlsh_hash {
                                    video_tlsh.insert(meta.id.clone(), tlsh_hex.clone());
                                }
                            }
                            videos.insert(meta.id.clone(), meta);
                        }
                        Err(e) => eprintln!("Warning: could not parse {:?}: {}", path, e),
                    },
                    Err(e) => eprintln!("Warning: could not read {:?}: {}", path, e),
                }
            }
        }

        println!("Loaded {} video(s) from disk.", videos.len());

        Self {
            oauth,
            github_oauth,
            pending_states: DashMap::new(),
            sessions: DashMap::new(),
            videos,
            video_hashes,
            video_tlsh,
            admin_ids,
            upload_dir,
        }
    }

    pub fn is_admin(&self, provider: &str, user_id: u64) -> bool {
        self.admin_ids
            .get(provider)
            .is_some_and(|ids| ids.contains(&user_id))
    }

    pub fn find_similar_tlsh(&self, new_tlsh_hex: &str) -> Option<String> {
        use tlsh2::TlshDefault;
        let new_tlsh: TlshDefault = match new_tlsh_hex.parse() {
            Ok(t) => t,
            Err(_) => return None,
        };
        for entry in self.video_tlsh.iter() {
            if let Ok(existing) = entry.value().parse::<TlshDefault>() {
                let distance = existing.diff(&new_tlsh, true);
                if distance < 100 {
                    return Some(entry.key().clone());
                }
            }
        }
        None
    }

    pub fn persist_video(&self, meta: &VideoMeta) {
        let path = Path::new(&self.upload_dir).join(format!("{}.meta.json", meta.id));
        match serde_json::to_string_pretty(meta) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    eprintln!("Warning: could not write metadata to {:?}: {}", path, e);
                }
            }
            Err(e) => eprintln!(
                "Warning: could not serialize metadata for {}: {}",
                meta.id, e
            ),
        }
    }

    pub fn delete_video_meta(&self, video_id: &str) {
        let path = Path::new(&self.upload_dir).join(format!("{}.meta.json", video_id));
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("Warning: could not delete metadata {:?}: {}", path, e);
        }
    }
}
