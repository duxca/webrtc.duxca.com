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
    client: oauth2::basic::BasicClient,
}

impl Backend {
    pub fn new(client_token: ClientToken, redirect_url: oauth2::RedirectUrl) -> Self {
        let auth_url = oauth2::AuthUrl::new(AUTH_URL.to_string()).unwrap();
        let token_url = oauth2::TokenUrl::new(TOKEN_URL.to_string()).unwrap();

        let client = oauth2::basic::BasicClient::new(
            client_token.client_id,
            Some(client_token.client_secret),
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(redirect_url);
        let db = Default::default();
        Self { db, client }
    }

    pub fn authorize_url(&self) -> (oauth2::url::Url, oauth2::CsrfToken) {
        self.client
            .authorize_url(oauth2::CsrfToken::new_random)
            .url()
    }
}

#[async_trait::async_trait]
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

        // Process authorization code, expecting a token response back.
        let token_res = self
            .client
            .exchange_code(oauth2::AuthorizationCode::new(creds.code))
            .request_async(|o| async {
                let res = oauth2::reqwest::async_http_client(o).await;
                log::debug!("{res:?}");
                if let Ok(ref res) = res {
                    log::debug!("{:?}", std::str::from_utf8(&res.body));
                }
                res
            })
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
