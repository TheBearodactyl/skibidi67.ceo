#![allow(clippy::too_many_arguments)]
mod app;
mod auth;
mod error;
mod models;
mod routes;
mod state;

#[global_allocator]
pub static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[rocket::launch]
fn rocket() -> _ {
    crate::app::run()
}
