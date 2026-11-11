use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::Json;

use crate::app::AppState;
use crate::auth::{self, User};
use crate::signaling::SetError;

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
    use crate::signaling::Slot;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: Slot,
        pub value: String,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {}
}
pub mod get_value {
    use crate::signaling::Slot;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: Slot,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub value: Option<String>,
    }
}
pub mod delete_value {
    use crate::signaling::Slot;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: Slot,
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
    RateLimited,
}

fn error(status: StatusCode, kind: ErrorKind) -> HttpResponse {
    (status, Json(Response::Error(kind))).into_response()
}

/// Signed-in caller of `/api`. As a parts extractor it runs before the body is
/// read, so unauthenticated or throttled requests never get their body parsed.
pub struct ApiCaller(User);

impl FromRequestParts<AppState> for ApiCaller {
    type Rejection = HttpResponse;

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
        Ok(Self(user))
    }
}

/// POST /api
///
/// Every slot is addressed relative to the caller, so one user can never read
/// or overwrite another user's signaling data.
pub async fn api(
    ApiCaller(user): ApiCaller,
    State(app): State<AppState>,
    body: Result<Json<Request>, JsonRejection>,
) -> HttpResponse {
    let req = match body {
        Ok(Json(req)) => req,
        Err(rejection) => return error(rejection.status(), ErrorKind::InvalidRequest),
    };
    let res: Response = match req {
        Request::GetMe(get_me::Request {}) => get_me::Response { user_id: user.id }.into(),
        Request::SetValue(set_value::Request { key, value }) => {
            match app.signaling.set(user.id, key, value) {
                Ok(()) => set_value::Response {}.into(),
                Err(SetError::ValueTooLarge) => {
                    return error(StatusCode::PAYLOAD_TOO_LARGE, ErrorKind::ValueTooLarge)
                }
                Err(SetError::StoreFull) => {
                    return error(StatusCode::SERVICE_UNAVAILABLE, ErrorKind::StoreFull)
                }
            }
        }
        Request::GetValue(get_value::Request { key }) => get_value::Response {
            value: app.signaling.get(user.id, key),
        }
        .into(),
        Request::DeleteValue(delete_value::Request { key }) => {
            app.signaling.delete(user.id, key);
            delete_value::Response {}.into()
        }
    };
    Json(res).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signaling::Slot;

    #[test]
    fn keys_are_slot_names_only() {
        let req: Request = serde_json::from_str(r#"{"type":"GetValue","key":"sdp-res"}"#).unwrap();
        assert_eq!(
            req,
            Request::GetValue(get_value::Request { key: Slot::SdpRes })
        );
        assert!(
            serde_json::from_str::<Request>(r#"{"type":"GetValue","key":"sdp-res-1"}"#).is_err()
        );
    }
}
