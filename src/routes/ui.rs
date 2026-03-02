use {
    crate::{auth::AuthenticatedUser, models::PlatformUser, state::AppState},
    rocket::{
        State, get,
        http::{ContentType, Status},
        response::Redirect,
        serde::json::Json,
    },
    rocket_dyn_templates::{Template, context},
    serde::{Deserialize, Serialize},
    std::hash::{Hash, Hasher},
};

pub struct SiteInfo {
    pub base_url: String,
    pub site_host: String,
}

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for SiteInfo {
    type Error = ();

    async fn from_request(
        req: &'r rocket::request::Request<'_>,
    ) -> rocket::request::Outcome<Self, Self::Error> {
        if let Ok(url) = std::env::var("BASE_URL") {
            let url = url.trim_end_matches('/').to_owned();
            let site_host = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(&url)
                .to_owned();
            return rocket::request::Outcome::Success(SiteInfo {
                base_url: url,
                site_host,
            });
        }

        let host = req.headers().get_one("Host").unwrap_or("localhost");
        let scheme = if host.starts_with("localhost") || host.starts_with("127.") {
            "http"
        } else {
            "https"
        };

        rocket::request::Outcome::Success(SiteInfo {
            base_url: format!("{}://{}", scheme, host),
            site_host: host.to_owned(),
        })
    }
}

#[derive(Serialize)]
struct CommentCtx {
    id: String,
    author_provider: String,
    author_id: u64,
    author_name: String,
    text: String,
    created_at: String,
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    for unit in UNITS {
        if size < 1024.0 {
            return if size.fract() < 0.05 {
                format!("{:.0} {}", size, unit)
            } else {
                format!("{:.1} {}", size, unit)
            };
        }
        size /= 1024.0;
    }
    format!("{:.1} PB", size)
}

#[derive(Serialize)]
struct UserCtx {
    id: u64,
    username: String,
    avatar_url: String,
    provider: String,
}

impl UserCtx {
    fn from_platform(u: &PlatformUser) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            avatar_url: u.avatar_url.clone(),
            provider: u.provider.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
struct VideoCtx {
    id: String,
    title: String,
    source: String,
    content_type: String,
    media_type: String,
    size_bytes: u64,
    size_human: String,
    sha256: String,
    tlsh_hash: Option<String>,
    uploaded_by_provider: String,
    uploaded_by_id: u64,
    uploaded_by_name: String,
    uploaded_at: chrono::DateTime<chrono::Utc>,
    uploaded_at_display: String,
    nsfw: bool,
    unlisted: bool,
    comments_disabled: bool,
    references_id: Option<String>,
    original_extension: Option<String>,
}

impl VideoCtx {
    fn from_meta(v: &crate::models::VideoMeta) -> Self {
        let media_type = if v.content_type.starts_with("audio/") {
            "audio"
        } else if v.content_type.starts_with("image/") {
            "image"
        } else if v.content_type.starts_with("text/") {
            "text"
        } else {
            "video"
        }
        .to_owned();

        let source = if let Some(s) = v.source.clone() {
            s.clone()
        } else {
            "N/A".to_string()
        };

        Self {
            id: v.id.clone(),
            title: v.title.clone(),
            source,
            content_type: v.content_type.clone(),
            media_type,
            size_bytes: v.size_bytes,
            size_human: format_size(v.size_bytes),
            sha256: v.sha256.clone(),
            tlsh_hash: v.tlsh_hash.clone(),
            uploaded_by_provider: v.uploaded_by_provider.clone(),
            uploaded_by_id: v.uploaded_by_id,
            uploaded_by_name: v.uploaded_by_name.clone(),
            uploaded_at: v.uploaded_at,
            uploaded_at_display: v.uploaded_at.format("%Y-%m-%d %H:%M UTC").to_string(),
            nsfw: v.nsfw,
            unlisted: v.unlisted,
            comments_disabled: v.comments_disabled,
            references_id: v.references_id.clone(),
            original_extension: v.original_extension.clone(),
        }
    }
}

fn media_url_prefix(media_type: &str) -> &'static str {
    match media_type {
        "audio" => "audio",
        "image" => "images",
        "text" => "text",
        _ => "videos",
    }
}

#[get("/favicon.ico")]
pub fn favicon() -> (ContentType, &'static [u8]) {
    (
        ContentType::Icon,
        include_bytes!("../../static/favicon.ico"),
    )
}

#[get("/")]
pub fn index() -> Redirect {
    Redirect::to("/ui")
}

#[get("/ui")]
pub fn listing(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let show_nsfw_on_homepage: bool = std::env::var("SHOW_NSFW_ON_HOMEPAGE")
        .unwrap_or_default()
        .parse()
        .unwrap_or(false);
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let mut videos: Vec<VideoCtx> = Vec::new();
    let mut audio: Vec<VideoCtx> = Vec::new();
    let mut images: Vec<VideoCtx> = Vec::new();
    let mut texts: Vec<VideoCtx> = Vec::new();

    for entry in state.videos.iter() {
        let v = entry.value();
        if v.unlisted {
            continue;
        }
        let ctx = VideoCtx::from_meta(v);
        match ctx.media_type.as_str() {
            "video" => videos.push(ctx),
            "audio" => audio.push(ctx),
            "image" => images.push(ctx),
            "text" => texts.push(ctx),
            _ => {}
        }
    }

    videos.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    audio.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    images.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    texts.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));

    let all: Vec<&VideoCtx> = if show_nsfw_on_homepage {
        videos
            .iter()
            .chain(audio.iter())
            .chain(images.iter())
            .chain(texts.iter())
            .collect()
    } else {
        videos
            .iter()
            .chain(audio.iter())
            .chain(images.iter())
            .chain(texts.iter())
            .filter(|v| !v.nsfw)
            .collect()
    };

    let featured: Option<VideoCtx> = {
        let queue_pick = {
            let queue = state.daily_pick_queue.read().unwrap();
            queue.iter().find_map(|id| {
                state
                    .videos
                    .get(id.as_str())
                    .map(|v| VideoCtx::from_meta(v.value()))
            })
        };
        if queue_pick.is_some() {
            queue_pick
        } else {
            let pool: Vec<&VideoCtx> = if all.is_empty() {
                videos
                    .iter()
                    .chain(audio.iter())
                    .chain(images.iter())
                    .chain(texts.iter())
                    .collect()
            } else {
                all
            };
            if pool.is_empty() {
                None
            } else {
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                today.hash(&mut hasher);
                let idx = hasher.finish() as usize % pool.len();
                Some(pool[idx].clone())
            }
        }
    };

    let latest_videos: Vec<VideoCtx> = videos.into_iter().take(5).collect();
    let latest_audio: Vec<VideoCtx> = audio.into_iter().take(5).collect();
    let latest_images: Vec<VideoCtx> = images.into_iter().take(5).collect();
    let latest_texts: Vec<VideoCtx> = texts.into_iter().take(5).collect();

    Template::render(
        "listing",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            base_url: site.base_url,
            latest_videos,
            latest_audio,
            latest_images,
            latest_texts,
            featured,
        },
    )
}

#[get("/ui/videos")]
pub fn video_listing(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let mut videos: Vec<VideoCtx> = state
        .videos
        .iter()
        .filter(|e| !e.value().unlisted && e.value().content_type.starts_with("video/"))
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    videos.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at));

    Template::render(
        "videos",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            videos,
        },
    )
}

#[get("/ui/audio")]
pub fn audio_listing(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let mut items: Vec<VideoCtx> = state
        .videos
        .iter()
        .filter(|e| !e.value().unlisted && e.value().content_type.starts_with("audio/"))
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    items.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at));

    Template::render(
        "audio_listing",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            items,
        },
    )
}

#[get("/ui/images")]
pub fn image_listing(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let mut items: Vec<VideoCtx> = state
        .videos
        .iter()
        .filter(|e| !e.value().unlisted && e.value().content_type.starts_with("image/"))
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    items.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at));

    Template::render(
        "image_listing",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            items,
        },
    )
}

fn render_media_player(
    id: &str,
    template_name: &'static str,
    site: SiteInfo,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();
    let site_host = site.site_host;
    let base_url = site.base_url;
    let video = state.videos.get(id).map(|v| VideoCtx::from_meta(v.value()));

    if video.is_none() {
        return Template::render(
            "message",
            context! {
                user: platform_user.map(UserCtx::from_platform),
                is_admin,
                has_github_oauth,
                site_host: site_host.clone(),
                title: "Not Found",
                message: "This media does not exist or has been deleted.",
            },
        );
    }

    let video = video.unwrap();

    if video.nsfw && platform_user.is_none() {
        return Template::render(
            "message",
            context! {
                user: Option::<UserCtx>::None,
                is_admin,
                has_github_oauth,
                site_host: site_host.clone(),
                title: "Login Required",
                message: "You must be logged in to view NSFW content.",
            },
        );
    }

    let api_prefix = media_url_prefix(&video.media_type);
    let file_url = format!("{}/{}/{}/file", base_url, api_prefix, id);
    let embed_url = format!("{}/e/{}", base_url, id);

    let comments: Vec<CommentCtx> = state
        .comments
        .get(id)
        .map(|c| {
            c.value()
                .iter()
                .map(|c| CommentCtx {
                    id: c.id.clone(),
                    author_provider: c.author_provider.clone(),
                    author_id: c.author_id,
                    author_name: c.author_name.clone(),
                    text: c.text.clone(),
                    created_at: c.created_at.format("%Y-%m-%d %H:%M UTC").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Template::render(
        template_name,
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host,
            base_url,
            video,
            file_url,
            embed_url,
            api_prefix,
            comments,
        },
    )
}

#[get("/ui/videos/<id>")]
pub fn player(
    id: &str,
    site: SiteInfo,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    render_media_player(id, "player", site, user, state)
}

#[get("/ui/audio/<id>")]
pub fn audio_player(
    id: &str,
    site: SiteInfo,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    render_media_player(id, "audio_player", site, user, state)
}

#[get("/ui/images/<id>")]
pub fn image_viewer(
    id: &str,
    site: SiteInfo,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    render_media_player(id, "image_viewer", site, user, state)
}

#[get("/ui/text")]
pub fn text_listing(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let mut items: Vec<VideoCtx> = state
        .videos
        .iter()
        .filter(|e| !e.value().unlisted && e.value().content_type.starts_with("text/"))
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    items.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at));

    Template::render(
        "text_listing",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            items,
        },
    )
}

#[get("/ui/text/<id>")]
pub fn text_viewer(
    id: &str,
    site: SiteInfo,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    render_media_player(id, "text_viewer", site, user, state)
}

#[get("/e/<id>?<start>&<end>")]
pub fn embed(
    id: &str,
    start: Option<u64>,
    end: Option<u64>,
    site: SiteInfo,
    state: &State<AppState>,
) -> Template {
    let video = state.videos.get(id).map(|v| VideoCtx::from_meta(v.value()));

    if video.is_none() {
        return Template::render(
            "message",
            context! {
                title: "Not Found",
                message: "This media does not exist or has been deleted.",
            },
        );
    }

    let video = video.unwrap();
    let api_prefix = media_url_prefix(&video.media_type);

    let file_url = {
        let base_file = format!("{}/{}/{}/file", site.base_url, api_prefix, id);
        match (start, end) {
            (None, None) => base_file,
            (s, e) => {
                let mut parts = vec![];
                if let Some(s) = s {
                    parts.push(format!("start={}", s));
                }
                if let Some(e) = e {
                    parts.push(format!("end={}", e));
                }
                format!("{}?{}", base_file, parts.join("&"))
            }
        }
    };

    Template::render(
        "embed",
        context! {
            site_host: site.site_host,
            base_url: site.base_url,
            video,
            file_url,
        },
    )
}

#[get("/ui/upload")]
pub fn upload_form(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    Template::render(
        "upload",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
        },
    )
}

#[get("/ui/admin")]
pub fn admin_panel(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let mut videos: Vec<VideoCtx> = state
        .videos
        .iter()
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    videos.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at));

    let disk_human: String = {
        let total_bytes: u64 = state
            .videos
            .iter()
            .filter(|e| e.value().references_id.is_none())
            .map(|e| e.value().size_bytes)
            .sum();
        format_size(total_bytes)
    };

    let daily_queue: Vec<VideoCtx> = {
        let queue = state.daily_pick_queue.read().unwrap();
        queue
            .iter()
            .filter_map(|id| {
                state
                    .videos
                    .get(id.as_str())
                    .map(|v| VideoCtx::from_meta(v.value()))
            })
            .collect()
    };

    Template::render(
        "admin",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            videos,
            video_count: state.videos.len(),
            disk_human,
            daily_queue,
        },
    )
}

async fn ui_delete_impl(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let (title, message) = if !is_admin {
        ("Error".to_owned(), "Admin access required.".to_owned())
    } else {
        match state.videos.remove(id) {
            None => ("Error".to_owned(), "Media not found.".to_owned()),
            Some((_, meta)) => {
                state.delete_video_meta(&meta.id);
                state.delete_comments(&meta.id);
                if meta.references_id.is_none() {
                    let still_referenced = state
                        .videos
                        .iter()
                        .any(|e| e.value().references_id.as_deref() == Some(&meta.id));
                    if !still_referenced {
                        state.video_hashes.remove(&meta.sha256);
                        state.video_tlsh.remove(&meta.id);
                        let path = std::path::Path::new(&state.upload_dir).join(&meta.filename);
                        let _ = rocket::tokio::fs::remove_file(path).await;
                    }
                }
                (
                    "Deleted".to_owned(),
                    format!("'{}' was deleted.", meta.title),
                )
            }
        }
    };

    Template::render(
        "message",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            site_host: site.site_host,
            title,
            message,
        },
    )
}

#[rocket::post("/ui/videos/<id>/delete")]
pub async fn ui_delete_video(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    ui_delete_impl(id, user, state, site).await
}

#[rocket::post("/ui/audio/<id>/delete")]
pub async fn ui_delete_audio(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    ui_delete_impl(id, user, state, site).await
}

#[rocket::post("/ui/images/<id>/delete")]
pub async fn ui_delete_image(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    ui_delete_impl(id, user, state, site).await
}

#[rocket::post("/ui/text/<id>/delete")]
pub async fn ui_delete_text(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    site: SiteInfo,
) -> Template {
    ui_delete_impl(id, user, state, site).await
}

#[derive(Deserialize)]
pub struct DailyQueueBody {
    pub media_id: String,
}

#[rocket::post("/ui/admin/daily-queue", data = "<body>")]
pub fn add_to_daily_queue(
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
    body: Json<DailyQueueBody>,
) -> Status {
    let platform_user = user.as_ref().map(|u| &u.0);
    if !platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id)) {
        return Status::Forbidden;
    }
    let mut queue = state.daily_pick_queue.write().unwrap();
    if !queue.contains(&body.media_id) {
        queue.push(body.media_id.clone());
    }
    drop(queue);
    state.persist_daily_queue();
    Status::Ok
}

#[rocket::delete("/ui/admin/daily-queue/<id>")]
pub fn remove_from_daily_queue(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Status {
    let platform_user = user.as_ref().map(|u| &u.0);
    if !platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id)) {
        return Status::Forbidden;
    }
    let mut queue = state.daily_pick_queue.write().unwrap();
    queue.retain(|item| item != id);
    drop(queue);
    state.persist_daily_queue();
    Status::Ok
}
