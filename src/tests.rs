//! End-to-end tests of the HTTP surface against a fake GitHub OAuth server.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use http_body_util::BodyExt;
use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use crate::app::{self, App, Settings, HTTPS_ONLY};
use crate::auth::{self, Allowlist, GitHubEndpoints, OAuthSettings};
use crate::rate_limit::Quota;
use crate::session_store::BoundedMemoryStore;
use crate::signaling;

const PUBLIC_ORIGIN: &str = "http://127.0.0.1:8080";
/// The fake `/user` endpoint fails for this login with a body that must not
/// reach the client.
const BROKEN_LOGIN: &str = "broken";
const UPSTREAM_DETAIL: &str = "upstream stack trace";

struct Grant {
    id: i64,
    login: String,
    code_challenge: String,
}

#[derive(Default)]
struct FakeGitHub {
    next_code: AtomicUsize,
    grants: Mutex<HashMap<String, Grant>>,
    tokens: Mutex<HashMap<String, (i64, String)>>,
}

impl FakeGitHub {
    fn grant(&self, id: i64, login: &str, code_challenge: &str) -> String {
        let code = format!("code-{}", self.next_code.fetch_add(1, Ordering::Relaxed));
        let grant = Grant {
            id,
            login: login.into(),
            code_challenge: code_challenge.into(),
        };
        self.grants.lock().unwrap().insert(code.clone(), grant);
        code
    }
}

fn s256(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

async fn fake_token(
    State(github): State<Arc<FakeGitHub>>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let grant = form
        .get("code")
        .and_then(|code| github.grants.lock().unwrap().remove(code));
    match (grant, form.get("code_verifier")) {
        (Some(grant), Some(verifier)) if s256(verifier) == grant.code_challenge => {
            let token = format!("token-{}", github.next_code.fetch_add(1, Ordering::Relaxed));
            github
                .tokens
                .lock()
                .unwrap()
                .insert(token.clone(), (grant.id, grant.login));
            Json(json!({"access_token": token, "token_type": "bearer", "scope": ""}))
        }
        // GitHub answers a bad code with 200 and an error body.
        _ => Json(json!({"error": "bad_verification_code"})),
    }
}

async fn fake_user(State(github): State<Arc<FakeGitHub>>, headers: HeaderMap) -> Response {
    let account = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|token| github.tokens.lock().unwrap().get(token).cloned());
    match account {
        Some((_, login)) if login == BROKEN_LOGIN => {
            (StatusCode::INTERNAL_SERVER_ERROR, UPSTREAM_DETAIL).into_response()
        }
        Some((id, login)) => Json(json!({"id": id, "login": login})).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Default)]
struct Browser {
    cookies: HashMap<String, String>,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|v| v.to_str().unwrap())
    }
}

struct Harness {
    router: Router,
    sessions: BoundedMemoryStore,
    github: Arc<FakeGitHub>,
}

async fn harness(allowlist: Allowlist, settings: Settings) -> Harness {
    let github = Arc::new(FakeGitHub::default());
    let fake = Router::new()
        .route("/login/oauth/access_token", post(fake_token))
        .route("/user", get(fake_user))
        .with_state(github.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });

    let oauth = OAuthSettings {
        client_id: ClientId::new("client".into()),
        client_secret: ClientSecret::new("secret".into()),
        redirect_url: RedirectUrl::new(format!("{PUBLIC_ORIGIN}/oauth/callback")).unwrap(),
        endpoints: GitHubEndpoints {
            authorize: AuthUrl::new(format!("{base}/login/oauth/authorize")).unwrap(),
            token: TokenUrl::new(format!("{base}/login/oauth/access_token")).unwrap(),
            user: format!("{base}/user").parse().unwrap(),
        },
    };
    let App { router, sessions } = app::build(oauth, allowlist, settings).unwrap();
    Harness {
        router,
        sessions,
        github,
    }
}

fn request(method: Method, uri: &str) -> axum::http::request::Builder {
    let builder = Request::builder().method(method.clone()).uri(uri);
    // What a browser sends for a fetch or form post from our own page.
    if method == Method::POST {
        builder.header("sec-fetch-site", "same-origin")
    } else {
        builder
    }
}

fn api_request() -> axum::http::request::Builder {
    request(Method::POST, "/api").header(header::CONTENT_TYPE, "application/json")
}

fn json_body(body: &Value) -> Body {
    Body::from(serde_json::to_vec(body).unwrap())
}

const GET_ME: &str = r#"{"type":"GetMe"}"#;

impl Harness {
    async fn send(&self, browser: &mut Browser, mut req: Request<Body>) -> Reply {
        if !browser.cookies.is_empty() {
            let cookie = browser
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ");
            req.headers_mut()
                .insert(header::COOKIE, cookie.parse().unwrap());
        }
        let res = self.router.clone().oneshot(req).await.unwrap();
        for set_cookie in res.headers().get_all(header::SET_COOKIE) {
            let raw = set_cookie.to_str().unwrap();
            let (pair, attributes) = raw.split_once(';').unwrap_or((raw, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if value.is_empty() || attributes.contains("Max-Age=0") {
                browser.cookies.remove(name);
            } else {
                browser.cookies.insert(name.into(), value.into());
            }
        }
        Reply {
            status: res.status(),
            headers: res.headers().clone(),
            body: res.into_body().collect().await.unwrap().to_bytes(),
        }
    }

    async fn api(&self, browser: &mut Browser, body: Value) -> Reply {
        let req = api_request().body(json_body(&body)).unwrap();
        self.send(browser, req).await
    }

    async fn get_me(&self, browser: &mut Browser) -> StatusCode {
        let req = api_request().body(Body::from(GET_ME)).unwrap();
        self.send(browser, req).await.status
    }

    /// POST /login and return the `state` and `code_challenge` sent to GitHub.
    async fn start_login(&self, browser: &mut Browser) -> (String, String) {
        let res = self
            .send(
                browser,
                request(Method::POST, "/login").body(Body::empty()).unwrap(),
            )
            .await;
        assert_eq!(res.status, StatusCode::SEE_OTHER);
        let authorize = reqwest::Url::parse(res.header("location").unwrap()).unwrap();
        let param = |name: &str| {
            authorize
                .query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
                .unwrap()
        };
        assert_eq!(param("code_challenge_method"), "S256");
        (param("state"), param("code_challenge"))
    }

    async fn callback(&self, browser: &mut Browser, code: &str, state: &str) -> Reply {
        let uri = format!("/oauth/callback?code={code}&state={state}");
        self.send(
            browser,
            request(Method::GET, &uri).body(Body::empty()).unwrap(),
        )
        .await
    }

    async fn login_as(&self, id: i64, login: &str) -> (Browser, Reply) {
        let mut browser = Browser::default();
        let (state, challenge) = self.start_login(&mut browser).await;
        let code = self.github.grant(id, login, &challenge);
        let res = self.callback(&mut browser, &code, &state).await;
        (browser, res)
    }

    async fn signed_in(&self, id: i64, login: &str) -> Browser {
        let (browser, res) = self.login_as(id, login).await;
        assert_eq!(res.status, StatusCode::SEE_OTHER, "{}", res.text());
        assert_eq!(res.header("location"), Some("/"));
        browser
    }
}

async fn open_harness() -> Harness {
    harness(Allowlist::default(), Settings::default()).await
}

#[tokio::test]
async fn unauthenticated_api_calls_are_rejected() {
    let h = open_harness().await;
    let res = h
        .api(&mut Browser::default(), json!({"type": "GetMe"}))
        .await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        res.json(),
        json!({"type": "Error", "errorType": "Unauthenticated"})
    );
}

#[tokio::test]
async fn h1_signaling_slots_belong_to_the_signed_in_user() {
    let h = open_harness().await;
    let mut alice = h.signed_in(1, "alice").await;
    let mut mallory = h.signed_in(2, "mallory").await;

    let res = h
        .api(
            &mut alice,
            json!({"type": "SetValue", "key": "sdp-req", "value": "offer-from-alice"}),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);

    // The same slot name addresses Mallory's own data, never Alice's.
    let res = h
        .api(&mut mallory, json!({"type": "GetValue", "key": "sdp-req"}))
        .await;
    assert_eq!(res.json()["value"], Value::Null);
    h.api(
        &mut mallory,
        json!({"type": "SetValue", "key": "sdp-res", "value": "answer-from-mallory"}),
    )
    .await;
    h.api(
        &mut mallory,
        json!({"type": "DeleteValue", "key": "sdp-req"}),
    )
    .await;

    let res = h
        .api(&mut alice, json!({"type": "GetValue", "key": "sdp-res"}))
        .await;
    assert_eq!(
        res.json()["value"],
        Value::Null,
        "Mallory's answer must not reach Alice"
    );
    let res = h
        .api(&mut alice, json!({"type": "GetValue", "key": "sdp-req"}))
        .await;
    assert_eq!(res.json()["value"], "offer-from-alice");

    // The old key format that embedded a user id is refused.
    let res = h
        .api(
            &mut mallory,
            json!({"type": "GetValue", "key": "sdp-req-1"}),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.json()["errorType"], "InvalidRequest");

    let res = h.api(&mut mallory, json!({"type": "GetMe"})).await;
    assert_eq!(res.json(), json!({"type": "GetMe", "userId": 2}));
}

#[tokio::test]
async fn h1_allowlist_limits_who_can_sign_in() {
    let h = harness(Allowlist::parse(Some("1")).unwrap(), Settings::default()).await;
    let mut alice = h.signed_in(1, "alice").await;
    assert_eq!(h.get_me(&mut alice).await, StatusCode::OK);

    let (mut mallory, res) = h.login_as(2, "mallory").await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert_eq!(h.get_me(&mut mallory).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn m1_unauthenticated_login_requests_keep_no_server_state() {
    let h = open_harness().await;
    for _ in 0..500 {
        let mut browser = Browser::default();
        h.start_login(&mut browser).await;
        assert!(browser.cookies.contains_key(auth::LOGIN_COOKIE));
        assert!(!browser.cookies.contains_key(auth::SESSION_COOKIE));
    }
    assert_eq!(h.sessions.len(), 0);

    h.signed_in(1, "alice").await;
    assert_eq!(h.sessions.len(), 1);
}

#[tokio::test]
async fn m1_session_count_is_capped() {
    let settings = Settings {
        max_sessions: 1,
        ..Settings::default()
    };
    let h = harness(Allowlist::default(), settings).await;
    let mut alice = h.signed_in(1, "alice").await;
    let (_, res) = h.login_as(2, "bob").await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(h.sessions.len(), 1);
    assert_eq!(h.get_me(&mut alice).await, StatusCode::OK);
}

#[tokio::test]
async fn m2_body_is_read_only_after_auth_and_sizes_are_capped() {
    let h = open_harness().await;
    let huge = json!({"type": "SetValue", "key": "sdp-req", "value": "x".repeat(1024 * 1024)});
    let res = h.api(&mut Browser::default(), huge.clone()).await;
    assert_eq!(
        res.status,
        StatusCode::UNAUTHORIZED,
        "body must not be parsed before auth"
    );

    let mut alice = h.signed_in(1, "alice").await;
    let res = h.api(&mut alice, huge).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);

    let max = signaling::Limits::default().max_value_bytes;
    let res = h
        .api(
            &mut alice,
            json!({"type": "SetValue", "key": "sdp-req", "value": "x".repeat(max + 1)}),
        )
        .await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.json()["errorType"], "ValueTooLarge");
    let res = h
        .api(
            &mut alice,
            json!({"type": "SetValue", "key": "sdp-req", "value": "x".repeat(max)}),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
}

#[tokio::test]
async fn m2_entry_count_is_capped() {
    let settings = Settings {
        signaling: signaling::Limits {
            max_entries: 1,
            ..signaling::Limits::default()
        },
        ..Settings::default()
    };
    let h = harness(Allowlist::default(), settings).await;
    let mut alice = h.signed_in(1, "alice").await;
    let mut bob = h.signed_in(2, "bob").await;
    let set = json!({"type": "SetValue", "key": "sdp-req", "value": "v"});
    assert_eq!(h.api(&mut alice, set.clone()).await.status, StatusCode::OK);
    let res = h.api(&mut bob, set).await;
    assert_eq!(res.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.json()["errorType"], "StoreFull");
}

#[tokio::test]
async fn m3_cross_origin_requests_are_refused() {
    let h = open_harness().await;
    let mut alice = h.signed_in(1, "alice").await;

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api")
        .header(header::ORIGIN, "https://evil.duxca.com")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .body(Body::empty())
        .unwrap();
    let res = h.send(&mut alice, preflight).await;
    assert!(res.header("access-control-allow-origin").is_none());
    assert!(res.header("access-control-allow-credentials").is_none());

    // A sibling subdomain is same-site, so the browser would attach the cookie.
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://evil.duxca.com")
        .header("sec-fetch-site", "same-site")
        .body(Body::from(GET_ME))
        .unwrap();
    assert_eq!(h.send(&mut alice, req).await.status, StatusCode::FORBIDDEN);

    // Without Fetch Metadata the Origin header decides.
    for (origin, expected) in [
        ("https://evil.duxca.com", StatusCode::FORBIDDEN),
        (PUBLIC_ORIGIN, StatusCode::OK),
    ] {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, origin)
            .body(Body::from(GET_ME))
            .unwrap();
        assert_eq!(h.send(&mut alice, req).await.status, expected, "{origin}");
    }

    let logout = Request::builder()
        .method(Method::POST)
        .uri("/logout")
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        h.send(&mut alice, logout).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(h.get_me(&mut alice).await, StatusCode::OK);
}

#[tokio::test]
async fn l1_internal_errors_do_not_leak_details() {
    let h = open_harness().await;
    let (_, res) = h.login_as(3, BROKEN_LOGIN).await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    let body = res.text();
    assert!(body.starts_with("internal server error (id: "), "{body}");
    for leaked in [UPSTREAM_DETAIL, "127.0.0.1", "GitHub", "reqwest"] {
        assert!(!body.contains(leaked), "{body}");
    }
}

#[tokio::test]
async fn l2_state_is_checked_and_single_use() {
    let h = open_harness().await;
    let mut browser = Browser::default();
    let (state, challenge) = h.start_login(&mut browser).await;
    let code = h.github.grant(1, "alice", &challenge);
    let res = h.callback(&mut browser, &code, "forged-state").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    // The failed attempt consumed the pending login.
    let res = h.callback(&mut browser, &code, &state).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);

    let mut browser = Browser::default();
    let (state, challenge) = h.start_login(&mut browser).await;
    let code = h.github.grant(1, "alice", &challenge);
    assert_eq!(
        h.callback(&mut browser, &code, &state).await.status,
        StatusCode::SEE_OTHER
    );
    let res = h.callback(&mut browser, &code, &state).await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "a completed callback cannot be replayed"
    );
}

#[tokio::test]
async fn l2_login_cookie_is_signed() {
    let h = open_harness().await;
    let mut browser = Browser::default();
    let (state, challenge) = h.start_login(&mut browser).await;
    let code = h.github.grant(1, "alice", &challenge);
    let cookie = browser.cookies.get_mut(auth::LOGIN_COOKIE).unwrap();
    *cookie = cookie.replacen(&state, "attacker-state", 1);
    let res = h.callback(&mut browser, &code, "attacker-state").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn l2_code_without_the_pkce_verifier_is_refused() {
    let h = open_harness().await;
    let mut browser = Browser::default();
    let (state, _) = h.start_login(&mut browser).await;
    // Simulates a code that was issued for someone else's PKCE challenge.
    let code = h.github.grant(1, "alice", "challenge-of-another-login");
    let res = h.callback(&mut browser, &code, &state).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert_eq!(res.text(), "authentication failed");
}

#[tokio::test]
async fn l3_sign_out_ends_every_session_of_the_user() {
    let h = open_harness().await;
    let mut laptop = h.signed_in(1, "alice").await;
    let mut phone = h.signed_in(1, "alice").await;
    let mut bob = h.signed_in(2, "bob").await;
    assert_eq!(h.get_me(&mut phone).await, StatusCode::OK);

    let logout = request(Method::POST, "/logout")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        h.send(&mut laptop, logout).await.status,
        StatusCode::SEE_OTHER
    );
    assert_eq!(h.get_me(&mut laptop).await, StatusCode::UNAUTHORIZED);
    assert_eq!(h.get_me(&mut phone).await, StatusCode::UNAUTHORIZED);
    assert_eq!(h.get_me(&mut bob).await, StatusCode::OK);

    let mut again = h.signed_in(1, "alice").await;
    assert_eq!(h.get_me(&mut again).await, StatusCode::OK);
}

#[tokio::test]
async fn l3_sessions_have_an_absolute_lifetime() {
    let settings = Settings {
        session_max_age: Duration::ZERO,
        ..Settings::default()
    };
    let h = harness(Allowlist::default(), settings).await;
    let mut alice = h.signed_in(1, "alice").await;
    assert_eq!(h.get_me(&mut alice).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn l3_session_cookie_is_signed_and_host_only() {
    let h = open_harness().await;
    let (mut alice, res) = h.login_as(1, "alice").await;
    let set_cookie = res
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|v| v.starts_with(&format!("{}=", auth::SESSION_COOKIE)))
        .unwrap();
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("Path=/"));
    assert!(!set_cookie.contains("Domain="));
    assert_eq!(set_cookie.contains("Secure"), HTTPS_ONLY);
    assert_eq!(auth::SESSION_COOKIE.starts_with("__Host-"), HTTPS_ONLY);

    let cookie = alice.cookies.get_mut(auth::SESSION_COOKIE).unwrap();
    let last = cookie.pop().unwrap();
    cookie.push(if last == 'A' { 'B' } else { 'A' });
    assert_eq!(h.get_me(&mut alice).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn l5_security_headers_are_sent() {
    let h = open_harness().await;
    let res = h
        .send(
            &mut Browser::default(),
            request(Method::GET, "/").body(Body::empty()).unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let csp = res.header("content-security-policy").unwrap();
    for directive in [
        "default-src 'none'",
        "script-src 'self'",
        "frame-ancestors 'none'",
    ] {
        assert!(csp.contains(directive), "{csp}");
    }
    assert!(!csp.contains("unsafe-inline"));
    assert_eq!(res.header("x-frame-options"), Some("DENY"));
    assert_eq!(res.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(res.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(
        res.header("strict-transport-security").is_some(),
        HTTPS_ONLY
    );
    assert!(
        !res.text().contains("<script>"),
        "scripts are external for CSP"
    );

    let res = h
        .api(&mut Browser::default(), json!({"type": "GetMe"}))
        .await;
    assert_eq!(res.header("cache-control"), Some("no-store"));
}

#[tokio::test]
async fn i2_requests_are_rate_limited() {
    let quota = Quota {
        burst: 2,
        per_second: 0.001,
    };
    let settings = Settings {
        api_quota: quota,
        callback_quota: quota,
        ..Settings::default()
    };
    let h = harness(Allowlist::default(), settings).await;
    let mut alice = h.signed_in(1, "alice").await;
    let mut bob = h.signed_in(2, "bob").await;
    let (_, res) = h.login_as(3, "carol").await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);

    assert_eq!(h.get_me(&mut alice).await, StatusCode::OK);
    assert_eq!(h.get_me(&mut alice).await, StatusCode::OK);
    let res = h.api(&mut alice, json!({"type": "GetMe"})).await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.json()["errorType"], "RateLimited");
    assert_eq!(
        h.get_me(&mut bob).await,
        StatusCode::OK,
        "limits are per user"
    );
}

#[tokio::test]
async fn i3_version_reports_only_the_package_version() {
    let h = open_harness().await;
    let res = h
        .send(
            &mut Browser::default(),
            request(Method::GET, "/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.text(), env!("CARGO_PKG_VERSION"));
}
