use {
    crate::{models::PlatformUser, state::AppState},
    rocket::{
        http::Status,
        request::{FromRequest, Outcome, Request},
    },
};

pub const SESSION_COOKIE: &str = "session_token";

pub struct AuthenticatedUser(pub PlatformUser);
#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let state = match req.rocket().state::<AppState>() {
            Some(s) => s,
            None => return Outcome::Error((Status::InternalServerError, ())),
        };

        let token = req
            .cookies()
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_owned());

        if let Some(t) = token
            && let Some(session) = state.sessions.get(&t)
        {
            return Outcome::Success(AuthenticatedUser(session.user.clone()));
        }

        #[cfg(debug_assertions)]
        {
            return Outcome::Success(AuthenticatedUser(PlatformUser {
                provider: "debug".to_owned(),
                id: 0,
                username: "debug_user".to_owned(),
                avatar_url: String::new(),
            }));
        }

        #[cfg(not(debug_assertions))]
        Outcome::Forward(Status::Unauthorized)
    }
}

#[allow(dead_code)]
pub struct AdminUser(pub PlatformUser);
#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminUser {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let AuthenticatedUser(user) = match AuthenticatedUser::from_request(req).await {
            Outcome::Success(u) => u,
            Outcome::Error(e) => return Outcome::Error(e),
            Outcome::Forward(f) => return Outcome::Forward(f),
        };

        let state = req.rocket().state::<AppState>().unwrap();

        #[cfg(debug_assertions)]
        if user.provider == "debug" {
            return Outcome::Success(AdminUser(user));
        }

        if state.is_admin(&user.provider, user.id) {
            Outcome::Success(AdminUser(user))
        } else {
            Outcome::Error((Status::Forbidden, ()))
        }
    }
}
