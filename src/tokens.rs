//! API tokens for non-browser clients (e.g. a camera device doing WebRTC
//! signaling through `/api`).
//!
//! Tokens live in process memory, like sessions and signaling entries: a
//! restart or redeploy drops every token and users have to issue new ones.
//! Only the SHA-256 of a token is kept; the plaintext is returned once.

use std::sync::{Mutex, MutexGuard};

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::app::AppState;
use crate::auth;
use crate::error::AppError;
use crate::web::{error, ErrorKind};

/// Makes leaked tokens easy to recognize, e.g. by secret scanners.
pub const TOKEN_PREFIX: &str = "wrtc_";

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_per_owner: usize,
    pub max_total: usize,
    pub max_label_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_per_owner: 20,
            max_total: 1000,
            max_label_chars: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub id: String,
    pub label: String,
    /// Unix seconds.
    pub created_at: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct IssuedToken {
    #[serde(flatten)]
    pub info: TokenInfo,
    /// The only time the plaintext leaves the server.
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum IssueError {
    #[error("label must be 1-{0} characters without control characters")]
    InvalidLabel(usize),
    #[error("token limit reached")]
    TooManyTokens,
    #[error("getrandom: {0}")]
    Random(getrandom::Error),
}

struct Record {
    owner: i64,
    info: TokenInfo,
    hash: [u8; 32],
}

pub struct TokenStore {
    limits: Limits,
    records: Mutex<Vec<Record>>,
}

impl std::fmt::Debug for TokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenStore").finish_non_exhaustive()
    }
}

impl TokenStore {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            records: Mutex::default(),
        }
    }

    pub fn issue(&self, owner: i64, label: &str) -> Result<IssuedToken, IssueError> {
        let label = label.trim();
        let chars = label.chars().count();
        if chars == 0 || chars > self.limits.max_label_chars || label.chars().any(char::is_control)
        {
            return Err(IssueError::InvalidLabel(self.limits.max_label_chars));
        }
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(IssueError::Random)?;
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(IssueError::Random)?;
        let token = format!("{TOKEN_PREFIX}{}", hex(&secret));
        let info = TokenInfo {
            id: hex(&id),
            label: label.to_owned(),
            created_at: auth::unix_now(),
        };

        let mut records = self.lock();
        if records.len() >= self.limits.max_total
            || records.iter().filter(|r| r.owner == owner).count() >= self.limits.max_per_owner
        {
            return Err(IssueError::TooManyTokens);
        }
        records.push(Record {
            owner,
            info: info.clone(),
            hash: digest(&token),
        });
        Ok(IssuedToken { info, token })
    }

    /// Returns the owner of a valid token.
    pub fn authenticate(&self, token: &str) -> Option<i64> {
        if !token.starts_with(TOKEN_PREFIX) {
            return None;
        }
        let hash = digest(token);
        // A linear scan with constant-time comparison; the store is small.
        self.lock()
            .iter()
            .find(|record| bool::from(record.hash.ct_eq(&hash)))
            .map(|record| record.owner)
    }

    pub fn list(&self, owner: i64) -> Vec<TokenInfo> {
        self.lock()
            .iter()
            .filter(|record| record.owner == owner)
            .map(|record| record.info.clone())
            .collect()
    }

    /// Removes one of `owner`'s tokens. Other owners' ids are reported as
    /// missing, so ids cannot be probed.
    pub fn revoke(&self, owner: i64, id: &str) -> bool {
        let mut records = self.lock();
        let before = records.len();
        records.retain(|record| !(record.owner == owner && record.info.id == id));
        records.len() != before
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Record>> {
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Credentials in an `Authorization` header: `None` when the header is absent,
/// `Some(None)` when it is present but not a usable bearer token.
pub fn bearer_token(headers: &HeaderMap) -> Option<Option<&str>> {
    let value = headers.get(header::AUTHORIZATION)?;
    Some(
        value
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty()),
    )
}

/// Signed-in browser session, rate-limited. Token management is session-only,
/// so a bearer token can never mint, list or revoke tokens.
pub struct SessionCaller(i64);

impl FromRequestParts<AppState> for SessionCaller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        app: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = auth::current_user(parts, app.settings.session_max_age)
            .await
            .map_err(IntoResponse::into_response)?;
        let Some(user) = user else {
            return Err(error(StatusCode::UNAUTHORIZED, ErrorKind::Unauthenticated));
        };
        if !app.api_limiter.check(user.id) {
            return Err(error(StatusCode::TOO_MANY_REQUESTS, ErrorKind::RateLimited));
        }
        Ok(Self(user.id))
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateRequest {
    label: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ListResponse {
    tokens: Vec<TokenInfo>,
}

/// POST /api/tokens
pub async fn create(
    SessionCaller(owner): SessionCaller,
    State(app): State<AppState>,
    body: Result<Json<CreateRequest>, JsonRejection>,
) -> Result<Response, AppError> {
    let req = match body {
        Ok(Json(req)) => req,
        Err(rejection) => return Ok(error(rejection.status(), ErrorKind::InvalidRequest)),
    };
    match app.tokens.issue(owner, &req.label) {
        Ok(issued) => {
            tracing::info!(owner, token_id = %issued.info.id, "API token issued");
            Ok((StatusCode::CREATED, Json(issued)).into_response())
        }
        Err(IssueError::InvalidLabel(_)) => Ok(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorKind::InvalidRequest,
        )),
        Err(IssueError::TooManyTokens) => Ok(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorKind::TooManyTokens,
        )),
        Err(err @ IssueError::Random(_)) => Err(err.into()),
    }
}

/// GET /api/tokens
pub async fn list(
    SessionCaller(owner): SessionCaller,
    State(app): State<AppState>,
) -> Json<ListResponse> {
    Json(ListResponse {
        tokens: app.tokens.list(owner),
    })
}

/// DELETE /api/tokens/{id}
pub async fn revoke(
    SessionCaller(owner): SessionCaller,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if app.tokens.revoke(owner, &id) {
        tracing::info!(owner, token_id = %id, "API token revoked");
        StatusCode::NO_CONTENT.into_response()
    } else {
        error(StatusCode::NOT_FOUND, ErrorKind::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn store() -> TokenStore {
        TokenStore::new(Limits {
            max_per_owner: 2,
            max_total: 3,
            max_label_chars: 8,
        })
    }

    #[test]
    fn issued_tokens_authenticate_until_revoked() {
        let store = store();
        let issued = store.issue(1, " device ").unwrap();
        assert_eq!(issued.info.label, "device");
        assert!(issued.token.starts_with(TOKEN_PREFIX));
        let secret = &issued.token[TOKEN_PREFIX.len()..];
        assert_eq!(secret.len(), 64);
        assert!(secret.bytes().all(|b| b.is_ascii_hexdigit()));

        assert_eq!(store.authenticate(&issued.token), Some(1));
        assert_eq!(store.authenticate(secret), None, "prefix is required");
        assert_eq!(
            store.authenticate(&format!("{TOKEN_PREFIX}{}", "0".repeat(64))),
            None
        );

        assert!(
            !store.revoke(2, &issued.info.id),
            "only the owner can revoke"
        );
        assert_eq!(store.authenticate(&issued.token), Some(1));
        assert!(store.revoke(1, &issued.info.id));
        assert_eq!(store.authenticate(&issued.token), None);
    }

    #[test]
    fn only_hashes_are_stored() {
        let store = store();
        let issued = store.issue(1, "device").unwrap();
        let records = store.lock();
        assert_eq!(records[0].hash, digest(&issued.token));
        assert_ne!(&records[0].hash[..], issued.token.as_bytes());
        assert!(!format!("{:?}", records[0].info).contains(&issued.token));
    }

    #[test]
    fn list_is_per_owner_and_has_no_secrets() {
        let store = store();
        let issued = store.issue(1, "device").unwrap();
        store.issue(2, "other").unwrap();
        assert_eq!(store.list(1), vec![issued.info.clone()]);
        let json = serde_json::to_string(&store.list(1)).unwrap();
        assert!(!json.contains(&issued.token[TOKEN_PREFIX.len()..]));
    }

    #[test]
    fn labels_and_counts_are_limited() {
        let store = store();
        for bad in ["", "   ", "123456789", "a\u{7}b"] {
            assert!(
                matches!(store.issue(1, bad), Err(IssueError::InvalidLabel(8))),
                "{bad:?}"
            );
        }
        store.issue(1, "a").unwrap();
        store.issue(1, "b").unwrap();
        assert!(matches!(
            store.issue(1, "c"),
            Err(IssueError::TooManyTokens)
        ));
        store.issue(2, "c").unwrap();
        assert!(matches!(
            store.issue(3, "d"),
            Err(IssueError::TooManyTokens)
        ));
    }

    #[test]
    fn bearer_header_parsing() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer_token(&headers), None);
        for (value, expected) in [
            ("Bearer wrtc_abc", Some("wrtc_abc")),
            ("Bearer ", None),
            ("Basic dXNlcjpwYXNz", None),
            ("wrtc_abc", None),
        ] {
            headers.insert(header::AUTHORIZATION, HeaderValue::from_static(value));
            assert_eq!(bearer_token(&headers), Some(expected), "{value}");
        }
    }
}
