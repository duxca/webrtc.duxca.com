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
