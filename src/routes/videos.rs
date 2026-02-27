use {
    crate::{
        auth::{AdminUser, AuthenticatedUser},
        error::{AppError, AppResult},
        models::{Comment, VideoMeta},
        state::AppState,
    },
    hex::ToHex,
    rocket::{
        Data, State,
        data::ToByteUnit,
        delete, get,
        http::{ContentType, Status},
        patch, post, put,
        serde::json::Json,
        tokio::{fs, task},
    },
    serde::Deserialize,
    sha2::{Digest, Sha256},
    std::{path::Path, process::Stdio},
    tlsh2::TlshDefaultBuilder,
    tokio::io::AsyncWriteExt,
    uuid::Uuid,
};

const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "video/mp4",
    "video/webm",
    "video/ogg",
    "video/quicktime",
    "video/x-matroska",
    "video/x-msvideo",
];

pub struct RangeHeader(pub Option<String>);

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for RangeHeader {
    type Error = ();

    async fn from_request(
        req: &'r rocket::request::Request<'_>,
    ) -> rocket::request::Outcome<Self, Self::Error> {
        let val = req.headers().get_one("Range").map(|s| s.to_owned());
        rocket::request::Outcome::Success(RangeHeader(val))
    }
}

pub struct VideoResponse {
    pub data: Vec<u8>,
    pub content_type: String,
    pub content_range: String,
    pub partial: bool,
}

impl<'r> rocket::response::Responder<'r, 'static> for VideoResponse {
    fn respond_to(
        self,
        _req: &'r rocket::request::Request<'_>,
    ) -> rocket::response::Result<'static> {
        let status = if self.partial {
            rocket::http::Status::PartialContent
        } else {
            rocket::http::Status::Ok
        };

        let mut builder = rocket::response::Response::build();
        builder
            .status(status)
            .raw_header("Content-Type", self.content_type)
            .raw_header("Content-Length", self.data.len().to_string())
            .raw_header("Accept-Ranges", "bytes");

        if !self.content_range.is_empty() {
            builder.raw_header("Content-Range", self.content_range);
        }

        builder
            .sized_body(self.data.len(), std::io::Cursor::new(self.data))
            .ok()
    }
}

#[get("/videos")]
pub fn list_videos(state: &State<AppState>) -> Json<Vec<VideoMeta>> {
    let mut videos: Vec<VideoMeta> = state
        .videos
        .iter()
        .filter(|entry| !entry.value().unlisted)
        .map(|entry| entry.value().clone())
        .collect();

    videos.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    Json(videos)
}

#[get("/videos/<id>")]
pub fn get_video(id: &str, state: &State<AppState>) -> AppResult<Json<VideoMeta>> {
    state
        .videos
        .get(id)
        .map(|v| Json(v.clone()))
        .ok_or(AppError::VideoNotFound)
}

#[get("/videos/<id>/file?<start>&<end>")]
pub async fn stream_video(
    id: &str,
    start: Option<u64>,
    end: Option<u64>,
    state: &State<AppState>,
    range: RangeHeader,
) -> Result<VideoResponse, AppError> {
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

    if start.is_some() || end.is_some() {
        let start_ms = start.unwrap_or(0);
        let end_ms = end;

        if let Some(e) = end_ms
            && e <= start_ms
        {
            return Err(AppError::Internal(
                "end must be greater than start".to_owned(),
            ));
        }

        let data = extract_segment(&file_path, start_ms, end_ms).await?;
        return Ok(VideoResponse {
            data,
            content_type: "video/mp4".to_owned(),
            content_range: String::new(),
            partial: false,
        });
    }

    let file_bytes = fs::read(&file_path).await?;
    let file_size = file_bytes.len() as u64;

    if file_size == 0 {
        return Ok(VideoResponse {
            data: vec![],
            content_type: meta.content_type.clone(),
            content_range: String::new(),
            partial: false,
        });
    }

    let (start, end, partial) = if let Some(ref range_val) = range.0 {
        if let Some(bytes) = range_val.strip_prefix("bytes=") {
            let parts: Vec<&str> = bytes.splitn(2, '-').collect();
            let start: u64 = parts[0].parse().unwrap_or(0);
            let end: u64 = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse().unwrap_or(file_size - 1)
            } else {
                file_size - 1
            };
            let end = end.min(file_size - 1);
            if start > end {
                return Ok(VideoResponse {
                    data: vec![],
                    content_type: meta.content_type.clone(),
                    content_range: format!("bytes */{}", file_size),
                    partial: true,
                });
            }
            (start, end, true)
        } else {
            (0, file_size - 1, false)
        }
    } else {
        (0, file_size - 1, false)
    };

    let data = file_bytes[start as usize..=end as usize].to_vec();
    let content_range = if partial {
        format!("bytes {}-{}/{}", start, end, file_size)
    } else {
        String::new()
    };

    Ok(VideoResponse {
        data,
        content_type: meta.content_type.clone(),
        content_range,
        partial,
    })
}

#[allow(clippy::too_many_arguments)]
async fn process_uploaded_file(
    temp_path: std::path::PathBuf,
    base_mime_in: &str,
    title: &str,
    is_nsfw: bool,
    is_unlisted: bool,
    is_comments_disabled: bool,
    user: &AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    let size_bytes_initial = fs::metadata(&temp_path).await?.len();

    let verify_path = temp_path.clone();
    let magic_mime = base_mime_in.to_owned();
    let verify_result = task::spawn_blocking(move || -> Result<(), AppError> {
        let bytes = std::fs::read(&verify_path)?;
        if !verify_magic_bytes(&bytes, &magic_mime) {
            return Err(AppError::MagicMismatch);
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Err(e) = verify_result {
        let _ = fs::remove_file(&temp_path).await;
        return Err(e);
    }

    let temp_id = temp_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("x")
        .to_owned();

    let mut base_mime = base_mime_in.to_owned();
    let mut ext = extension_for_mime(base_mime_in);
    let mut size_bytes = size_bytes_initial;

    if base_mime != "video/mp4" {
        let converted_path = Path::new(&state.upload_dir).join(format!("{}.mp4", temp_id));

        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                temp_path.to_str().unwrap(),
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
                converted_path.to_str().unwrap(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| AppError::Internal(format!("ffmpeg launch failed: {e}")))?;

        if !status.success() {
            let _ = fs::remove_file(&temp_path).await;
            let _ = fs::remove_file(&converted_path).await;
            return Err(AppError::Internal("ffmpeg conversion failed".to_string()));
        }

        let _ = fs::remove_file(&temp_path).await;
        fs::rename(&converted_path, &temp_path).await?;

        base_mime = "video/mp4".to_owned();
        ext = ".mp4";
        let meta = fs::metadata(&temp_path).await?;
        size_bytes = meta.len();
    }

    let hash_path = temp_path.clone();
    let hash_result =
        task::spawn_blocking(move || -> Result<(String, Option<String>), AppError> {
            let bytes = std::fs::read(&hash_path)?;

            let digest = Sha256::digest(&bytes);
            let sha256 = digest.encode_hex::<String>();

            let tlsh_hex = TlshDefaultBuilder::build_from(&bytes)
                .and_then(|t| std::str::from_utf8(&t.hash()).ok().map(|s| s.to_owned()));

            Ok((sha256, tlsh_hex))
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let (sha256_hex, tlsh_hex) = match hash_result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&temp_path).await;
            return Err(e);
        }
    };

    if let Some(existing_id) = state.video_hashes.get(&sha256_hex) {
        let _ = fs::remove_file(&temp_path).await;
        return Err(AppError::DuplicateVideo(existing_id.clone()));
    }

    if let Some(ref new_tlsh_hex) = tlsh_hex
        && let Some(original_id) = state.find_similar_tlsh(new_tlsh_hex)
    {
        let _ = fs::remove_file(&temp_path).await;

        let original_filename = state
            .videos
            .get(&original_id)
            .map(|v| v.filename.clone())
            .unwrap_or_default();

        let video_id = Uuid::new_v4().to_string();
        let meta = VideoMeta {
            id: video_id.clone(),
            title: title.to_owned(),
            filename: original_filename,
            content_type: base_mime.to_owned(),
            size_bytes,
            sha256: sha256_hex.clone(),
            tlsh_hash: tlsh_hex.clone(),
            uploaded_by_provider: user.0.provider.clone(),
            uploaded_by_id: user.0.id,
            uploaded_by_name: user.0.username.clone(),
            uploaded_at: chrono::Utc::now(),
            nsfw: is_nsfw,
            unlisted: is_unlisted,
            comments_disabled: is_comments_disabled,
            references_id: Some(original_id.clone()),
        };

        state.videos.insert(video_id.clone(), meta.clone());
        state.persist_video(&meta);

        return Ok((
            Status::Created,
            Json(serde_json::json!({
                "message": "Video uploaded successfully (content deduplicated — similar file found)",
                "deduplicated": true,
                "original_id": original_id,
                "video": meta,
            })),
        ));
    }

    let video_id = Uuid::new_v4().to_string();
    let final_filename = format!("{}{}", video_id, ext);
    let final_path = Path::new(&state.upload_dir).join(&final_filename);

    fs::rename(&temp_path, &final_path).await?;

    let meta = VideoMeta {
        id: video_id.clone(),
        title: title.to_owned(),
        filename: final_filename,
        content_type: base_mime.to_owned(),
        size_bytes,
        sha256: sha256_hex.clone(),
        tlsh_hash: tlsh_hex.clone(),
        uploaded_by_provider: user.0.provider.clone(),
        uploaded_by_id: user.0.id,
        uploaded_by_name: user.0.username.clone(),
        uploaded_at: chrono::Utc::now(),
        nsfw: is_nsfw,
        unlisted: is_unlisted,
        comments_disabled: is_comments_disabled,
        references_id: None,
    };

    state
        .video_hashes
        .insert(sha256_hex.clone(), video_id.clone());
    if let Some(ref tlsh_val) = tlsh_hex {
        state.video_tlsh.insert(video_id.clone(), tlsh_val.clone());
    }
    state.videos.insert(video_id.clone(), meta.clone());
    state.persist_video(&meta);

    Ok((
        Status::Created,
        Json(serde_json::json!({
            "message": "Video uploaded successfully",
            "deduplicated": false,
            "video": meta,
        })),
    ))
}

#[allow(clippy::too_many_arguments)]
#[post(
    "/videos/upload?<title>&<nsfw>&<unlisted>&<comments_disabled>",
    data = "<data>"
)]
pub async fn upload_video(
    title: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    data: Data<'_>,
    content_type: &ContentType,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    let title = title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err(AppError::InvalidTitle);
    }

    let mime_str = content_type.to_string();
    let base_mime = mime_str.split(';').next().unwrap_or("").trim();

    if !ALLOWED_CONTENT_TYPES.contains(&base_mime) {
        return Err(AppError::InvalidFileType);
    }

    let is_nsfw = nsfw.unwrap_or(false);
    let is_unlisted = unlisted.unwrap_or(false);
    let is_comments_disabled = comments_disabled.unwrap_or(true);

    let temp_id = Uuid::new_v4().to_string();
    let ext = extension_for_mime(base_mime);
    let temp_filename = format!("tmp_{}{}", temp_id, ext);
    let temp_path = Path::new(&state.upload_dir).join(&temp_filename);

    let written = data.open(100.mebibytes()).into_file(&temp_path).await?;

    if !written.is_complete() {
        let _ = fs::remove_file(&temp_path).await;
        return Err(AppError::FileTooLarge);
    }

    process_uploaded_file(
        temp_path,
        base_mime,
        title,
        is_nsfw,
        is_unlisted,
        is_comments_disabled,
        &user,
        state,
    )
    .await
}

#[post("/videos/upload/init?<content_type>")]
pub async fn init_upload(
    content_type: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let base_mime = content_type.split(';').next().unwrap_or("").trim();
    if !ALLOWED_CONTENT_TYPES.contains(&base_mime) {
        return Err(AppError::InvalidFileType);
    }

    let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
    let stale: Vec<String> = state
        .upload_sessions
        .iter()
        .filter(|e| e.value().created_at < cutoff)
        .map(|e| e.key().clone())
        .collect();
    for id in &stale {
        state.upload_sessions.remove(id);
        let dir = Path::new(&state.upload_dir).join(format!("tmp_chunks_{}", id));
        let _ = fs::remove_dir_all(&dir).await;
    }

    let upload_id = Uuid::new_v4().to_string();
    let chunk_dir = Path::new(&state.upload_dir).join(format!("tmp_chunks_{}", upload_id));
    fs::create_dir_all(&chunk_dir).await?;

    state.upload_sessions.insert(
        upload_id.clone(),
        crate::state::UploadSession {
            user_provider: user.0.provider.clone(),
            user_id: user.0.id,
            content_type: base_mime.to_owned(),
            created_at: chrono::Utc::now(),
            chunk_count: 0,
        },
    );

    Ok(Json(serde_json::json!({ "upload_id": upload_id })))
}

#[put("/videos/upload/<upload_id>/<chunk_index>", data = "<data>")]
pub async fn upload_chunk(
    upload_id: &str,
    chunk_index: usize,
    data: Data<'_>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    {
        let session = state
            .upload_sessions
            .get(upload_id)
            .ok_or(AppError::VideoNotFound)?;
        if session.user_id != user.0.id || session.user_provider != user.0.provider {
            return Err(AppError::VideoNotFound);
        }
    }

    let chunk_dir = Path::new(&state.upload_dir).join(format!("tmp_chunks_{}", upload_id));
    let chunk_path = chunk_dir.join(format!("{}", chunk_index));

    let written = data.open(6.mebibytes()).into_file(&chunk_path).await?;

    {
        let mut session = state
            .upload_sessions
            .get_mut(upload_id)
            .ok_or(AppError::VideoNotFound)?;
        if chunk_index >= session.chunk_count {
            session.chunk_count = chunk_index + 1;
        }
    }

    Ok(Json(serde_json::json!({ "received": written.n.written })))
}

#[post("/videos/upload/<upload_id>/complete?<title>&<nsfw>&<unlisted>&<comments_disabled>")]
pub async fn complete_upload(
    upload_id: &str,
    title: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    let title = title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err(AppError::InvalidTitle);
    }

    let session = state
        .upload_sessions
        .remove(upload_id)
        .ok_or(AppError::VideoNotFound)?
        .1;

    if session.user_id != user.0.id || session.user_provider != user.0.provider {
        return Err(AppError::VideoNotFound);
    }

    let chunk_dir = Path::new(&state.upload_dir).join(format!("tmp_chunks_{}", upload_id));
    let temp_id = Uuid::new_v4().to_string();
    let ext = extension_for_mime(&session.content_type);
    let temp_filename = format!("tmp_{}{}", temp_id, ext);
    let temp_path = Path::new(&state.upload_dir).join(&temp_filename);

    {
        let mut outfile = tokio::fs::File::create(&temp_path).await?;
        let mut total_size: u64 = 0;

        for i in 0..session.chunk_count {
            let chunk_path = chunk_dir.join(format!("{}", i));
            let chunk_data = fs::read(&chunk_path)
                .await
                .map_err(|_| AppError::Internal(format!("Missing chunk {}", i)))?;
            total_size += chunk_data.len() as u64;

            if total_size > 100 * 1024 * 1024 {
                let _ = fs::remove_file(&temp_path).await;
                let _ = fs::remove_dir_all(&chunk_dir).await;
                return Err(AppError::FileTooLarge);
            }

            outfile.write_all(&chunk_data).await?;
        }
        outfile.flush().await?;
    }

    let _ = fs::remove_dir_all(&chunk_dir).await;

    let is_nsfw = nsfw.unwrap_or(false);
    let is_unlisted = unlisted.unwrap_or(false);
    let is_comments_disabled = comments_disabled.unwrap_or(true);
    process_uploaded_file(
        temp_path,
        &session.content_type,
        title,
        is_nsfw,
        is_unlisted,
        is_comments_disabled,
        &user,
        state,
    )
    .await
}

#[post("/videos/upload/init?<_content_type>", rank = 2)]
pub async fn init_upload_unauthorized(
    _content_type: Option<&str>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[put(
    "/videos/upload/<_upload_id>/<_chunk_index>",
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
    "/videos/upload/<_upload_id>/complete?<_title>&<_nsfw>&<_unlisted>&<_comments_disabled>",
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
    "/videos/upload?<_title>&<_nsfw>&<_unlisted>&<_comments_disabled>",
    data = "<_data>",
    rank = 2
)]
pub async fn upload_video_unauthorized(
    _title: Option<&str>,
    _nsfw: Option<bool>,
    _unlisted: Option<bool>,
    _comments_disabled: Option<bool>,
    _data: Data<'_>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required to upload videos" })),
    )
}

#[derive(Deserialize)]
pub struct NsfwPatch {
    pub nsfw: bool,
}

#[patch("/videos/<id>/nsfw", format = "json", data = "<body>")]
pub fn patch_nsfw(
    id: &str,
    body: Json<NsfwPatch>,
    _admin: AdminUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    match state.videos.get_mut(id) {
        None => Err(AppError::VideoNotFound),
        Some(mut v) => {
            v.nsfw = body.nsfw;
            let updated = v.clone();
            drop(v);
            state.persist_video(&updated);
            Ok(Json(serde_json::json!({
                "message": "NSFW flag updated",
                "id": id,
                "nsfw": body.nsfw,
            })))
        }
    }
}

#[patch("/videos/<_id>/nsfw", format = "json", data = "<_body>", rank = 2)]
pub fn patch_nsfw_forbidden(
    _id: &str,
    _body: Json<NsfwPatch>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Forbidden,
        Json(serde_json::json!({ "error": "Admin privileges required" })),
    )
}

#[delete("/videos/<id>")]
pub async fn delete_video(
    id: &str,
    _admin: AdminUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (_, meta) = state.videos.remove(id).ok_or(AppError::VideoNotFound)?;

    state.delete_video_meta(id);

    if meta.references_id.is_none() {
        let has_references = state
            .videos
            .iter()
            .any(|e| e.value().references_id.as_deref() == Some(id));

        if !has_references {
            state.video_hashes.remove(&meta.sha256);
            state.video_tlsh.remove(id);
            let file_path = Path::new(&state.upload_dir).join(&meta.filename);
            let _ = fs::remove_file(&file_path).await;
        }
    }

    state.delete_comments(id);

    Ok(Json(serde_json::json!({
        "message": format!("Video '{}' deleted", meta.title),
        "deleted_sha256": meta.sha256,
    })))
}

#[delete("/videos/<_id>", rank = 2)]
pub fn delete_video_forbidden(
    _id: &str,
    _user: AuthenticatedUser,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Forbidden,
        Json(serde_json::json!({ "error": "Admin privileges required" })),
    )
}

#[delete("/videos/<_id>", rank = 3)]
pub fn delete_video_unauthorized(_id: &str) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[get("/videos/<id>/comments")]
pub fn get_comments(id: &str, state: &State<AppState>) -> Result<Json<Vec<Comment>>, AppError> {
    if !state.videos.contains_key(id) {
        return Err(AppError::VideoNotFound);
    }
    let comments = state
        .comments
        .get(id)
        .map(|c| c.value().clone())
        .unwrap_or_default();
    Ok(Json(comments))
}

#[derive(Deserialize)]
pub struct CommentBody {
    pub text: String,
}

#[post("/videos/<id>/comments", format = "json", data = "<body>")]
pub fn add_comment(
    id: &str,
    body: Json<CommentBody>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<Comment>), AppError> {
    let trimmed_text = body.text.trim();
    if trimmed_text.is_empty() || trimmed_text.len() > 2000 {
        return Err(AppError::InvalidComment);
    }

    let meta = state.videos.get(id).ok_or(AppError::VideoNotFound)?;
    if meta.comments_disabled {
        return Err(AppError::Forbidden);
    }
    drop(meta);

    let comment = Comment {
        id: Uuid::new_v4().to_string(),
        video_id: id.to_owned(),
        author_provider: user.0.provider.clone(),
        author_id: user.0.id,
        author_name: user.0.username.clone(),
        text: trimmed_text.to_owned(),
        created_at: chrono::Utc::now(),
    };

    state
        .comments
        .entry(id.to_owned())
        .or_default()
        .push(comment.clone());
    state.persist_comments(id);

    Ok((Status::Created, Json(comment)))
}

#[post("/videos/<_id>/comments", format = "json", data = "<_body>", rank = 2)]
pub fn add_comment_unauthorized(
    _id: &str,
    _body: Json<CommentBody>,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[delete("/videos/<id>/comments/<comment_id>")]
pub fn delete_comment(
    id: &str,
    comment_id: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.videos.contains_key(id) {
        return Err(AppError::VideoNotFound);
    }
    let is_admin = state.is_admin(&user.0.provider, user.0.id);

    let mut comments = state.comments.get_mut(id).ok_or(AppError::VideoNotFound)?;
    let idx = comments
        .iter()
        .position(|c| c.id == comment_id)
        .ok_or(AppError::VideoNotFound)?;

    let is_own_comment =
        comments[idx].author_id == user.0.id && comments[idx].author_provider == user.0.provider;
    if !is_own_comment && !is_admin {
        return Err(AppError::Forbidden);
    }

    comments.remove(idx);
    drop(comments);
    state.persist_comments(id);

    Ok(Json(serde_json::json!({ "message": "Comment deleted" })))
}

#[delete("/videos/<_id>/comments/<_comment_id>", rank = 2)]
pub fn delete_comment_unauthorized(
    _id: &str,
    _comment_id: &str,
) -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Authentication required" })),
    )
}

#[derive(Deserialize)]
pub struct CommentsDisabledPatch {
    pub comments_disabled: bool,
}

#[patch("/videos/<id>/comments_disabled", format = "json", data = "<body>")]
pub fn patch_comments_disabled(
    id: &str,
    body: Json<CommentsDisabledPatch>,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut meta = state.videos.get_mut(id).ok_or(AppError::VideoNotFound)?;
    let is_admin = state.is_admin(&user.0.provider, user.0.id);
    let is_owner = meta.uploaded_by_id == user.0.id && meta.uploaded_by_provider == user.0.provider;
    if !is_owner && !is_admin {
        return Err(AppError::Forbidden);
    }
    meta.comments_disabled = body.comments_disabled;
    let updated = meta.clone();
    drop(meta);
    state.persist_video(&updated);
    Ok(Json(serde_json::json!({
        "message": "Comments disabled flag updated",
        "id": id,
        "comments_disabled": body.comments_disabled,
    })))
}

#[patch(
    "/videos/<_id>/comments_disabled",
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

fn verify_magic_bytes(bytes: &[u8], mime: &str) -> bool {
    if bytes.len() < 12 {
        return false;
    }

    match mime {
        "video/mp4" | "video/quicktime" => bytes[4..8] == *b"ftyp",
        "video/webm" | "video/x-matroska" => bytes[..4] == [0x1A, 0x45, 0xDF, 0xA3],
        "video/ogg" => bytes[..4] == *b"OggS",
        "video/x-msvideo" => bytes[..4] == *b"RIFF" && bytes[8..12] == *b"AVI ",

        _ => false,
    }
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/ogg" => ".ogv",
        "video/quicktime" => ".mov",
        "video/x-matroska" => ".mkv",
        "video/x-msvideo" => ".avi",
        _ => ".bin",
    }
}

async fn extract_segment(
    file_path: &Path,
    start_ms: u64,
    end_ms: Option<u64>,
) -> Result<Vec<u8>, AppError> {
    let path_str = file_path
        .to_str()
        .ok_or_else(|| AppError::Internal("Invalid file path".to_owned()))?;

    let mut args: Vec<String> = Vec::new();

    if start_ms > 0 {
        let ss = format!("{}.{:03}", start_ms / 1000, start_ms % 1000);
        args.extend(["-ss".into(), ss]);
    }

    args.extend(["-i".into(), path_str.to_owned()]);

    if let Some(e) = end_ms {
        let duration_ms = e.saturating_sub(start_ms);
        let duration_s = format!("{}.{:03}", duration_ms / 1000, duration_ms % 1000);
        args.extend(["-t".into(), duration_s]);
    }

    args.extend([
        "-c".into(),
        "copy".into(),
        "-avoid_negative_ts".into(),
        "make_zero".into(),
        "-f".into(),
        "mp4".into(),
        "-movflags".into(),
        "frag_keyframe+empty_moov".into(),
        "pipe:1".into(),
    ]);

    let output = rocket::tokio::process::Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("ffmpeg launch failed: {e}")))?;

    if !output.status.success() || output.stdout.is_empty() {
        return Err(AppError::Internal(
            "ffmpeg failed — segment may be out of range".to_owned(),
        ));
    }

    Ok(output.stdout)
}
