use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::Json;

use crate::app::AppState;
use crate::auth;
use crate::signaling::SetError;
use crate::tokens;

pub mod get_me {
    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {}

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub user_id: i64,
    }
}

pub mod set_value {
    use crate::signaling::Key;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: Key,
        pub value: String,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {}
}
pub mod get_value {
    use crate::signaling::Key;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: Key,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub value: Option<String>,
    }
}
pub mod delete_value {
    use crate::signaling::Key;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: Key,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {}
}

#[derive(
    Debug,
    serde::Deserialize,
    serde::Serialize,
    Clone,
    PartialEq,
    derive_more::TryInto,
    derive_more::From,
)]
#[serde(tag = "type")]
#[serde(rename_all = "PascalCase")]
pub enum Request {
    GetMe(get_me::Request),
    SetValue(set_value::Request),
    GetValue(get_value::Request),
    DeleteValue(delete_value::Request),
}

#[derive(
    Debug,
    serde::Deserialize,
    serde::Serialize,
    Clone,
    PartialEq,
    derive_more::TryInto,
    derive_more::From,
)]
#[serde(tag = "type")]
#[serde(rename_all = "PascalCase")]
pub enum Response {
    GetMe(get_me::Response),
    SetValue(set_value::Response),
    GetValue(get_value::Response),
    DeleteValue(delete_value::Response),
    Error(ErrorKind),
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "errorType")]
#[serde(rename_all = "PascalCase")]
pub enum ErrorKind {
    Unauthenticated,
    InvalidRequest,
    ValueTooLarge,
    StoreFull,
    TooManyKeys,
    RateLimited,
    TooManyTokens,
    NotFound,
}

pub(crate) fn error(status: StatusCode, kind: ErrorKind) -> HttpResponse {
    (status, Json(Response::Error(kind))).into_response()
}

/// Who is calling `/api`. Either way the store is addressed by the owner's
/// GitHub id, so a caller only ever reaches its owner's own keys.
pub enum ApiCaller {
    /// Signed-in browser session.
    Session(i64),
    /// `Authorization: Bearer` token issued by the owner (non-browser clients).
    Token(i64),
}

impl ApiCaller {
    fn owner(&self) -> i64 {
        match *self {
            Self::Session(owner) | Self::Token(owner) => owner,
        }
    }
}

/// As a parts extractor this runs before the body is read, so unauthenticated,
/// badly authenticated or throttled requests never get their body parsed.
impl FromRequestParts<AppState> for ApiCaller {
    type Rejection = HttpResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        app: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let unauthenticated = || error(StatusCode::UNAUTHORIZED, ErrorKind::Unauthenticated);
        // An Authorization header is always checked on its own merits and never
        // falls back to the session cookie.
        let caller = match tokens::bearer_token(&parts.headers) {
            Some(token) => {
                let owner = token
                    .and_then(|token| app.tokens.authenticate(token))
                    .ok_or_else(unauthenticated)?;
                Self::Token(owner)
            }
            None => {
                let user = auth::current_user(parts, app.settings.session_max_age)
                    .await
                    .map_err(IntoResponse::into_response)?
                    .ok_or_else(unauthenticated)?;
                Self::Session(user.id)
            }
        };
        if !app.api_limiter.check(caller.owner()) {
            return Err(error(StatusCode::TOO_MANY_REQUESTS, ErrorKind::RateLimited));
        }
        Ok(caller)
    }
}

/// POST /api
///
/// Keys are free-form but always scoped to the caller's owner, so one user can
/// never read or overwrite another user's signaling data.
pub async fn api(
    caller: ApiCaller,
    State(app): State<AppState>,
    body: Result<Json<Request>, JsonRejection>,
) -> HttpResponse {
    let req = match body {
        Ok(Json(req)) => req,
        Err(rejection) => return error(rejection.status(), ErrorKind::InvalidRequest),
    };
    let owner = caller.owner();
    let res: Response = match req {
        Request::GetMe(get_me::Request {}) => match caller {
            ApiCaller::Session(user_id) => get_me::Response { user_id }.into(),
            // Identity lookups are for the signed-in page only.
            ApiCaller::Token(_) => {
                return error(StatusCode::UNAUTHORIZED, ErrorKind::Unauthenticated)
            }
        },
        Request::SetValue(set_value::Request { key, value }) => {
            match app.signaling.set(owner, key, value) {
                Ok(()) => set_value::Response {}.into(),
                Err(SetError::ValueTooLarge) => {
                    return error(StatusCode::PAYLOAD_TOO_LARGE, ErrorKind::ValueTooLarge)
                }
                Err(SetError::StoreFull) => {
                    return error(StatusCode::SERVICE_UNAVAILABLE, ErrorKind::StoreFull)
                }
                Err(SetError::TooManyKeys) => {
                    return error(StatusCode::INSUFFICIENT_STORAGE, ErrorKind::TooManyKeys)
                }
            }
        }
        Request::GetValue(get_value::Request { key }) => get_value::Response {
            value: app.signaling.get(owner, key),
        }
        .into(),
        Request::DeleteValue(delete_value::Request { key }) => {
            app.signaling.delete(owner, key);
            delete_value::Response {}.into()
        }
    };
    Json(res).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signaling::Key;

    #[test]
    fn keys_are_validated_free_form_strings() {
        let req: Request =
            serde_json::from_str(r#"{"type":"GetValue","key":"aicast-webrtc/r00m/offer"}"#)
                .unwrap();
        assert_eq!(
            req,
            Request::GetValue(get_value::Request {
                key: Key::try_from("aicast-webrtc/r00m/offer".to_owned()).unwrap()
            })
        );
        for bad in [r#""""#, r#""a b""#, r#""a\u0000b""#] {
            let json = format!(r#"{{"type":"GetValue","key":{bad}}}"#);
            let err = serde_json::from_str::<Request>(&json).unwrap_err();
            assert!(
                err.is_data(),
                "{bad}: rejected by key validation, not syntax: {err}"
            );
        }
    }
}
