use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Request};
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post, MethodRouter};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::{Key, SameSite};

use crate::auth::{self, Allowlist, OAuthSettings};
use crate::rate_limit::{Quota, RateLimiter};
use crate::session_store::BoundedMemoryStore;
use crate::signaling::{self, SignalingStore};
use crate::tokens::{self, TokenStore};
use crate::{security, web};

/// Off only for `--features local`, which serves plain HTTP on 127.0.0.1.
pub const HTTPS_ONLY: bool = cfg!(not(feature = "local"));

#[derive(Debug, Clone)]
pub struct Settings {
    pub session_idle_timeout: Duration,
    pub session_max_age: Duration,
    pub max_sessions: usize,
    pub api_body_limit: usize,
    pub signaling: signaling::Limits,
    pub tokens: tokens::Limits,
    pub api_quota: Quota,
    pub callback_quota: Quota,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            session_idle_timeout: Duration::from_secs(24 * 60 * 60),
            session_max_age: Duration::from_secs(7 * 24 * 60 * 60),
            max_sessions: 10_000,
            api_body_limit: 64 * 1024,
            signaling: signaling::Limits::default(),
            tokens: tokens::Limits::default(),
            // Per owner, shared by the page (polls every 3 s per tab) and its
            // bearer-token devices (poll about once a second).
            api_quota: Quota {
                burst: 30,
                per_second: 5.0,
            },
            // Each callback makes two requests to GitHub. The limit is global
            // because Cloud Run gives no client address we can trust.
            callback_quota: Quota {
                burst: 20,
                per_second: 1.0,
            },
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub cookie_key: Key,
    pub public_origin: String,
    pub content_security_policy: HeaderValue,
    pub signaling: Arc<SignalingStore>,
    pub tokens: Arc<TokenStore>,
    pub api_limiter: Arc<RateLimiter<i64>>,
    pub callback_limiter: Arc<RateLimiter<()>>,
}

pub struct App {
    pub router: Router,
    pub sessions: BoundedMemoryStore,
}

pub fn build(
    oauth: OAuthSettings,
    allowlist: Allowlist,
    settings: Settings,
) -> anyhow::Result<App> {
    let public_origin = oauth.redirect_url.url().origin().ascii_serialization();
    let authorize_origin = oauth
        .endpoints
        .authorize
        .url()
        .origin()
        .ascii_serialization();
    // Sessions live in process memory, so a per-process key loses nothing on
    // restart that would not be lost anyway.
    let cookie_key = Key::generate();
    let sessions = BoundedMemoryStore::new(settings.max_sessions);
    let session_layer = tower_sessions::SessionManagerLayer::new(sessions.clone())
        .with_name(auth::SESSION_COOKIE)
        .with_path("/")
        .with_secure(HTTPS_ONLY)
        .with_http_only(true)
        // Strict would drop the cookie on the redirect back from GitHub.
        .with_same_site(SameSite::Lax)
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            settings.session_idle_timeout.try_into()?,
        ))
        .with_signed(cookie_key.clone());
    let auth_layer = axum_login::AuthManagerLayerBuilder::new(
        auth::Backend::new(oauth, allowlist),
        session_layer,
    )
    .build();

    let state = AppState {
        cookie_key,
        public_origin,
        content_security_policy: security::content_security_policy(&authorize_origin)?,
        signaling: Arc::new(SignalingStore::new(settings.signaling)),
        tokens: Arc::new(TokenStore::new(settings.tokens)),
        api_limiter: Arc::new(RateLimiter::new(settings.api_quota, settings.max_sessions)),
        callback_limiter: Arc::new(RateLimiter::new(settings.callback_quota, 1)),
        settings: Arc::new(settings),
    };

    let no_store = SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    let api: MethodRouter<AppState> = post(web::api)
        .layer::<_, std::convert::Infallible>(DefaultBodyLimit::max(state.settings.api_body_limit))
        .layer(no_store.clone());
    let token_collection: MethodRouter<AppState> = post(tokens::create)
        .get(tokens::list)
        .layer::<_, std::convert::Infallible>(DefaultBodyLimit::max(4 * 1024))
        .layer(no_store.clone());
    let token_item: MethodRouter<AppState> = delete(tokens::revoke).layer(no_store);
    let router = Router::new()
        .route("/api", api)
        .route("/api/tokens", token_collection)
        .route("/api/tokens/{id}", token_item)
        .route("/version", get(|| async { env!("CARGO_PKG_VERSION") }))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/oauth/callback", get(auth::callback))
        .layer(auth_layer)
        .fallback_service(ServeDir::new("dist"))
        .layer(from_fn_with_state(state.clone(), security::require_same_origin))
        .layer(from_fn_with_state(state.clone(), security::security_headers))
        // The default span records the full URI, including the OAuth code.
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
            tracing::debug_span!("request", method = %req.method(), path = %req.uri().path())
        }))
        .layer(CompressionLayer::new())
        .with_state(state);
    Ok(App { router, sessions })
}
