//! HTML UI routes — renders Tera templates from `templates/`.
//! No HTML is constructed in Rust; all markup lives in the template files.

use {
    crate::{auth::AuthenticatedUser, models::OsuUser, state::AppState},
    rocket::{State, get, response::Redirect},
    rocket_dyn_templates::{Template, context},
    serde::Serialize,
};

// ── View models ───────────────────────────────────────────────────────────────
// Tera needs plain serialisable types, not DashMap references.

#[derive(Serialize)]
struct UserCtx {
    id: u64,
    username: String,
    avatar_url: String,
}

impl UserCtx {
    fn from_osu(u: &OsuUser) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            avatar_url: u.avatar_url.clone(),
        }
    }
}

#[derive(Serialize)]
struct VideoCtx {
    id: String,
    title: String,
    content_type: String,
    size_bytes: u64,
    sha256: String,
    uploaded_by_name: String,
    uploaded_at: String,
    nsfw: bool,
    references_id: Option<String>,
}

impl VideoCtx {
    fn from_meta(v: &crate::models::VideoMeta) -> Self {
        Self {
            id: v.id.clone(),
            title: v.title.clone(),
            content_type: v.content_type.clone(),
            size_bytes: v.size_bytes,
            sha256: v.sha256.clone(),
            uploaded_by_name: v.uploaded_by_name.clone(),
            uploaded_at: v.uploaded_at.format("%Y-%m-%d %H:%M UTC").to_string(),
            nsfw: v.nsfw,
            references_id: v.references_id.clone(),
        }
    }
}

// ── Routes ────────────────────────────────────────────────────────────────────

#[get("/")]
pub fn index() -> Redirect {
    Redirect::to("/ui")
}

#[get("/ui")]
pub fn listing(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    let osu_user = user.as_ref().map(|u| &u.0);
    let is_admin = osu_user.is_some_and(|u| state.is_admin(u.id));

    let mut videos: Vec<VideoCtx> = state
        .videos
        .iter()
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    videos.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at.clone()));

    Template::render(
        "listing",
        context! {
            user: osu_user.map(UserCtx::from_osu),
            is_admin,
            videos,
        },
    )
}

#[get("/ui/videos/<id>")]
pub fn player(id: &str, user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    let osu_user = user.as_ref().map(|u| &u.0);
    let is_admin = osu_user.is_some_and(|u| state.is_admin(u.id));
    let video = state.videos.get(id).map(|v| VideoCtx::from_meta(v.value()));
    let video_url = format!("https://skibidi67.ceo/videos/{}/file", id);

    Template::render(
        "player",
        context! {
            user: osu_user.map(UserCtx::from_osu),
            is_admin,
            video,
            video_url,
        },
    )
}

#[get("/ui/upload")]
pub fn upload_form(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    let osu_user = user.as_ref().map(|u| &u.0);
    let is_admin = osu_user.is_some_and(|u| state.is_admin(u.id));

    Template::render(
        "upload",
        context! {
            user: osu_user.map(UserCtx::from_osu),
            is_admin,
        },
    )
}

#[get("/ui/admin")]
pub fn admin_panel(user: Option<AuthenticatedUser>, state: &State<AppState>) -> Template {
    let osu_user = user.as_ref().map(|u| &u.0);
    let is_admin = osu_user.is_some_and(|u| state.is_admin(u.id));

    let mut videos: Vec<VideoCtx> = state
        .videos
        .iter()
        .map(|e| VideoCtx::from_meta(e.value()))
        .collect();
    videos.sort_by_key(|v| std::cmp::Reverse(v.uploaded_at.clone()));

    let disk_kib: u64 = state
        .videos
        .iter()
        .filter(|e| e.value().references_id.is_none())
        .map(|e| e.value().size_bytes / 1024)
        .sum();

    Template::render(
        "admin",
        context! {
            user: osu_user.map(UserCtx::from_osu),
            is_admin,
            videos,
            video_count: state.videos.len(),
            disk_kib,
        },
    )
}

#[rocket::post("/ui/videos/<id>/delete")]
pub async fn ui_delete(
    id: &str,
    user: Option<AuthenticatedUser>,
    state: &State<AppState>,
) -> Template {
    let osu_user = user.as_ref().map(|u| &u.0);
    let is_admin = osu_user.is_some_and(|u| state.is_admin(u.id));

    let (title, message) = if !is_admin {
        ("Error".to_owned(), "Admin access required.".to_owned())
    } else {
        match state.videos.remove(id) {
            None => ("Error".to_owned(), "Video not found.".to_owned()),
            Some((_, meta)) => {
                if meta.references_id.is_none() {
                    let still_referenced = state
                        .videos
                        .iter()
                        .any(|e| e.value().references_id.as_deref() == Some(&meta.id));
                    if !still_referenced {
                        state.video_hashes.remove(&meta.sha256);
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
            user: osu_user.map(UserCtx::from_osu),
            is_admin,
            title,
            message,
        },
    )
}
