pub const CSRF_STATE_KEY: &str = "oauth.csrf-state";

/// POST /login
#[tracing::instrument(level = "trace")]
pub async fn login(
    auth_session: axum_login::AuthSession<crate::auth::Backend>,
    session: tower_sessions::Session,
    axum::Form(()): axum::Form<()>,
) -> Result<impl axum::response::IntoResponse, crate::web::Ise> {
    use axum::response::IntoResponse;
    let auth_url = oauth2::AuthUrl::new(AUTH_URL.to_string()).unwrap();
    let token_url = oauth2::TokenUrl::new(TOKEN_URL.to_string()).unwrap();
    let client_id = auth_session.backend.github_client_id.clone();
    let client_secret = auth_session.backend.github_client_secret.clone();
    let redirect_url = auth_session.backend.redirect_url.clone();
    let client = oauth2::basic::BasicClient::new(client_id)
        .set_client_secret(client_secret)
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url);
    let (auth_url, csrf_state) = client.authorize_url(oauth2::CsrfToken::new_random).url();
    session.insert(CSRF_STATE_KEY, csrf_state.secret()).await?;
    session.save().await?;
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

#[derive(Debug, Clone)]
pub struct ClientToken {
    pub client_id: oauth2::ClientId,
    pub client_secret: oauth2::ClientSecret,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub user_id: i64,
}

impl axum_login::AuthUser for User {
    type Id = i64;

    fn id(&self) -> Self::Id {
        self.user_id
    }

    fn session_auth_hash(&self) -> &[u8] {
        // ヤケクソ
        unsafe {
            std::slice::from_raw_parts(
                &self.user_id as *const i64 as *const u8,
                std::mem::size_of::<i64>(),
            )
        }
    }
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub code: String,
    pub old_state: oauth2::CsrfToken,
    pub new_state: oauth2::CsrfToken,
    pub user: Option<User>,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct BackendError(#[from] pub anyhow::Error);

const AUTH_URL: &str = "https://github.com/login/oauth/authorize";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Debug, Clone)]
pub struct Backend {
    db: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<i64, String>>>,
    github_client_id: oauth2::ClientId,
    github_client_secret: oauth2::ClientSecret,
    redirect_url: oauth2::RedirectUrl,
}

impl Backend {
    pub fn new(client_token: ClientToken, redirect_url: oauth2::RedirectUrl) -> Self {
        let db = Default::default();
        let github_client_id = client_token.client_id;
        let github_client_secret = client_token.client_secret;
        Self {
            db,
            github_client_id,
            github_client_secret,
            redirect_url,
        }
    }
}

impl axum_login::AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = BackendError;

    #[tracing::instrument]
    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        use oauth2::TokenResponse;
        // Ensure the CSRF state has not been tampered with.
        if creds.old_state.secret() != creds.new_state.secret() {
            return Ok(None);
        };
        let auth_url = oauth2::AuthUrl::new(AUTH_URL.to_string()).unwrap();
        let token_url = oauth2::TokenUrl::new(TOKEN_URL.to_string()).unwrap();
        let client = oauth2::basic::BasicClient::new(self.github_client_id.clone())
            .set_client_secret(self.github_client_secret.clone())
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(self.redirect_url.clone());

        // Process authorization code, expecting a token response back.
        let token_res = client
            .exchange_code(oauth2::AuthorizationCode::new(creds.code.clone()))
            .request_async(&oauth2::reqwest::Client::new())
            .await
            .map_err(anyhow::Error::from)?;

        // https://docs.github.com/ja/rest/users/users?apiVersion=2022-11-28#get-the-authenticated-user
        #[derive(Debug, serde::Deserialize)]
        struct GithubUserInfo {
            // legokichi
            login: String,
            // github unique id
            id: i64,
        }

        let res = reqwest::Client::new()
            .get("https://api.github.com/user")
            .header(
                axum::http::header::AUTHORIZATION.as_str(),
                format!("Bearer {}", token_res.access_token().secret()),
            )
            .header(axum::http::header::USER_AGENT.as_str(), "axum-login")
            .send()
            .await;
        let user_info = res
            .map_err(anyhow::Error::from)?
            .text()
            .await
            .map_err(anyhow::Error::from)?;
        log::debug!("{}", user_info);
        let user_info =
            serde_json::from_str::<GithubUserInfo>(&user_info).map_err(anyhow::Error::from)?;

        // let mut db = self.db.acquire().await.map_err(anyhow::Error::from)?;
        if let Some(user) = creds.user {
            Ok(Some(user))
        } else {
            log::info!("signup: {:?}", user_info);
            self.db.lock().await.insert(user_info.id, user_info.login);
            let user = User {
                user_id: user_info.id,
            };
            Ok(Some(user))
        }
    }

    #[tracing::instrument]
    async fn get_user(
        &self,
        user_id: &axum_login::UserId<Self>,
    ) -> Result<Option<Self::User>, Self::Error> {
        let db = self.db.lock().await;
        let login = db.get(user_id);
        if let Some(_login) = login {
            let user = User { user_id: *user_id };
            dbg!(&user);
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }
}
