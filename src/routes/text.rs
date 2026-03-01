use {
    crate::{
        auth::{AdminUser, AuthenticatedUser},
        error::{AppError, AppResult},
        models::{Comment, VideoMeta},
        routes::media::{
            self, ALLOWED_TEXT_TYPES, CommentBody, CommentsDisabledPatch, MediaResponse, NsfwPatch,
            RangeHeader,
        },
        state::AppState,
    },
    rocket::{
        Data, State, delete, get,
        http::{ContentType, Status},
        patch, post, put,
        serde::json::Json,
    },
    std::{io::Cursor, path::Path},
    syntect::{
        highlighting::{Theme, ThemeSet},
        html::highlighted_html_for_string,
        parsing::SyntaxSet,
    },
};

const ROSE_PINE: &[u8] = include_bytes!("../../synthemes/rose-pine.tmTheme");

fn load_theme(bytes: &'static [u8]) -> Theme {
    ThemeSet::load_from_reader(&mut Cursor::new(bytes)).expect("couldn't load theme")
}

#[get("/text")]
pub fn list_text(state: &State<AppState>) -> Json<Vec<VideoMeta>> {
    media::handle_list(state, "text/")
}

#[get("/text/<id>")]
pub fn get_text(id: &str, state: &State<AppState>) -> AppResult<Json<VideoMeta>> {
    media::handle_get(id, state)
}

#[get("/text/<id>/file")]
pub async fn stream_text(
    id: &str,
    state: &State<AppState>,
    range: RangeHeader,
) -> Result<MediaResponse, AppError> {
    media::stream_file(id, None, None, state, range, false).await
}

#[get("/text/<id>/highlighted")]
pub async fn highlighted_text(
    id: &str,
    state: &State<AppState>,
) -> Result<(ContentType, String), AppError> {
    let meta = state.videos.get(id).ok_or(AppError::VideoNotFound)?.clone();

    let filename = if let Some(ref ref_id) = meta.references_id {
        state
            .videos
            .get(ref_id)
            .map(|v| v.filename.clone())
            .unwrap_or_else(|| meta.filename.clone())
    } else {
        meta.filename.clone()
    };

    let file_path = Path::new(&state.upload_dir).join(&filename);
    let source = rocket::tokio::fs::read_to_string(&file_path)
        .await
        .map_err(AppError::Io)?;

    let ext = meta
        .original_extension
        .as_deref()
        .unwrap_or(".txt")
        .trim_start_matches('.');

    let ss = SyntaxSet::load_defaults_newlines();
    let theme = load_theme(ROSE_PINE);

    let syntax = ss
        .find_syntax_by_extension(ext)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let html = highlighted_html_for_string(&source, &ss, syntax, &theme)
        .unwrap_or_else(|_| format!("<pre>{}</pre>", html_escape(&source)));

    Ok((ContentType::HTML, html))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[allow(clippy::too_many_arguments)]
#[post(
    "/text/upload?<title>&<nsfw>&<unlisted>&<comments_disabled>&<filename>",
    data = "<data>"
)]
pub async fn upload_text(
    title: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    filename: Option<&str>,
    data: Data<'_>,
    content_type: &ContentType,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    media::handle_upload(
        title,
        nsfw,
        unlisted,
        comments_disabled,
        data,
        content_type,
        user,
        state,
        ALLOWED_TEXT_TYPES,
        filename,
    )
    .await
}

#[post("/text/upload/init?<content_type>")]
pub async fn init_upload(
    content_type: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_init_upload(content_type, user, state, ALLOWED_TEXT_TYPES).await
}

#[put("/text/upload/<upload_id>/<chunk_index>", data = "<data>")]
pub async fn upload_chunk(
    upload_id: &str,
    chunk_index: usize,
    data: Data<'_>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_upload_chunk(upload_id, chunk_index, data, user, state).await
}

#[allow(clippy::too_many_arguments)]
#[post(
    "/text/upload/<upload_id>/complete?<title>&<nsfw>&<unlisted>&<comments_disabled>&<filename>"
)]
pub async fn complete_upload(
    upload_id: &str,
    title: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    filename: Option<&str>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    media::handle_complete_upload(
        upload_id,
        title,
        nsfw,
        unlisted,
        comments_disabled,
        user,
        state,
        ALLOWED_TEXT_TYPES,
        filename,
    )
    .await
}

#[post("/text/upload/init?<_content_type>", rank = 2)]
pub async fn init_upload_unauthorized(
    _content_type: Option<&str>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[put("/text/upload/<_upload_id>/<_chunk_index>", data = "<_data>", rank = 2)]
pub async fn upload_chunk_unauthorized(
    _upload_id: &str,
    _chunk_index: usize,
    _data: Data<'_>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[post(
    "/text/upload/<_upload_id>/complete?<_title>&<_nsfw>&<_unlisted>&<_comments_disabled>&<_filename>",
    rank = 2
)]
pub async fn complete_upload_unauthorized(
    _upload_id: &str,
    _title: Option<&str>,
    _nsfw: Option<bool>,
    _unlisted: Option<bool>,
    _comments_disabled: Option<bool>,
    _filename: Option<&str>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[post(
    "/text/upload?<_title>&<_nsfw>&<_unlisted>&<_comments_disabled>&<_filename>",
    data = "<_data>",
    rank = 2
)]
pub async fn upload_text_unauthorized(
    _title: Option<&str>,
    _nsfw: Option<bool>,
    _unlisted: Option<bool>,
    _comments_disabled: Option<bool>,
    _filename: Option<&str>,
    _data: Data<'_>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required to upload" })),
    )
}

#[patch("/text/<id>/nsfw", format = "json", data = "<body>")]
pub fn patch_nsfw(
    id: &str,
    body: Json<NsfwPatch>,
    _admin: AdminUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_patch_nsfw(id, body, state)
}

#[patch("/text/<_id>/nsfw", format = "json", data = "<_body>", rank = 2)]
pub fn patch_nsfw_forbidden(
    _id: &str,
    _body: Json<NsfwPatch>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Forbidden,
        Json(serde_json::json!({ "error": "Admin privileges required" })),
    )
}

#[delete("/text/<id>")]
pub async fn delete_text(
    id: &str,
    _admin: AdminUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_delete(id, state).await
}

#[delete("/text/<_id>", rank = 2)]
pub fn delete_text_forbidden(
    _id: &str,
    _user: AuthenticatedUser,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Forbidden,
        Json(serde_json::json!({ "error": "Admin privileges required" })),
    )
}

#[delete("/text/<_id>", rank = 3)]
pub fn delete_text_unauthorized(_id: &str) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[get("/text/<id>/comments")]
pub fn get_comments(id: &str, state: &State<AppState>) -> Result<Json<Vec<Comment>>, AppError> {
    media::handle_get_comments(id, state)
}

#[post("/text/<id>/comments", format = "json", data = "<body>")]
pub fn add_comment(
    id: &str,
    body: Json<CommentBody>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<Comment>), AppError> {
    media::handle_add_comment(id, body, user, state)
}

#[post("/text/<_id>/comments", format = "json", data = "<_body>", rank = 2)]
pub fn add_comment_unauthorized(
    _id: &str,
    _body: Json<CommentBody>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[delete("/text/<id>/comments/<comment_id>")]
pub fn delete_comment(
    id: &str,
    comment_id: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_delete_comment(id, comment_id, user, state)
}

#[delete("/text/<_id>/comments/<_comment_id>", rank = 2)]
pub fn delete_comment_unauthorized(
    _id: &str,
    _comment_id: &str,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[patch("/text/<id>/comments_disabled", format = "json", data = "<body>")]
pub fn patch_comments_disabled(
    id: &str,
    body: Json<CommentsDisabledPatch>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_patch_comments_disabled(id, body, user, state)
}

#[patch(
    "/text/<_id>/comments_disabled",
    format = "json",
    data = "<_body>",
    rank = 2
)]
pub fn patch_comments_disabled_unauthorized(
    _id: &str,
    _body: Json<CommentsDisabledPatch>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}
