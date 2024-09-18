// mod api;
mod auth;
// mod db;
mod web;

#[derive(serde::Deserialize, Debug)]
struct Config {
    host_addr: String,
    github_client_id: oauth2::ClientId,
    github_client_secret: oauth2::ClientSecret,
    redirect_url: oauth2::RedirectUrl,
    local_client_id: oauth2::ClientId,
    local_client_secret: oauth2::ClientSecret,
    local_redirect_url: oauth2::RedirectUrl,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    shadow_rs::shadow!(build);
    dotenvy::dotenv().ok();
    //env_logger::init();
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .with_thread_names(true)
        .with_thread_ids(true)
        .init();
    let config = envy::from_env::<Config>()?;
    log::debug!("config: {:#?}", config);

    // ここで remote db に対して migrate する
    // sqlx::migrate!().run(&pool).await?;

    let session_store = tower_sessions::MemoryStore::default();

    // cookie のセッションの設定
    let mut session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        // oauth でリダイレクトするときにStrict だとエラーになる
        //.with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_same_site(tower_sessions::cookie::SameSite::Strict)
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            std::time::Duration::from_secs(600).try_into()?,
        ));
    if cfg!(not(feature = "local")) {
        // 本番環境で有効にする
        session_layer = session_layer.with_secure(true).with_http_only(true);
    }
    let (client_token, redirect_url) = if cfg!(feature = "local") {
        (
            crate::auth::ClientToken {
                client_id: config.local_client_id.clone(),
                client_secret: config.local_client_secret.clone(),
            },
            config.local_redirect_url,
        )
    } else {
        (
            crate::auth::ClientToken {
                client_id: config.github_client_id.clone(),
                client_secret: config.github_client_secret.clone(),
            },
            config.redirect_url,
        )
    };
    let backend = crate::auth::Backend::new(client_token, redirect_url);

    let app = axum::Router::new()
        .route(
            "/version",
            axum::routing::get(|| async { build::CLAP_LONG_VERSION }),
        )
        .route("/login", axum::routing::post(crate::web::login))
        .route("/logout", axum::routing::post(crate::web::logout))
        .route("/oauth/callback", axum::routing::get(crate::web::callback))
        .layer(axum_login::AuthManagerLayerBuilder::new(backend, session_layer).build())
        .layer(
            tower_http::cors::CorsLayer::very_permissive(), // .allow_credentials(true)
                                                            // .allow_methods(tower_http::cors::Any)
                                                            // .allow_origin(tower_http::cors::Any),
        )
        .nest_service("/", tower_http::services::ServeDir::new("dist"))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::compression::CompressionLayer::new());

    let listener = tokio::net::TcpListener::bind(config.host_addr).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
