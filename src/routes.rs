pub mod audio;
pub mod auth;
pub mod images;
pub mod media;
pub mod text;
pub mod ui;
pub mod videos;

#[rocket::get("/health")]
pub fn health() -> rocket::serde::json::Json<serde_json::Value> {
    rocket::serde::json::Json(serde_json::json!({
        "status": "ok",
        "service": "skibidi67",
    }))
}
