#[derive(Clone, Default)]
pub struct State {
    pub db: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
}

pub struct Ise(anyhow::Error);

impl axum::response::IntoResponse for Ise {
    fn into_response(self) -> axum::response::Response {
        log::error!("{:?}", self.0);
        // TODO: 本番環では stack trace を表示しない
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {:?}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for Ise
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

pub const CSRF_STATE_KEY: &str = "oauth.csrf-state";

/// POST /login
#[tracing::instrument(level = "trace")]
pub async fn login(
    auth_session: axum_login::AuthSession<crate::auth::Backend>,
    session: tower_sessions::Session,
    axum::Form(()): axum::Form<()>,
) -> Result<impl axum::response::IntoResponse, crate::web::Ise> {
    use axum::response::IntoResponse;
    let (auth_url, csrf_state) = auth_session.backend.authorize_url();
    session.insert(CSRF_STATE_KEY, csrf_state.secret()).await?;
    Ok(axum::response::Redirect::to(auth_url.as_str()).into_response())
}

// OAuth2 の認可コードを受け取るためのクエリパラメータ
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AuthzRequestQuery {
    pub code: String,
    pub state: oauth2::CsrfToken,
}

/// GET /oauth/callback
#[tracing::instrument(level = "trace")]
pub async fn callback(
    mut auth_session: axum_login::AuthSession<crate::auth::Backend>,
    session: tower_sessions::Session,
    axum::extract::Query(AuthzRequestQuery {
        code,
        state: new_state,
    }): axum::extract::Query<AuthzRequestQuery>,
) -> Result<impl axum::response::IntoResponse, crate::web::Ise> {
    use axum::response::IntoResponse;
    // セッションがない場合はエラー
    let Some(old_state) = session.get(CSRF_STATE_KEY).await? else {
        return Ok((axum::http::StatusCode::BAD_REQUEST, "session expired").into_response());
    };
    let creds = crate::auth::Credentials {
        code,
        old_state,
        new_state,
        // ログイン済みかどうか
        user: auth_session.user.clone(),
    };
    let Some(user) = auth_session.authenticate(creds.clone()).await? else {
        return Ok((
            axum::http::StatusCode::UNAUTHORIZED,
            "authentication failed",
        )
            .into_response());
    };
    auth_session.login(&user).await?;

    Ok(axum::response::Redirect::to("/").into_response())
}

/// POST /logout
#[tracing::instrument(level = "trace")]
pub async fn logout(
    mut auth_session: axum_login::AuthSession<crate::auth::Backend>,
    axum::Form(()): axum::Form<()>,
) -> Result<impl axum::response::IntoResponse, crate::web::Ise> {
    auth_session.logout().await?;
    Ok(axum::response::Redirect::to("/"))
}

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
    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: String,
        pub value: String,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {}
}
pub mod get_value {
    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: String,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Response {
        pub value: Option<String>,
    }
}
pub mod delete_value {
    #[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct Request {
        pub key: String,
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
    PermissionDenied,
    InvalidRequest,
}

impl Request {
    #[allow(dead_code)]
    pub fn to_request_type_string(&self) -> Result<String, anyhow::Error> {
        use anyhow::Context;
        let json = serde_json::to_value(self)?;
        let json = json.pointer(".type").context("type field not found")?;
        let txt = json.as_str().context("type field is not a string")?;
        Ok(txt.to_string())
    }
}

impl Response {
    #[allow(dead_code)]
    pub fn to_response_type_string(&self) -> Result<String, anyhow::Error> {
        use anyhow::Context;
        let json = serde_json::to_value(self)?;
        let json = json.pointer(".type").context("type field not found")?;
        let txt = json.as_str().context("type field is not a string")?;
        Ok(txt.to_string())
    }
}

/// POST /api
#[tracing::instrument(level = "trace", skip(st))]
pub async fn api(
    auth_session: axum_login::AuthSession<crate::auth::Backend>,
    axum::extract::State(ref st): axum::extract::State<crate::web::State>,
    axum::extract::Json(json): axum::extract::Json<serde_json::Value>,
) -> Result<impl axum::response::IntoResponse, crate::web::Ise> {
    use axum::response::IntoResponse;
    let user = auth_session.user;
    let Some(user) = user else {
        return Ok((axum::http::StatusCode::UNAUTHORIZED, "401").into_response());
    };
    let Ok(req) = serde_json::from_value::<Request>(json) else {
        return Ok((axum::http::StatusCode::BAD_REQUEST, "invalid request").into_response());
    };
    let res: Response = match req {
        Request::GetMe(get_me::Request {}) => {
            let res = get_me::Response {
                user_id: user.user_id,
            };
            res.into()
        }
        Request::SetValue(set_value::Request { key, value }) => {
            let mut lock = st.db.lock().await;
            lock.insert(key, value);
            set_value::Response {}.into()
        }
        Request::GetValue(get_value::Request { key }) => {
            let lock = st.db.lock().await;
            let value = lock.get(&key).cloned();
            let res = get_value::Response { value };
            res.into()
        }
        Request::DeleteValue(delete_value::Request { key }) => {
            let mut lock = st.db.lock().await;
            lock.remove(&key);
            delete_value::Response {}.into()
        }
    };
    let json = serde_json::to_value(res)?;
    Ok(axum::response::Json::from(json).into_response())
}
