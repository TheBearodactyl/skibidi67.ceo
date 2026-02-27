use {
    crate::{auth::AuthenticatedUser, models::PlatformUser, state::AppState},
    rand::seq::IndexedRandom,
    rocket::{State, get, http::ContentType, response::Redirect},
    rocket_dyn_templates::{Template, context},
    serde::Serialize,
};

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
}

impl VideoCtx {
    fn from_meta(v: &crate::models::VideoMeta) -> Self {
        let media_type = if v.content_type.starts_with("audio/") {
            "audio"
        } else if v.content_type.starts_with("image/") {
            "image"
        } else {
            "video"
        }
        .to_owned();

        Self {
            id: v.id.clone(),
            title: v.title.clone(),
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
        }
    }
}

fn media_url_prefix(media_type: &str) -> &'static str {
    match media_type {
        "audio" => "audio",
        "image" => "images",
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
pub fn listing(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    let all: Vec<VideoCtx> = state
        .videos
        .iter()
        .filter(|e| !e.value().unlisted)
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();

    let mut latest_videos: Vec<VideoCtx> = all
        .iter()
        .filter(|v| v.media_type == "video")
        .cloned()
        .collect();
    latest_videos.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    latest_videos.truncate(5);

    let mut latest_audio: Vec<VideoCtx> = all
        .iter()
        .filter(|v| v.media_type == "audio")
        .cloned()
        .collect();
    latest_audio.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    latest_audio.truncate(5);

    let mut latest_images: Vec<VideoCtx> = all
        .iter()
        .filter(|v| v.media_type == "image")
        .cloned()
        .collect();
    latest_images.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    latest_images.truncate(5);

    let featured: Option<VideoCtx> = if all.is_empty() {
        None
    } else {
        let sfw: Vec<_> = all.iter().filter(|v| !v.nsfw).collect();
        if sfw.is_empty() {
            if all.is_empty() {
                None
            } else {
                all.choose(&mut rand::rng()).cloned()
            }
        } else {
            sfw.choose(&mut rand::rng()).map(|v| (*v).clone())
        }
    };

    Template::render(
        "listing",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            latest_videos,
            latest_audio,
            latest_images,
            featured,
        },
    )
}

#[get("/ui/videos")]
pub fn video_listing(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
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
            videos,
        },
    )
}

#[get("/ui/audio")]
pub fn audio_listing(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
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
            items,
        },
    )
}

#[get("/ui/images")]
pub fn image_listing(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
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
            items,
        },
    )
}

fn render_media_player(
    id: &str,
    template_name: &'static str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();
    let video = state.videos.get(id).map(|v| VideoCtx::from_meta(v.value()));

    if video.is_none() {
        return Template::render(
            "message",
            context! {
                user: platform_user.map(UserCtx::from_platform),
                is_admin,
                has_github_oauth,
                title: "Not Found",
                message: "This media does not exist or has been deleted.",
            },
        );
    }

    if video.as_ref().unwrap().nsfw && platform_user.is_none() {
        return Template::render(
            "message",
            context! {
                user: Option::<UserCtx>::None,
                is_admin,
                has_github_oauth,
                title: "Login Required",
                message: "You must be logged in to view NSFW content.",
            },
        );
    }

    let video_ref = video.as_ref().unwrap();
    let api_prefix = media_url_prefix(&video_ref.media_type);
    let file_url = format!("https://skibidi67.ceo/{}/{}/file", api_prefix, id);
    let embed_url = format!("https://skibidi67.ceo/e/{}", id);

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
            video,
            file_url,
            embed_url,
            api_prefix,
            comments,
        },
    )
}

#[get("/ui/videos/<id>")]
pub fn player(id: &str, user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    render_media_player(id, "player", user, state)
}

#[get("/ui/audio/<id>")]
pub fn audio_player(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    render_media_player(id, "audio_player", user, state)
}

#[get("/ui/images/<id>")]
pub fn image_viewer(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    render_media_player(id, "image_viewer", user, state)
}

#[get("/e/<id>?<start>&<end>")]
pub fn embed(id: &str, start: Option<u64>, end: Option<u64>, state: &State<AppState>) -> Template {
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

    let video_ref = video.as_ref().unwrap();
    let api_prefix = media_url_prefix(&video_ref.media_type);

    let file_url = {
        let base = format!("https://skibidi67.ceo/{}/{}/file", api_prefix, id);
        match (start, end) {
            (None, None) => base,
            (s, e) => {
                let mut parts = vec![];
                if let Some(s) = s {
                    parts.push(format!("start={}", s));
                }
                if let Some(e) = e {
                    parts.push(format!("end={}", e));
                }
                format!("{}?{}", base, parts.join("&"))
            }
        }
    };

    Template::render(
        "embed",
        context! {
            video,
            file_url,
        },
    )
}

#[get("/ui/upload")]
pub fn upload_form(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    let platform_user = user.as_ref().map(|u| &u.0);
    let is_admin = platform_user.is_some_and(|u| state.is_admin(&u.provider, u.id));
    let has_github_oauth = state.github_oauth.is_some();

    Template::render(
        "upload",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
        },
    )
}

#[get("/ui/admin")]
pub fn admin_panel(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
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

    Template::render(
        "admin",
        context! {
            user: platform_user.map(UserCtx::from_platform),
            is_admin,
            has_github_oauth,
            videos,
            video_count: state.videos.len(),
            disk_human,
        },
    )
}

async fn ui_delete_impl(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
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
) -> Template {
    ui_delete_impl(id, user, state).await
}

#[rocket::post("/ui/audio/<id>/delete")]
pub async fn ui_delete_audio(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    ui_delete_impl(id, user, state).await
}

#[rocket::post("/ui/images/<id>/delete")]
pub async fn ui_delete_image(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    ui_delete_impl(id, user, state).await
}
