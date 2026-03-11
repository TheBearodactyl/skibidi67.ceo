use {
    crate::{
        auth::{create_jwt, validate_jwt, AuthenticatedUser, SESSION_COOKIE},
        error::AppError,
        models::{
            DiscordTokenResponse, DiscordUser, GithubTokenResponse, GithubUser, OsuTokenResponse,
            OsuUser, PlatformUser,
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

const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const DISCORD_USER_URL: &str = "https://discord.com/api/users/@me";

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

fn set_jwt_cookie(cookies: &CookieJar<'_>, user: &PlatformUser, secret: &str) {
    let remember = cookies.get("remember_me").map(|c| c.value()) == Some("true");
    let jwt = create_jwt(user, secret, remember);

    let mut cookie = Cookie::new(SESSION_COOKIE, jwt);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(true);
    cookie.set_path("/");
    if remember {
        cookie.set_max_age(rocket::time::Duration::days(30));
    }
    cookies.add(cookie);
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

    let platform_user = PlatformUser::from_osu(&user);
    set_jwt_cookie(cookies, &platform_user, &app_state.jwt_secret);

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

    let platform_user = PlatformUser::from_github(&gh_user);
    set_jwt_cookie(cookies, &platform_user, &app_state.jwt_secret);

    Ok(Redirect::to("/ui"))
}

#[get("/auth/discord/login")]
pub fn discord_login(state: &State<AppState>) -> Result<Redirect, AppError> {
    let dc = state
        .discord_oauth
        .as_ref()
        .ok_or(AppError::Internal("Discord OAuth not configured".to_owned()))?;

    if state.pending_states.len() > 10_000 {
        state.pending_states.clear();
    }

    let csrf_state = Uuid::new_v4().to_string();
    state.pending_states.insert(csrf_state.clone(), ());

    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=identify&state={}",
        DISCORD_AUTHORIZE_URL,
        urlencoded(&dc.client_id),
        urlencoded(&dc.redirect_uri),
        urlencoded(&csrf_state),
    );

    Ok(Redirect::to(url))
}

#[get("/auth/discord/callback?<code>&<state>")]
pub async fn discord_callback(
    code: &str,
    state: &str,
    app_state: &State<AppState>,
    cookies: &CookieJar<'_>,
) -> Result<Redirect, AppError> {
    if app_state.pending_states.remove(state).is_none() {
        return Err(AppError::OAuthStateMismatch);
    }

    let dc = app_state
        .discord_oauth
        .as_ref()
        .ok_or(AppError::Internal("Discord OAuth not configured".to_owned()))?;

    let client = reqwest::Client::new();

    let mut form = HashMap::new();
    form.insert("client_id", dc.client_id.clone());
    form.insert("client_secret", dc.client_secret.clone());
    form.insert("code", code.to_owned());
    form.insert("grant_type", "authorization_code".to_owned());
    form.insert("redirect_uri", dc.redirect_uri.clone());

    let token_res: DiscordTokenResponse = client
        .post(DISCORD_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(AppError::Reqwest)?
        .json()
        .await
        .map_err(|e| AppError::OAuthTokenExchange(e.to_string()))?;

    let dc_user: DiscordUser = client
        .get(DISCORD_USER_URL)
        .bearer_auth(&token_res.access_token)
        .send()
        .await
        .map_err(AppError::Reqwest)?
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch Discord user: {}", e)))?;

    let platform_user = PlatformUser::from_discord(&dc_user);
    set_jwt_cookie(cookies, &platform_user, &app_state.jwt_secret);

    Ok(Redirect::to("/ui"))
}

#[get("/auth/logout")]
pub fn logout(cookies: &CookieJar<'_>) -> Redirect {
    cookies.remove(Cookie::from(SESSION_COOKIE));
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

#[get("/auth/refresh-cookie")]
pub fn refresh_cookie(cookies: &CookieJar<'_>, app_state: &State<AppState>) -> Redirect {
    if let Some(session_cookie) = cookies.get(SESSION_COOKIE) {
        let token = session_cookie.value();
        if let Ok(claims) = validate_jwt(token, &app_state.jwt_secret) {
            let user = PlatformUser {
                provider: claims.provider,
                id: claims.id,
                username: claims.username,
                avatar_url: claims.avatar_url,
            };
            set_jwt_cookie(cookies, &user, &app_state.jwt_secret);
        }
    }
    Redirect::to("/ui")
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
