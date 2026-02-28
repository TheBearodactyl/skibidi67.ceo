use {
    crate::{
        routes,
        state::{AppState, GithubOAuthConfig, OsuOAuthConfig},
    },
    color_eyre::eyre::Context,
    hashbrown::{HashMap, HashSet},
    rocket::{Build, Rocket, routes},
    rocket_dyn_templates::Template,
};

fn parse_admin_ids(env_var: &str) -> HashSet<u64> {
    std::env::var(env_var)
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| {
            s.trim().parse::<u64>().ok().or_else(|| {
                eprintln!(
                    "Warning: could not parse admin ID '{}' from {}, skipping",
                    s, env_var
                );
                None
            })
        })
        .collect()
}

pub fn run() -> Rocket<Build> {
    color_eyre::install().expect("Failed to install color-eyre");
    let _ = dotenvy::dotenv();
    let oauth_config = OsuOAuthConfig::from_env()
        .wrap_err("Failed to load OAuth configuration")
        .expect("OAuth config error");

    let github_oauth = GithubOAuthConfig::from_env();
    if github_oauth.is_some() {
        println!("GitHub OAuth configured.");
    } else {
        println!(
            "GitHub OAuth not configured (set GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET, GITHUB_REDIRECT_URI to enable)."
        );
    }

    let mut admin_ids: HashMap<String, HashSet<u64>> = HashMap::new();

    let osu_admins = parse_admin_ids("ADMIN_OSU_IDS");
    let legacy_admins = parse_admin_ids("ADMIN_USER_IDS");
    let combined_osu: HashSet<u64> = osu_admins.union(&legacy_admins).copied().collect();
    if !combined_osu.is_empty() {
        admin_ids.insert("osu".to_owned(), combined_osu);
    }

    let github_admins = parse_admin_ids("ADMIN_GITHUB_IDS");
    if !github_admins.is_empty() {
        admin_ids.insert("github".to_owned(), github_admins);
    }

    if admin_ids.is_empty() {
        eprintln!(
            "Warning: No admin IDs configured — no one will have admin/moderation privileges!"
        );
    } else {
        println!("Admin IDs: {:?}", admin_ids);
    }

    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".into());
    std::fs::create_dir_all(&upload_dir)
        .wrap_err_with(|| format!("Could not create upload directory: {}", upload_dir))
        .expect("Failed to create upload dir");

    let app_state = AppState::new(oauth_config, github_oauth, admin_ids, upload_dir);

    rocket::build()
        .manage(app_state)
        .attach(Template::fairing())
        .mount(
            "/",
            routes![
                routes::health,
                routes::auth::login,
                routes::auth::callback,
                routes::auth::github_login,
                routes::auth::github_callback,
                routes::auth::logout,
                routes::auth::me,
                routes::auth::me_unauthenticated,
                routes::videos::list_videos,
                routes::videos::get_video,
                routes::videos::stream_video,
                routes::videos::upload_video,
                routes::videos::upload_video_unauthorized,
                routes::videos::init_upload,
                routes::videos::init_upload_unauthorized,
                routes::videos::upload_chunk,
                routes::videos::upload_chunk_unauthorized,
                routes::videos::complete_upload,
                routes::videos::complete_upload_unauthorized,
                routes::videos::patch_nsfw,
                routes::videos::patch_nsfw_forbidden,
                routes::videos::delete_video,
                routes::videos::delete_video_forbidden,
                routes::videos::delete_video_unauthorized,
                routes::videos::get_comments,
                routes::videos::add_comment,
                routes::videos::add_comment_unauthorized,
                routes::videos::delete_comment,
                routes::videos::delete_comment_unauthorized,
                routes::videos::patch_comments_disabled,
                routes::videos::patch_comments_disabled_unauthorized,
                routes::audio::list_audio,
                routes::audio::get_audio,
                routes::audio::stream_audio,
                routes::audio::upload_audio,
                routes::audio::upload_audio_unauthorized,
                routes::audio::init_upload,
                routes::audio::init_upload_unauthorized,
                routes::audio::upload_chunk,
                routes::audio::upload_chunk_unauthorized,
                routes::audio::complete_upload,
                routes::audio::complete_upload_unauthorized,
                routes::audio::patch_nsfw,
                routes::audio::patch_nsfw_forbidden,
                routes::audio::delete_audio,
                routes::audio::delete_audio_forbidden,
                routes::audio::delete_audio_unauthorized,
                routes::audio::get_comments,
                routes::audio::add_comment,
                routes::audio::add_comment_unauthorized,
                routes::audio::delete_comment,
                routes::audio::delete_comment_unauthorized,
                routes::audio::patch_comments_disabled,
                routes::audio::patch_comments_disabled_unauthorized,
                routes::images::list_images,
                routes::images::get_image,
                routes::images::stream_image,
                routes::images::upload_image,
                routes::images::upload_image_unauthorized,
                routes::images::init_upload,
                routes::images::init_upload_unauthorized,
                routes::images::upload_chunk,
                routes::images::upload_chunk_unauthorized,
                routes::images::complete_upload,
                routes::images::complete_upload_unauthorized,
                routes::images::patch_nsfw,
                routes::images::patch_nsfw_forbidden,
                routes::images::delete_image,
                routes::images::delete_image_forbidden,
                routes::images::delete_image_unauthorized,
                routes::images::get_comments,
                routes::images::add_comment,
                routes::images::add_comment_unauthorized,
                routes::images::delete_comment,
                routes::images::delete_comment_unauthorized,
                routes::images::patch_comments_disabled,
                routes::images::patch_comments_disabled_unauthorized,
                routes::text::list_text,
                routes::text::get_text,
                routes::text::stream_text,
                routes::text::highlighted_text,
                routes::text::upload_text,
                routes::text::upload_text_unauthorized,
                routes::text::init_upload,
                routes::text::init_upload_unauthorized,
                routes::text::upload_chunk,
                routes::text::upload_chunk_unauthorized,
                routes::text::complete_upload,
                routes::text::complete_upload_unauthorized,
                routes::text::patch_nsfw,
                routes::text::patch_nsfw_forbidden,
                routes::text::delete_text,
                routes::text::delete_text_forbidden,
                routes::text::delete_text_unauthorized,
                routes::text::get_comments,
                routes::text::add_comment,
                routes::text::add_comment_unauthorized,
                routes::text::delete_comment,
                routes::text::delete_comment_unauthorized,
                routes::text::patch_comments_disabled,
                routes::text::patch_comments_disabled_unauthorized,
                routes::ui::index,
                routes::ui::favicon,
                routes::ui::listing,
                routes::ui::video_listing,
                routes::ui::audio_listing,
                routes::ui::image_listing,
                routes::ui::player,
                routes::ui::audio_player,
                routes::ui::image_viewer,
                routes::ui::embed,
                routes::ui::text_listing,
                routes::ui::text_viewer,
                routes::ui::upload_form,
                routes::ui::admin_panel,
                routes::ui::ui_delete_video,
                routes::ui::ui_delete_audio,
                routes::ui::ui_delete_image,
                routes::ui::ui_delete_text,
            ],
        )
}
