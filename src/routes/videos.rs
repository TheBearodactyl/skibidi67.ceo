use {
    crate::{
        auth::{AdminUser, AuthenticatedUser},
        error::AppError,
        models::VideoMeta,
        state::AppState,
    },
    dashmap::mapref::entry::Entry,
    hex::ToHex,
    rocket::{
        Data, State,
        data::ToByteUnit,
        delete, get,
        http::{ContentType, Status},
        patch, post,
        response::stream::ReaderStream,
        serde::json::Json,
        tokio::{fs, task},
    },
    serde::Deserialize,
    sha2::{Digest, Sha256},
    std::path::Path,
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

#[get("/videos")]
pub fn list_videos(state: &State<AppState>) -> Json<Vec<VideoMeta>> {
    let mut videos: Vec<VideoMeta> = state
        .videos
        .iter()
        .map(|entry| entry.value().clone())
        .collect();

    videos.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    Json(videos)
}

#[get("/videos/<id>")]
pub fn get_video(id: &str, state: &State<AppState>) -> Result<Json<VideoMeta>, AppError> {
    state
        .videos
        .get(id)
        .map(|v| Json(v.clone()))
        .ok_or(AppError::VideoNotFound)
}

#[get("/videos/<id>/file")]
pub async fn stream_video(
    id: &str,
    state: &State<AppState>,
) -> Result<(ContentType, ReaderStream![fs::File]), AppError> {
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
    let file = fs::File::open(&file_path).await?;

    let content_type =
        ContentType::parse_flexible(&meta.content_type).unwrap_or(ContentType::Binary);

    Ok((content_type, ReaderStream::one(file)))
}

#[post("/videos/upload?<title>&<nsfw>", data = "<data>")]
pub async fn upload_video(
    title: &str,
    nsfw: Option<bool>,
    data: Data<'_>,
    content_type: &ContentType,
    user: AuthenticatedUser,
    state: &State<AppState>,
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    let mime_str = content_type.to_string();
    let base_mime = mime_str.split(';').next().unwrap_or("").trim();

    if !ALLOWED_CONTENT_TYPES.contains(&base_mime) {
        return Err(AppError::InvalidFileType);
    }

    let is_nsfw = nsfw.unwrap_or(false);

    let temp_id = Uuid::new_v4().to_string();
    let ext = extension_for_mime(base_mime);
    let temp_filename = format!("tmp_{}{}", temp_id, ext);
    let temp_path = Path::new(&state.upload_dir).join(&temp_filename);

    let written = data.open(15.mebibytes()).into_file(&temp_path).await?;

    if !written.is_complete() {
        let _ = fs::remove_file(&temp_path).await;
        return Err(AppError::FileTooLarge);
    }

    let size_bytes = written.n.written as u64;
    let hash_path = temp_path.clone();
    let sha256_hex: String = task::spawn_blocking(move || -> Result<String, AppError> {
        let bytes = std::fs::read(&hash_path)?;
        let digest = Sha256::digest(&bytes);
        Ok(digest.encode_hex::<String>())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    match state.video_hashes.entry(sha256_hex.clone()) {
        Entry::Occupied(existing) => {
            let _ = fs::remove_file(&temp_path).await;

            let original_id = existing.get().clone();
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
                uploaded_by_id: user.0.id,
                uploaded_by_name: user.0.username.clone(),
                uploaded_at: chrono::Utc::now(),
                nsfw: is_nsfw,
                references_id: Some(original_id.clone()),
            };

            state.videos.insert(video_id.clone(), meta.clone());
            state.persist_video(&meta);

            Ok((
                Status::Created,
                Json(serde_json::json!({
                    "message": "Video uploaded successfully (content deduplicated — file shared with an earlier post)",
                    "deduplicated": true,
                    "original_id": original_id,
                    "video": meta,
                })),
            ))
        }

        Entry::Vacant(slot) => {
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
                uploaded_by_id: user.0.id,
                uploaded_by_name: user.0.username.clone(),
                uploaded_at: chrono::Utc::now(),
                nsfw: is_nsfw,
                references_id: None,
            };

            slot.insert(video_id.clone());
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
    }
}

#[post("/videos/upload?<_title>&<_nsfw>", data = "<_data>", rank = 2)]
pub async fn upload_video_unauthorized(
    _title: Option<&str>,
    _nsfw: Option<bool>,
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
            let file_path = Path::new(&state.upload_dir).join(&meta.filename);
            let _ = fs::remove_file(&file_path).await;
        }
    }

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
