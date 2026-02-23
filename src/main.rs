#![allow(dead_code)]
use {
    crate::state::{AppState, GithubOAuthConfig, OsuOAuthConfig},
    color_eyre::eyre::Context,
    rocket::{get, routes, serde::json::Json},
    rocket_dyn_templates::Template,
    std::collections::{HashMap, HashSet},
};

mod auth;
mod error;
mod models;
mod routes;
mod state;

#[get("/health")]
fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "skibidi67",
    }))
}

fn parse_admin_ids(env_var: &str) -> HashSet<u64> {
    std::env::var(env_var)
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| {
            s.trim().parse::<u64>().ok().or_else(|| {
                eprintln!("Warning: could not parse admin ID '{}' from {}, skipping", s, env_var);
                None
            })
        })
        .collect()
}

#[rocket::launch]
fn rocket() -> _ {
    color_eyre::install().expect("Failed to install color-eyre");
    let _ = dotenvy::dotenv();
    let oauth_config = OsuOAuthConfig::from_env()
        .wrap_err("Failed to load OAuth configuration")
        .expect("OAuth config error");

    let github_oauth = GithubOAuthConfig::from_env();
    if github_oauth.is_some() {
        println!("GitHub OAuth configured.");
    } else {
        println!("GitHub OAuth not configured (set GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET, GITHUB_REDIRECT_URI to enable).");
    }

    let mut admin_ids: HashMap<String, HashSet<u64>> = HashMap::new();

    let osu_admins = parse_admin_ids("ADMIN_OSU_IDS");
    // Backward compat: also check ADMIN_USER_IDS
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
        eprintln!("Warning: No admin IDs configured — no one will be able to upload videos!");
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
                health,
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
                routes::videos::patch_nsfw,
                routes::videos::patch_nsfw_forbidden,
                routes::videos::delete_video,
                routes::videos::delete_video_forbidden,
                routes::videos::delete_video_unauthorized,
                routes::ui::index,
                routes::ui::listing,
                routes::ui::player,
                routes::ui::embed,
                routes::ui::upload_form,
                routes::ui::admin_panel,
                routes::ui::ui_delete,
            ],
        )
}
