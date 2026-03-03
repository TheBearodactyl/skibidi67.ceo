use {
    crate::{
        error::AppError,
        models::{Comment, VideoMeta},
        routes::{
            media::{
                extension_for_mime, is_audio_mime, is_image_mime, is_text_mime, is_video_mime,
                verify_magic_bytes,
            },
            ui::format_size,
        },
    },
    rocket::http::Status,
};

#[test]
fn format_size_bytes() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1023), "1023 B");
}

#[test]
fn format_size_kilobytes() {
    assert_eq!(format_size(1024), "1 KB");
    assert_eq!(format_size(1536), "1.5 KB");
}

#[test]
fn format_size_megabytes() {
    assert_eq!(format_size(1024 * 1024), "1 MB");
    assert_eq!(format_size(5 * 1024 * 1024), "5 MB");
}

#[test]
fn format_size_gigabytes() {
    assert_eq!(format_size(1024 * 1024 * 1024), "1 GB");
}

#[test]
fn magic_bytes_mp4() {
    let data = [0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p'];
    assert!(verify_magic_bytes(&data, "video/mp4"));
}

#[test]
fn magic_bytes_webm() {
    let data = [0x1A, 0x45, 0xDF, 0xA3, 0x00, 0x00, 0x00, 0x00];
    assert!(verify_magic_bytes(&data, "video/webm"));
}

#[test]
fn magic_bytes_ogg() {
    let data = b"OggS\x00\x00\x00\x00";
    assert!(verify_magic_bytes(data, "video/ogg"));
    assert!(verify_magic_bytes(data, "audio/ogg"));
}

#[test]
fn magic_bytes_png() {
    let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    assert!(verify_magic_bytes(&data, "image/png"));
}

#[test]
fn magic_bytes_jpeg() {
    let data = [0xFF, 0xD8, 0xFF, 0xE0];
    assert!(verify_magic_bytes(&data, "image/jpeg"));
}

#[test]
fn magic_bytes_gif() {
    assert!(verify_magic_bytes(b"GIF89a", "image/gif"));
    assert!(verify_magic_bytes(b"GIF87a", "image/gif"));
}

#[test]
fn magic_bytes_wav() {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&[0x00; 4]);
    data.extend_from_slice(b"WAVE");
    assert!(verify_magic_bytes(&data, "audio/wav"));
}

#[test]
fn magic_bytes_flac() {
    assert!(verify_magic_bytes(b"fLaC\x00\x00\x00\x00", "audio/flac"));
}

#[test]
fn magic_bytes_text() {
    assert!(verify_magic_bytes(b"Hello world", "text/plain"));
    assert!(!verify_magic_bytes(&[0xFF, 0xFE, 0x00, 0x80], "text/plain"));
}

#[test]
fn magic_bytes_too_short() {
    assert!(!verify_magic_bytes(&[0x00, 0x00], "video/mp4"));
}

#[test]
fn magic_bytes_unknown_mime() {
    assert!(!verify_magic_bytes(b"whatever", "application/octet-stream"));
}

#[test]
fn test_is_video_mime() {
    assert!(is_video_mime("video/mp4"));
    assert!(is_video_mime("video/webm"));
    assert!(!is_video_mime("audio/mp3"));
    assert!(!is_video_mime("image/png"));
}

#[test]
fn test_is_audio_mime() {
    assert!(is_audio_mime("audio/mpeg"));
    assert!(is_audio_mime("audio/ogg"));
    assert!(!is_audio_mime("video/mp4"));
}

#[test]
fn test_is_image_mime() {
    assert!(is_image_mime("image/png"));
    assert!(is_image_mime("image/jpeg"));
    assert!(!is_image_mime("video/mp4"));
}

#[test]
fn test_is_text_mime() {
    assert!(is_text_mime("text/plain"));
    assert!(!is_text_mime("text/html"));
    assert!(!is_text_mime("application/json"));
}

#[test]
fn test_extension_for_mime() {
    assert_eq!(extension_for_mime("video/mp4"), ".mp4");
    assert_eq!(extension_for_mime("video/webm"), ".webm");
    assert_eq!(extension_for_mime("audio/mpeg"), ".mp3");
    assert_eq!(extension_for_mime("audio/flac"), ".flac");
    assert_eq!(extension_for_mime("image/png"), ".png");
    assert_eq!(extension_for_mime("image/jpeg"), ".jpg");
    assert_eq!(extension_for_mime("text/plain"), ".txt");
    assert_eq!(extension_for_mime("application/octet-stream"), ".bin");
}

#[test]
fn test_parse_admin_ids_empty() {
    let ids = crate::app::parse_admin_ids("NONEXISTENT_TEST_VAR_12345");
    assert!(ids.is_empty());
}

#[test]
fn test_parse_admin_ids_with_values() {
    unsafe { std::env::set_var("TEST_ADMIN_IDS_A", "123,456,789") };
    let ids = crate::app::parse_admin_ids("TEST_ADMIN_IDS_A");
    assert!(ids.contains(&123));
    assert!(ids.contains(&456));
    assert!(ids.contains(&789));
    assert_eq!(ids.len(), 3);
    unsafe { std::env::remove_var("TEST_ADMIN_IDS_A") };
}

#[test]
fn test_parse_admin_ids_with_whitespace() {
    unsafe { std::env::set_var("TEST_ADMIN_IDS_B", " 1 , 2 , , 3 ") };
    let ids = crate::app::parse_admin_ids("TEST_ADMIN_IDS_B");
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
    assert!(ids.contains(&3));
    assert_eq!(ids.len(), 3);
    unsafe { std::env::remove_var("TEST_ADMIN_IDS_B") };
}

#[test]
fn test_parse_admin_ids_invalid() {
    unsafe { std::env::set_var("TEST_ADMIN_IDS_C", "abc,123,def") };
    let ids = crate::app::parse_admin_ids("TEST_ADMIN_IDS_C");
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&123));
    unsafe { std::env::remove_var("TEST_ADMIN_IDS_C") };
}

#[test]
fn test_error_status_not_authenticated() {
    assert_eq!(AppError::NotAuthenticated.status(), Status::Unauthorized);
}

#[test]
fn test_error_status_forbidden() {
    assert_eq!(AppError::Forbidden.status(), Status::Forbidden);
}

#[test]
fn test_error_status_video_not_found() {
    assert_eq!(AppError::VideoNotFound.status(), Status::NotFound);
}

#[test]
fn test_error_status_file_too_large() {
    assert_eq!(AppError::FileTooLarge.status(), Status::PayloadTooLarge);
}

#[test]
fn test_error_status_duplicate() {
    assert_eq!(
        AppError::DuplicateVideo("x".into()).status(),
        Status::Conflict
    );
}

#[test]
fn test_error_status_invalid_file_type() {
    assert_eq!(
        AppError::InvalidFileType.status(),
        Status::UnsupportedMediaType
    );
}

#[test]
fn test_error_status_magic_mismatch() {
    assert_eq!(
        AppError::MagicMismatch.status(),
        Status::UnsupportedMediaType
    );
}

#[test]
fn test_error_status_invalid_title() {
    assert_eq!(AppError::InvalidTitle.status(), Status::BadRequest);
}

#[test]
fn test_error_status_internal() {
    assert_eq!(
        AppError::Internal("test".into()).status(),
        Status::InternalServerError
    );
}

#[test]
fn videometa_serialize_new_source_fields() {
    let meta = VideoMeta {
        id: "test".into(),
        title: "Test".into(),
        source: None,
        source_name: Some("Example".into()),
        source_link: Some("https://example.com".into()),
        filename: "test.mp4".into(),
        content_type: "video/mp4".into(),
        size_bytes: 1024,
        sha256: "abc".into(),
        tlsh_hash: None,
        uploaded_by_provider: "osu".into(),
        uploaded_by_id: 1,
        uploaded_by_name: "user".into(),
        uploaded_at: chrono::Utc::now(),
        nsfw: false,
        unlisted: false,
        comments_disabled: false,
        references_id: None,
        original_extension: None,
    };

    let json = serde_json::to_string(&meta).unwrap();
    assert!(json.contains("\"source_name\":\"Example\""));
    assert!(json.contains("\"source_link\":\"https://example.com\""));
}

#[test]
fn videometa_deserialize_legacy_source() {
    let json = r#"{
            "id": "test",
            "title": "Test",
            "source": "Old Source",
            "filename": "test.mp4",
            "content_type": "video/mp4",
            "size_bytes": 1024,
            "sha256": "abc",
            "uploaded_by_provider": "osu",
            "uploaded_by_id": 1,
            "uploaded_by_name": "user",
            "uploaded_at": "2024-01-01T00:00:00Z",
            "nsfw": false,
            "comments_disabled": false,
            "references_id": null
        }"#;

    let meta: VideoMeta = serde_json::from_str(json).unwrap();
    assert_eq!(meta.source.as_deref(), Some("Old Source"));
    assert!(meta.source_name.is_none());
    assert!(meta.source_link.is_none());
}

#[test]
fn comment_serialize_with_parent_id() {
    let comment = Comment {
        id: "c1".into(),
        video_id: "v1".into(),
        author_provider: "osu".into(),
        author_id: 123,
        author_name: "user".into(),
        text: "Hello".into(),
        created_at: chrono::Utc::now(),
        parent_id: Some("c0".into()),
    };

    let json = serde_json::to_string(&comment).unwrap();
    assert!(json.contains("\"parent_id\":\"c0\""));
}

#[test]
fn comment_deserialize_without_parent_id() {
    let json = r#"{
            "id": "c1",
            "video_id": "v1",
            "author_provider": "osu",
            "author_id": 123,
            "author_name": "user",
            "text": "Hello",
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

    let comment: Comment = serde_json::from_str(json).unwrap();
    assert!(comment.parent_id.is_none());
}
