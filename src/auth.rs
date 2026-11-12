use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum_login::{AuthUser, AuthnBackend};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RequestTokenError, TokenResponse, TokenUrl,
};
use subtle::ConstantTimeEq;
use tower_cookies::cookie::SameSite;
use tower_cookies::{Cookie, Cookies};

use crate::app::{AppState, HTTPS_ONLY};
use crate::error::AppError;

pub type AuthSession = axum_login::AuthSession<Backend>;

pub const SESSION_COOKIE: &str = if HTTPS_ONLY { "__Host-id" } else { "id" };
pub const LOGIN_COOKIE: &str = if HTTPS_ONLY { "__Host-login" } else { "login" };
const LOGIN_COOKIE_MAX_AGE: time::Duration = time::Duration::minutes(10);
const ISSUED_AT_KEY: &str = "auth.issued_at";

type GitHubClient = oauth2::basic::BasicClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

#[derive(Debug, Clone)]
pub struct GitHubEndpoints {
    pub authorize: AuthUrl,
    pub token: TokenUrl,
    pub user: reqwest::Url,
}

impl GitHubEndpoints {
    pub fn github_com() -> Self {
        Self {
            authorize: AuthUrl::new("https://github.com/login/oauth/authorize".into())
                .expect("static URL"),
            token: TokenUrl::new("https://github.com/login/oauth/access_token".into())
                .expect("static URL"),
            user: reqwest::Url::parse("https://api.github.com/user").expect("static URL"),
        }
    }
}

pub struct OAuthSettings {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub redirect_url: RedirectUrl,
    pub endpoints: GitHubEndpoints,
}

/// GitHub numeric ids that may sign in. Ids are used rather than logins
/// because logins can be renamed and re-registered. `None` keeps sign-in open.
#[derive(Debug, Clone, Default)]
pub struct Allowlist(Option<HashSet<i64>>);

impl Allowlist {
    pub fn parse(raw: Option<&str>) -> anyhow::Result<Self> {
        let ids = raw
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                id.parse::<i64>()
                    .with_context(|| format!("ALLOWED_GITHUB_IDS: {id:?} is not a GitHub user id"))
            })
            .collect::<anyhow::Result<HashSet<_>>>()?;
        Ok(Self((!ids.is_empty()).then_some(ids)))
    }

    pub fn allows(&self, github_id: i64) -> bool {
        self.0.as_ref().is_none_or(|ids| ids.contains(&github_id))
    }

    pub fn allowed_count(&self) -> Option<usize> {
        self.0.as_ref().map(HashSet::len)
    }
}

#[derive(Clone)]
pub struct User {
    pub id: i64,
    /// Random per user. Rotating it invalidates every session of the user.
    session_secret: [u8; 32],
}

impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl AuthUser for User {
    type Id = i64;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.session_secret
    }
}

pub struct Credentials {
    code: AuthorizationCode,
    pkce_verifier: PkceCodeVerifier,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct BackendError(#[from] anyhow::Error);

struct Account {
    login: String,
    session_secret: [u8; 32],
}

#[derive(Clone)]
pub struct Backend(Arc<BackendInner>);

struct BackendInner {
    oauth: GitHubClient,
    user_endpoint: reqwest::Url,
    allowlist: Allowlist,
    accounts: Mutex<HashMap<i64, Account>>,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend").finish_non_exhaustive()
    }
}

impl Backend {
    pub fn new(settings: OAuthSettings, allowlist: Allowlist) -> Self {
        let oauth = oauth2::basic::BasicClient::new(settings.client_id)
            .set_client_secret(settings.client_secret)
            .set_auth_uri(settings.endpoints.authorize)
            .set_token_uri(settings.endpoints.token)
            .set_redirect_uri(settings.redirect_url);
        Self(Arc::new(BackendInner {
            oauth,
            user_endpoint: settings.endpoints.user,
            allowlist,
            accounts: Mutex::default(),
        }))
    }

    fn authorize_url(&self) -> (oauth2::url::Url, CsrfToken, PkceCodeVerifier) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = self
            .0
            .oauth
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge)
            .url();
        (url, state, verifier)
    }

    fn revoke_sessions(&self, user_id: i64) -> anyhow::Result<()> {
        let secret = random_secret()?;
        if let Some(account) = self.accounts().get_mut(&user_id) {
            account.session_secret = secret;
        }
        Ok(())
    }

    fn accounts(&self) -> MutexGuard<'_, HashMap<i64, Account>> {
        self.0.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = BackendError;

    async fn authenticate(&self, creds: Self::Credentials) -> Result<Option<User>, BackendError> {
        // Both clients refuse redirects so a token is never sent to another host.
        let oauth_http = oauth2::reqwest::ClientBuilder::new()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()
            .context("build OAuth HTTP client")?;
        // Built before the one-time code is spent: unlike the OAuth client, this
        // one needs the system CA store (ca-certificates in the runtime image),
        // so a missing bundle fails every callback here instead of after GitHub
        // consumed the code.
        let api_http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build GitHub API client")?;
        let token = match self
            .0
            .oauth
            .exchange_code(creds.code)
            .set_pkce_verifier(creds.pkce_verifier)
            .request_async(&oauth_http)
            .await
        {
            Ok(token) => token,
            Err(RequestTokenError::Request(err)) => {
                return Err(anyhow::Error::new(err)
                    .context("token request failed")
                    .into())
            }
            Err(err) => {
                tracing::warn!(error = %err, "GitHub rejected the authorization code");
                return Ok(None);
            }
        };

        #[derive(serde::Deserialize)]
        struct GitHubUser {
            id: i64,
            login: String,
        }
        let profile: GitHubUser = api_http
            .get(self.0.user_endpoint.clone())
            .bearer_auth(token.access_token().secret())
            .header(header::USER_AGENT, "webrtc.duxca.com")
            .header(header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .context("GitHub user API request failed")?
            .json()
            .await
            .context("GitHub user API returned an unexpected body")?;

        if !self.0.allowlist.allows(profile.id) {
            tracing::info!(
                github_id = profile.id,
                "sign-in rejected: not in ALLOWED_GITHUB_IDS"
            );
            return Ok(None);
        }
        let fresh_secret = random_secret()?;
        let mut accounts = self.accounts();
        let account = accounts.entry(profile.id).or_insert_with(|| Account {
            login: profile.login.clone(),
            session_secret: fresh_secret,
        });
        account.login = profile.login;
        tracing::info!(github_id = profile.id, login = %account.login, "signed in");
        Ok(Some(User {
            id: profile.id,
            session_secret: account.session_secret,
        }))
    }

    async fn get_user(&self, user_id: &i64) -> Result<Option<User>, BackendError> {
        Ok(self.accounts().get(user_id).map(|account| User {
            id: *user_id,
            session_secret: account.session_secret,
        }))
    }
}

/// Returns the signed-in user and enforces the absolute session lifetime on
/// top of the idle timeout of the session store.
pub async fn current_user(parts: &mut Parts, max_age: Duration) -> Result<Option<User>, AppError> {
    let auth = parts
        .extensions
        .get_mut::<AuthSession>()
        .context("AuthManagerLayer is not installed")?;
    let Some(user) = auth.user.clone() else {
        return Ok(None);
    };
    let issued_at: Option<u64> = auth.session.get(ISSUED_AT_KEY).await?;
    if issued_at.is_none_or(|issued_at| unix_now().saturating_sub(issued_at) >= max_age.as_secs()) {
        auth.logout().await?;
        return Ok(None);
    }
    Ok(Some(user))
}

/// The pending login (CSRF state and PKCE verifier) lives in a short-lived
/// signed cookie, so unauthenticated requests never create server-side state.
fn login_cookie(value: String) -> Cookie<'static> {
    Cookie::build((LOGIN_COOKIE, value))
        .path("/")
        .http_only(true)
        .secure(HTTPS_ONLY)
        .same_site(SameSite::Lax)
        .max_age(LOGIN_COOKIE_MAX_AGE)
        .build()
}

/// POST /login
pub async fn login(auth: AuthSession, cookies: Cookies, State(app): State<AppState>) -> Redirect {
    let (url, state, verifier) = auth.backend.authorize_url();
    // Both values are base64url, which never contains '.'.
    let pending = format!("{}.{}", state.secret(), verifier.secret());
    cookies.signed(&app.cookie_key).add(login_cookie(pending));
    Redirect::to(url.as_str())
}

#[derive(Debug, serde::Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

/// GET /oauth/callback
pub async fn callback(
    mut auth: AuthSession,
    cookies: Cookies,
    State(app): State<AppState>,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    if !app.callback_limiter.check(()) {
        return Ok((StatusCode::TOO_MANY_REQUESTS, "too many sign-in attempts").into_response());
    }
    let jar = cookies.signed(&app.cookie_key);
    let pending = jar.get(LOGIN_COOKIE);
    // A pending login is single-use, whatever the outcome.
    jar.remove(login_cookie(String::new()));

    let Ok(Query(query)) = query else {
        return Ok((StatusCode::BAD_REQUEST, "invalid callback").into_response());
    };
    let Some((state, verifier)) = pending.as_ref().and_then(|c| c.value().split_once('.')) else {
        return Ok((StatusCode::BAD_REQUEST, "sign-in expired, please try again").into_response());
    };
    if !bool::from(state.as_bytes().ct_eq(query.state.as_bytes())) {
        return Ok((StatusCode::BAD_REQUEST, "state mismatch").into_response());
    }
    let creds = Credentials {
        code: AuthorizationCode::new(query.code),
        pkce_verifier: PkceCodeVerifier::new(verifier.to_owned()),
    };
    let Some(user) = auth.authenticate(creds).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "authentication failed").into_response());
    };
    // Also rotate the id when an already signed-in session signs in again.
    auth.session.cycle_id().await?;
    auth.login(&user).await?;
    auth.session.insert(ISSUED_AT_KEY, unix_now()).await?;
    Ok(Redirect::to("/").into_response())
}

/// POST /logout
pub async fn logout(mut auth: AuthSession) -> Result<Redirect, AppError> {
    if let Some(user) = auth.logout().await? {
        // Signing out ends the user's sessions on every device, which is the
        // way to revoke a leaked session cookie.
        auth.backend.revoke_sessions(user.id)?;
    }
    Ok(Redirect::to("/"))
}

fn random_secret() -> anyhow::Result<[u8; 32]> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    Ok(secret)
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_is_open_unless_ids_are_given() {
        for raw in [None, Some(""), Some(" , ")] {
            let allowlist = Allowlist::parse(raw).unwrap();
            assert_eq!(allowlist.allowed_count(), None);
            assert!(allowlist.allows(42));
        }
        let allowlist = Allowlist::parse(Some("1, 2")).unwrap();
        assert_eq!(allowlist.allowed_count(), Some(2));
        assert!(allowlist.allows(2));
        assert!(!allowlist.allows(3));
        assert!(Allowlist::parse(Some("legokichi")).is_err());
    }
}
