use {
    crate::state::{AppState, OsuOAuthConfig},
    color_eyre::eyre::Context,
    rocket::{get, routes, serde::json::Json},
    rocket_dyn_templates::Template,
    std::collections::HashSet,
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

#[rocket::launch]
fn rocket() -> _ {
    color_eyre::install().expect("Failed to install color-eyre");
    let _ = dotenvy::dotenv();
    let oauth_config = OsuOAuthConfig::from_env()
        .wrap_err("Failed to load OAuth configuration")
        .expect("OAuth config error");

    let admin_ids: HashSet<u64> = std::env::var("ADMIN_USER_IDS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| {
            s.trim().parse::<u64>().ok().or_else(|| {
                eprintln!("Warning: could not parse admin ID '{}', skipping", s);
                None
            })
        })
        .collect();

    if admin_ids.is_empty() {
        eprintln!("Warning: ADMIN_USER_IDS is empty — no one will be able to upload videos!");
    } else {
        println!("Admin user IDs: {:?}", admin_ids);
    }

    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".into());
    std::fs::create_dir_all(&upload_dir)
        .wrap_err_with(|| format!("Could not create upload directory: {}", upload_dir))
        .expect("Failed to create upload dir");

    let app_state = AppState::new(oauth_config, admin_ids, upload_dir);

    rocket::build()
        .manage(app_state)
        .attach(Template::fairing())
        .mount(
            "/",
            routes![
                health,
                routes::auth::login,
                routes::auth::callback,
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
                routes::ui::upload_form,
                routes::ui::admin_panel,
                routes::ui::ui_delete,
            ],
        )
}
