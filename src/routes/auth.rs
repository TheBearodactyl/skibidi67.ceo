use {
    crate::{
        auth::{AuthenticatedUser, SESSION_COOKIE},
        error::AppError,
        models::{
            GithubTokenResponse, GithubUser, OsuTokenResponse, OsuUser, PlatformUser, Session,
        },
        state::AppState,
    },
    hashbrown::HashMap,
    rocket::{
        State, get,
        http::{Cookie, CookieJar, SameSite, Status},
        response::Redirect,
        serde::json::Json,
    },
    serde_json::Value,
    uuid::Uuid,
};

const OSU_AUTHORIZE_URL: &str = "https://osu.ppy.sh/oauth/authorize";
const OSU_TOKEN_URL: &str = "https://osu.ppy.sh/oauth/token";
const OSU_ME_URL: &str = "https://osu.ppy.sh/api/v2/me";

const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";

#[get("/auth/login")]
pub fn login(state: &State<AppState>) -> Redirect {
    if state.pending_states.len() > 10_000 {
        state.pending_states.clear();
    }

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
        user: PlatformUser::from_osu(&user),
        access_token: token_res.access_token,
        refresh_token: token_res.refresh_token,
        created_at: chrono::Utc::now(),
    };

    app_state.sessions.insert(session_token.clone(), session);

    let mut cookie = Cookie::new(SESSION_COOKIE, session_token);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(true);
    cookie.set_path("/");
    cookies.add(cookie);

    Ok(Redirect::to("/ui"))
}

#[get("/auth/github/login")]
pub fn github_login(state: &State<AppState>) -> Result<Redirect, AppError> {
    let gh = state
        .github_oauth
        .as_ref()
        .ok_or(AppError::Internal("GitHub OAuth not configured".to_owned()))?;

    if state.pending_states.len() > 10_000 {
        state.pending_states.clear();
    }

    let csrf_state = Uuid::new_v4().to_string();
    state.pending_states.insert(csrf_state.clone(), ());

    let url = format!(
        "{}?client_id={}&redirect_uri={}&scope=read:user&state={}",
        GITHUB_AUTHORIZE_URL,
        urlencoded(&gh.client_id),
        urlencoded(&gh.redirect_uri),
        urlencoded(&csrf_state),
    );

    Ok(Redirect::to(url))
}

#[get("/auth/github/callback?<code>&<state>")]
pub async fn github_callback(
    code: &str,
    state: &str,
    app_state: &State<AppState>,
    cookies: &CookieJar<'_>,
) -> Result<Redirect, AppError> {
    if app_state.pending_states.remove(state).is_none() {
        return Err(AppError::OAuthStateMismatch);
    }

    let gh = app_state
        .github_oauth
        .as_ref()
        .ok_or(AppError::Internal("GitHub OAuth not configured".to_owned()))?;

    let client = reqwest::Client::new();

    let mut form = HashMap::new();
    form.insert("client_id", gh.client_id.clone());
    form.insert("client_secret", gh.client_secret.clone());
    form.insert("code", code.to_owned());
    form.insert("redirect_uri", gh.redirect_uri.clone());

    let token_res: GithubTokenResponse = client
        .post(GITHUB_TOKEN_URL)
        .header("Accept", "application/json")
        .json(&form)
        .send()
        .await
        .map_err(AppError::Reqwest)?
        .json()
        .await
        .map_err(|e| AppError::OAuthTokenExchange(e.to_string()))?;

    let gh_user: GithubUser = client
        .get(GITHUB_USER_URL)
        .header("User-Agent", "skibidi67")
        .bearer_auth(&token_res.access_token)
        .send()
        .await
        .map_err(AppError::Reqwest)?
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch GitHub user: {}", e)))?;

    let session_token = Uuid::new_v4().to_string();

    let session = Session {
        user: PlatformUser::from_github(&gh_user),
        access_token: token_res.access_token,
        refresh_token: None,
        created_at: chrono::Utc::now(),
    };

    app_state.sessions.insert(session_token.clone(), session);

    let mut cookie = Cookie::new(SESSION_COOKIE, session_token);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(true);
    cookie.set_path("/");
    cookies.add(cookie);

    Ok(Redirect::to("/ui"))
}

#[get("/auth/logout")]
pub fn logout(app_state: &State<AppState>, cookies: &CookieJar<'_>) -> Redirect {
    if let Some(cookie) = cookies.get(SESSION_COOKIE) {
        app_state.sessions.remove(cookie.value());
        cookies.remove(Cookie::from(SESSION_COOKIE));
    }
    Redirect::to("/ui")
}

#[get("/auth/me")]
pub fn me(user: AuthenticatedUser) -> Json<serde_json::Value> {
    let u = &user.0;
    Json(serde_json::json!({
        "provider": u.provider,
        "id": u.id,
        "username": u.username,
        "avatar_url": u.avatar_url,
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
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    out
}
