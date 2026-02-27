use {
    crate::{
        auth::AuthenticatedUser,
        error::{AppError, AppResult},
        models::{Comment, VideoMeta},
        state::AppState,
    },
    hex::ToHex,
    rocket::{
        Data, State,
        data::ToByteUnit,
        http::{ContentType, Status},
        serde::json::Json,
        tokio::{fs, task},
    },
    serde::Deserialize,
    sha2::{Digest, Sha256},
    std::{io::SeekFrom, path::Path, process::Stdio},
    tlsh2::TlshDefaultBuilder,
    tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    uuid::Uuid,
};

pub const ALLOWED_VIDEO_TYPES: &[&str] = &[
    "video/mp4",
    "video/webm",
    "video/ogg",
    "video/quicktime",
    "video/x-matroska",
    "video/x-msvideo",
];

pub const ALLOWED_AUDIO_TYPES: &[&str] = &[
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
    "audio/flac",
    "audio/aac",
    "audio/webm",
];

pub const ALLOWED_IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/svg+xml",
    "image/avif",
];

pub const ALLOWED_TEXT_TYPES: &[&str] = &["text/plain"];

const MAGIC_READ_BYTES: usize = 256;

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

pub struct MediaResponse {
    pub data: Vec<u8>,
    pub content_type: String,
    pub content_range: String,
    pub status: rocket::http::Status,
}

impl<'r> rocket::response::Responder<'r, 'static> for MediaResponse {
    fn respond_to(
        self,
        _req: &'r rocket::request::Request<'_>,
    ) -> rocket::response::Result<'static> {
        let mut builder = rocket::response::Response::build();
        builder
            .status(self.status)
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

#[derive(Deserialize)]
pub struct NsfwPatch {
    pub nsfw: bool,
}

#[derive(Deserialize)]
pub struct CommentBody {
    pub text: String,
}

#[derive(Deserialize)]
pub struct CommentsDisabledPatch {
    pub comments_disabled: bool,
}

pub fn is_video_mime(mime: &str) -> bool {
    mime.starts_with("video/")
}

#[allow(dead_code)]
pub fn is_audio_mime(mime: &str) -> bool {
    mime.starts_with("audio/")
}

#[allow(dead_code)]
pub fn is_image_mime(mime: &str) -> bool {
    mime.starts_with("image/")
}

pub fn is_text_mime(mime: &str) -> bool {
    mime == "text/plain"
}

pub fn verify_magic_bytes(bytes: &[u8], mime: &str) -> bool {
    if bytes.len() < 4 {
        return false;
    }

    match mime {
        "video/mp4" | "video/quicktime" => bytes.len() >= 8 && bytes[4..8] == *b"ftyp",
        "video/webm" | "video/x-matroska" => bytes[..4] == [0x1A, 0x45, 0xDF, 0xA3],
        "video/ogg" => bytes[..4] == *b"OggS",
        "video/x-msvideo" => {
            bytes.len() >= 12 && bytes[..4] == *b"RIFF" && bytes[8..12] == *b"AVI "
        }

        "audio/mpeg" => (bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0) || bytes[..3] == *b"ID3",
        "audio/ogg" => bytes[..4] == *b"OggS",
        "audio/wav" => bytes.len() >= 12 && bytes[..4] == *b"RIFF" && bytes[8..12] == *b"WAVE",
        "audio/flac" => bytes[..4] == *b"fLaC",
        "audio/aac" => bytes[0] == 0xFF && (bytes[1] & 0xF0) == 0xF0,
        "audio/webm" => bytes[..4] == [0x1A, 0x45, 0xDF, 0xA3],

        "image/png" => bytes[..4] == [0x89, 0x50, 0x4E, 0x47],
        "image/jpeg" => bytes[..3] == [0xFF, 0xD8, 0xFF],
        "image/gif" => bytes.len() >= 6 && (bytes[..6] == *b"GIF87a" || bytes[..6] == *b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes[..4] == *b"RIFF" && bytes[8..12] == *b"WEBP",
        "image/svg+xml" => {
            let head = std::str::from_utf8(&bytes[..bytes.len().min(256)]).unwrap_or("");
            let trimmed = head.trim_start();
            trimmed.starts_with("<svg") || trimmed.starts_with("<?xml")
        }
        "image/avif" => bytes.len() >= 12 && bytes[4..8] == *b"ftyp",

        "text/plain" => std::str::from_utf8(bytes).is_ok(),

        _ => false,
    }
}

pub fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/ogg" => ".ogv",
        "video/quicktime" => ".mov",
        "video/x-matroska" => ".mkv",
        "video/x-msvideo" => ".avi",

        "audio/mpeg" => ".mp3",
        "audio/ogg" => ".ogg",
        "audio/wav" => ".wav",
        "audio/flac" => ".flac",
        "audio/aac" => ".aac",
        "audio/webm" => ".weba",

        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/svg+xml" => ".svg",
        "image/avif" => ".avif",
        "text/plain" => ".txt",
        _ => ".bin",
    }
}

pub async fn extract_segment(
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
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-crf".into(),
        "18".into(),
        "-c:a".into(),
        "aac".into(),
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

pub async fn stream_file(
    id: &str,
    start: Option<u64>,
    end: Option<u64>,
    state: &State<AppState>,
    range: RangeHeader,
    allow_segment_extraction: bool,
) -> Result<MediaResponse, AppError> {
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

    if allow_segment_extraction
        && is_video_mime(&meta.content_type)
        && (start.is_some() || end.is_some())
    {
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
        return Ok(MediaResponse {
            data,
            content_type: "video/mp4".to_owned(),
            content_range: String::new(),
            status: Status::Ok,
        });
    }

    let mut file = fs::File::open(&file_path).await.map_err(AppError::Io)?;
    let file_size = file.metadata().await?.len();

    if file_size == 0 {
        return Ok(MediaResponse {
            data: vec![],
            content_type: meta.content_type.clone(),
            content_range: String::new(),
            status: Status::Ok,
        });
    }

    let (range_start, range_end, partial) = if let Some(ref range_val) = range.0 {
        if let Some(bytes) = range_val.strip_prefix("bytes=") {
            let parts: Vec<&str> = bytes.splitn(2, '-').collect();
            let rs: u64 = parts[0].parse().unwrap_or(0);
            let re: u64 = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse().unwrap_or(file_size - 1)
            } else {
                file_size - 1
            };
            let re = re.min(file_size - 1);

            if rs > re {
                return Ok(MediaResponse {
                    data: vec![],
                    content_type: meta.content_type.clone(),
                    content_range: format!("bytes */{}", file_size),
                    status: Status::RangeNotSatisfiable,
                });
            }
            (rs, re, true)
        } else {
            (0, file_size - 1, false)
        }
    } else {
        (0, file_size - 1, false)
    };

    let read_len = (range_end - range_start + 1) as usize;
    let mut data = vec![0u8; read_len];

    file.seek(SeekFrom::Start(range_start)).await?;
    file.read_exact(&mut data).await.map_err(AppError::Io)?;

    let content_range = if partial {
        format!("bytes {}-{}/{}", range_start, range_end, file_size)
    } else {
        String::new()
    };

    Ok(MediaResponse {
        data,
        content_type: meta.content_type.clone(),
        content_range,
        status: if partial {
            Status::PartialContent
        } else {
            Status::Ok
        },
    })
}

async fn read_magic_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path).await?;
    let mut buf = vec![0u8; MAGIC_READ_BYTES];
    let n = file.read(&mut buf).await?;
    buf.truncate(n);
    Ok(buf)
}

#[allow(clippy::too_many_arguments)]
pub async fn process_uploaded_file(
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

    let magic_bytes = read_magic_bytes(&temp_path).await.map_err(AppError::Io)?;
    if !verify_magic_bytes(&magic_bytes, base_mime_in) {
        let _ = fs::remove_file(&temp_path).await;
        return Err(AppError::MagicMismatch);
    }

    if is_text_mime(base_mime_in) {
        let file_bytes = fs::read(&temp_path).await.map_err(AppError::Io)?;
        if std::str::from_utf8(&file_bytes).is_err() {
            let _ = fs::remove_file(&temp_path).await;
            return Err(AppError::MagicMismatch);
        }
    }

    let temp_id = temp_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("x")
        .to_owned();

    let mut base_mime = base_mime_in.to_owned();
    let mut ext = extension_for_mime(base_mime_in);
    let mut size_bytes = size_bytes_initial;

    if is_video_mime(base_mime_in) && base_mime != "video/mp4" {
        let converted_path = Path::new(&state.upload_dir).join(format!("{}.mp4", temp_id));

        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                temp_path.to_str().unwrap(),
                "-c:v",
                "libx264",
                "-preset",
                "slow",
                "-crf",
                "17",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
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
        size_bytes = fs::metadata(&temp_path).await?.len();
    }

    let hash_path = temp_path.clone();
    let compute_tlsh = is_video_mime(&base_mime);
    let hash_result =
        task::spawn_blocking(move || -> Result<(String, Option<String>), AppError> {
            let bytes = std::fs::read(&hash_path)?;
            let digest = Sha256::digest(&bytes);
            let sha256 = digest.encode_hex::<String>();

            let tlsh_hex = if compute_tlsh {
                TlshDefaultBuilder::build_from(&bytes).map(|t| t.hash().encode_hex::<String>())
            } else {
                None
            };

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
                "message": "Upload successful (content deduplicated — similar file found)",
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

    state.persist_video(&meta);
    state
        .video_hashes
        .insert(sha256_hex.clone(), video_id.clone());
    if let Some(ref tlsh_val) = tlsh_hex {
        state.video_tlsh.insert(video_id.clone(), tlsh_val.clone());
    }
    state.videos.insert(video_id.clone(), meta.clone());

    Ok((
        Status::Created,
        Json(serde_json::json!({
            "message": "Upload successful",
            "deduplicated": false,
            "video": meta,
        })),
    ))
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_upload(
    title: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    data: Data<'_>,
    content_type: &ContentType,
    user: AuthenticatedUser,
    state: &State<AppState>,
    allowed_types: &[&str],
) -> Result<(Status, Json<serde_json::Value>), AppError> {
    let title = title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err(AppError::InvalidTitle);
    }

    let mime_str = content_type.to_string();
    let base_mime = mime_str.split(';').next().unwrap_or("").trim();

    if !allowed_types.contains(&base_mime) {
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

pub async fn handle_init_upload(
    content_type: &str,
    user: AuthenticatedUser,
    state: &State<AppState>,
    allowed_types: &[&str],
) -> Result<Json<serde_json::Value>, AppError> {
    let base_mime = content_type.split(';').next().unwrap_or("").trim();
    if !allowed_types.contains(&base_mime) {
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

pub async fn handle_upload_chunk(
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
    if !written.is_complete() {
        let _ = fs::remove_file(&chunk_path).await;
        return Err(AppError::FileTooLarge);
    }

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

#[allow(clippy::too_many_arguments)]
pub async fn handle_complete_upload(
    upload_id: &str,
    title: &str,
    nsfw: Option<bool>,
    unlisted: Option<bool>,
    comments_disabled: Option<bool>,
    user: AuthenticatedUser,
    state: &State<AppState>,
    allowed_types: &[&str],
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

    if !allowed_types.contains(&session.content_type.as_str()) {
        return Err(AppError::InvalidFileType);
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

            let chunk_meta = fs::metadata(&chunk_path)
                .await
                .map_err(|_| AppError::Internal(format!("Missing chunk {}", i)))?;

            total_size += chunk_meta.len();
            if total_size > 100 * 1024 * 1024 {
                let _ = fs::remove_file(&temp_path).await;
                let _ = fs::remove_dir_all(&chunk_dir).await;
                return Err(AppError::FileTooLarge);
            }

            let chunk_data = fs::read(&chunk_path)
                .await
                .map_err(|_| AppError::Internal(format!("Failed to read chunk {}", i)))?;

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

pub fn handle_list(state: &State<AppState>, mime_prefix: &str) -> Json<Vec<VideoMeta>> {
    let mut items: Vec<VideoMeta> = state
        .videos
        .iter()
        .filter(|entry| {
            !entry.value().unlisted && entry.value().content_type.starts_with(mime_prefix)
        })
        .map(|entry| entry.value().clone())
        .collect();

    items.sort_by_key(|b| std::cmp::Reverse(b.uploaded_at));
    Json(items)
}

pub fn handle_get(id: &str, state: &State<AppState>) -> AppResult<Json<VideoMeta>> {
    state
        .videos
        .get(id)
        .map(|v| Json(v.clone()))
        .ok_or(AppError::VideoNotFound)
}

pub fn handle_patch_nsfw(
    id: &str,
    body: Json<NsfwPatch>,
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

pub async fn handle_delete(
    id: &str,
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
        "message": format!("'{}' deleted", meta.title),
        "deleted_sha256": meta.sha256,
    })))
}

pub fn handle_get_comments(
    id: &str,
    state: &State<AppState>,
) -> Result<Json<Vec<Comment>>, AppError> {
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

pub fn handle_add_comment(
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

pub fn handle_delete_comment(
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

pub fn handle_patch_comments_disabled(
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
