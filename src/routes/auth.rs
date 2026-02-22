//! OAuth 2.0 routes for the osu! "Sign in with osu!" flow.
//!
//! Flow:
//!  1. User visits `GET /auth/login`  → redirected to osu! authorization page.
//!  2. osu! redirects back to `GET /auth/callback?code=...&state=...`.
//!  3. We exchange the code for tokens, fetch the user's profile,
//!     create a session, set a private cookie, and redirect to `/videos`.
//!  4. `GET /auth/logout` clears the cookie and the session entry.

use {
    crate::{
        auth::{AuthenticatedUser, SESSION_COOKIE},
        error::AppError,
        models::{OsuTokenResponse, OsuUser, Session},
        state::AppState,
    },
    rocket::{
        State, get,
        http::{Cookie, CookieJar, SameSite, Status},
        response::Redirect,
        serde::json::Json,
    },
    serde_json::Value,
    std::collections::HashMap,
    uuid::Uuid,
};

const OSU_AUTHORIZE_URL: &str = "https://osu.ppy.sh/oauth/authorize";
const OSU_TOKEN_URL: &str = "https://osu.ppy.sh/oauth/token";
const OSU_ME_URL: &str = "https://osu.ppy.sh/api/v2/me";

#[get("/auth/login")]
pub fn login(state: &State<AppState>) -> Redirect {
    let csrf_state = Uuid::new_v4().to_string();

    state.pending_states.insert(csrf_state.clone(), ());

    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=identify+public&state={}",
        OSU_AUTHORIZE_URL,
        state.oauth.client_id,
        urlencoded(&state.oauth.redirect_uri),
        urlencoded(&csrf_state),
    );

    Redirect::to(url)
}

#[get("/auth/callback?<code>&<state>")]
pub async fn callback(
    code: &str,
    state: &str,
    app_state: &State<AppState>,
    cookies: &CookieJar<'_>,
) -> Result<Redirect, AppError> {
    if app_state.pending_states.remove(state).is_none() {
        return Err(AppError::OAuthStateMismatch);
    }

    let client = reqwest::Client::new();

    let mut form = HashMap::new();
    form.insert("client_id", app_state.oauth.client_id.to_string());
    form.insert("client_secret", app_state.oauth.client_secret.clone());
    form.insert("code", code.to_owned());
    form.insert("grant_type", "authorization_code".to_owned());
    form.insert("redirect_uri", app_state.oauth.redirect_uri.clone());

    let token_res: OsuTokenResponse = client
        .post(OSU_TOKEN_URL)
        .json(&form)
        .send()
        .await
        .map_err(AppError::Reqwest)?
        .json()
        .await
        .map_err(|e| AppError::OAuthTokenExchange(e.to_string()))?;

    let me_res: Value = client
        .get(OSU_ME_URL)
        .bearer_auth(&token_res.access_token)
        .send()
        .await
        .map_err(AppError::Reqwest)?
        .json()
        .await
        .map_err(|e| AppError::OsuUserFetch(e.to_string()))?;

    let user: OsuUser =
        serde_json::from_value(me_res).map_err(|e| AppError::OsuUserFetch(e.to_string()))?;

    let session_token = Uuid::new_v4().to_string();

    let session = Session {
        user,
        access_token: token_res.access_token,
        refresh_token: token_res.refresh_token,
        created_at: chrono::Utc::now(),
    };

    app_state.sessions.insert(session_token.clone(), session);

    let mut cookie = Cookie::new(SESSION_COOKIE, session_token);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookies.add(cookie);

    Ok(Redirect::to("/ui"))
}

#[get("/auth/logout")]
pub fn logout(app_state: &State<AppState>, cookies: &CookieJar<'_>) -> Json<serde_json::Value> {
    if let Some(cookie) = cookies.get(SESSION_COOKIE) {
        app_state.sessions.remove(cookie.value());
        cookies.remove(Cookie::from(SESSION_COOKIE));
    }
    Json(serde_json::json!({ "message": "Logged out successfully" }))
}

#[get("/auth/me")]
pub fn me(user: AuthenticatedUser) -> Json<serde_json::Value> {
    let u = &user.0;
    Json(serde_json::json!({
        "id": u.id,
        "username": u.username,
        "avatar_url": u.avatar_url,
        "is_restricted": u.is_restricted,
    }))
}

#[get("/auth/me", rank = 2)]
pub fn me_unauthenticated() -> (Status, Json<serde_json::Value>) {
    (
        Status::Unauthorized,
        Json(serde_json::json!({ "error": "Not authenticated" })),
    )
}

fn urlencoded(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                vec![c]
            }
            _ => format!("%{:02X}", c as u32).chars().collect(),
        })
        .collect()
}
