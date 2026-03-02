use {
    crate::{
        auth::{AdminUser, AuthenticatedUser},
        error::{AppError, AppResult},
        models::{Comment, VideoMeta},
        routes::media::{
            self, ALLOWED_IMAGE_TYPES, CommentBody, CommentsDisabledPatch, MediaResponse,
            NsfwPatch, RangeHeader,
        },
        state::AppState,
    },
    rocket::{
        Data, State, delete, get,
        http::{ContentType, Status},
        patch, post, put,
        serde::json::Json,
    },
};

#[get("/images")]
pub fn list_images(state: &State<AppState>) -> Json<Vec<VideoMeta>> {
    media::handle_list(state, "image/")
}

#[get("/images/<id>")]
pub fn get_image(id: &str, state: &State<AppState>) -> AppResult<Json<VideoMeta>> {
    media::handle_get(id, state)
}

#[get("/images/<id>/file")]
pub async fn stream_image(
    id: &str,
    state: &State<AppState>,
    range: RangeHeader,
) -> Result<MediaResponse, AppError> {
    media::stream_file(id, None, None, state, range, false).await
}

#[allow(clippy::too_many_arguments)]
#[post(
    "/images/upload?<title>&<source>&<nsfw>&<unlisted>&<comments_disabled>",
    data = "<data>"
)]
pub async fn upload_image(
    title: &str,
    source: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    data: Data<'_>,
    content_type: &ContentType,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    media::handle_upload(
        title,
        source,
        nsfw,
        unlisted,
        comments_disabled,
        data,
        content_type,
        user,
        state,
        ALLOWED_IMAGE_TYPES,
        None,
    )
    .await
}

#[post("/images/upload/init?<content_type>")]
pub async fn init_upload(
    content_type: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_init_upload(content_type, user, state, ALLOWED_IMAGE_TYPES).await
}

#[put("/images/upload/<upload_id>/<chunk_index>", data = "<data>")]
pub async fn upload_chunk(
    upload_id: &str,
    chunk_index: usize,
    data: Data<'_>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_upload_chunk(upload_id, chunk_index, data, user, state).await
}

#[post(
    "/images/upload/<upload_id>/complete?<title>&<source>&<nsfw>&<unlisted>&<comments_disabled>"
)]
pub async fn complete_upload(
    upload_id: &str,
    title: &str,
    source: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    media::handle_complete_upload(
        upload_id,
        title,
        source,
        nsfw,
        unlisted,
        comments_disabled,
        user,
        state,
        ALLOWED_IMAGE_TYPES,
        None,
    )
    .await
}

#[post("/images/upload/init?<_content_type>", rank = 2)]
pub async fn init_upload_unauthorized(
    _content_type: Option<&str>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[put(
    "/images/upload/<_upload_id>/<_chunk_index>",
    data = "<_data>",
    rank = 2
)]
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
    "/images/upload/<_upload_id>/complete?<_title>&<_nsfw>&<_unlisted>&<_comments_disabled>",
    rank = 2
)]
pub async fn complete_upload_unauthorized(
    _upload_id: &str,
    _title: Option<&str>,
    _nsfw: Option<bool>,
    _unlisted: Option<bool>,
    _comments_disabled: Option<bool>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[post(
    "/images/upload?<_title>&<_nsfw>&<_unlisted>&<_comments_disabled>",
    data = "<_data>",
    rank = 2
)]
pub async fn upload_image_unauthorized(
    _title: Option<&str>,
    _nsfw: Option<bool>,
    _unlisted: Option<bool>,
    _comments_disabled: Option<bool>,
    _data: Data<'_>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required to upload" })),
    )
}

#[patch("/images/<id>/nsfw", format = "json", data = "<body>")]
pub fn patch_nsfw(
    id: &str,
    body: Json<NsfwPatch>,
    _admin: AdminUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_patch_nsfw(id, body, state)
}

#[patch("/images/<_id>/nsfw", format = "json", data = "<_body>", rank = 2)]
pub fn patch_nsfw_forbidden(
    _id: &str,
    _body: Json<NsfwPatch>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Forbidden,
        Json(serde_json::json!({ "error": "Admin privileges required" })),
    )
}

#[delete("/images/<id>")]
pub async fn delete_image(
    id: &str,
    _admin: AdminUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_delete(id, state).await
}

#[delete("/images/<_id>", rank = 2)]
pub fn delete_image_forbidden(
    _id: &str,
    _user: AuthenticatedUser,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Forbidden,
        Json(serde_json::json!({ "error": "Admin privileges required" })),
    )
}

#[delete("/images/<_id>", rank = 3)]
pub fn delete_image_unauthorized(_id: &str) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[get("/images/<id>/comments")]
pub fn get_comments(id: &str, state: &State<AppState>) -> Result<Json<Vec<Comment>>, AppError> {
    media::handle_get_comments(id, state)
}

#[post("/images/<id>/comments", format = "json", data = "<body>")]
pub fn add_comment(
    id: &str,
    body: Json<CommentBody>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<Comment>), AppError> {
    media::handle_add_comment(id, body, user, state)
}

#[post("/images/<_id>/comments", format = "json", data = "<_body>", rank = 2)]
pub fn add_comment_unauthorized(
    _id: &str,
    _body: Json<CommentBody>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[delete("/images/<id>/comments/<comment_id>")]
pub fn delete_comment(
    id: &str,
    comment_id: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_delete_comment(id, comment_id, user, state)
}

#[delete("/images/<_id>/comments/<_comment_id>", rank = 2)]
pub fn delete_comment_unauthorized(
    _id: &str,
    _comment_id: &str,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[patch("/images/<id>/comments_disabled", format = "json", data = "<body>")]
pub fn patch_comments_disabled(
    id: &str,
    body: Json<CommentsDisabledPatch>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    media::handle_patch_comments_disabled(id, body, user, state)
}

#[patch(
    "/images/<_id>/comments_disabled",
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
