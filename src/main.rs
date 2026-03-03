#![allow(clippy::too_many_arguments)]
mod app;
mod auth;
mod error;
mod models;
mod routes;
mod state;
#[cfg(test)]
mod tests;

#[global_allocator]
pub static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[rocket::launch]
fn rocket() -> _ {
    crate::app::run()
}
