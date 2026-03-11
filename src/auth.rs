use {
    crate::{models::PlatformUser, state::AppState},
    jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation},
    rocket::{
        http::Status,
        request::{FromRequest, Outcome, Request},
    },
    serde::{Deserialize, Serialize},
};

pub const SESSION_COOKIE: &str = "session_token";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub provider: String,
    pub id: u64,
    pub username: String,
    pub avatar_url: String,
    pub exp: u64,
}

pub fn create_jwt(user: &PlatformUser, secret: &str, remember: bool) -> String {
    let duration = if remember {
        30 * 24 * 60 * 60
    } else {
        24 * 60 * 60
    };

    let claims = Claims {
        provider: user.provider.clone(),
        id: user.id,
        username: user.username.clone(),
        avatar_url: user.avatar_url.clone(),
        exp: jsonwebtoken::get_current_timestamp() + duration,
    };

    jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("JWT encoding should not fail")
}

pub fn validate_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

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
            && let Ok(claims) = validate_jwt(&t, &state.jwt_secret)
        {
            return Outcome::Success(AuthenticatedUser(PlatformUser {
                provider: claims.provider,
                id: claims.id,
                username: claims.username,
                avatar_url: claims.avatar_url,
            }));
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
