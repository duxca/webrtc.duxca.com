mod app;
mod auth;
mod error;
mod rate_limit;
mod security;
mod session_store;
mod signaling;
#[cfg(test)]
mod tests;
mod web;

#[cfg(all(feature = "local", not(debug_assertions)))]
compile_error!(
    "the `local` feature turns off Secure cookies and HSTS; never build a release with it"
);

#[derive(serde::Deserialize)]
struct Config {
    host_addr: String,
    github_client_id: oauth2::ClientId,
    github_client_secret: oauth2::ClientSecret,
    redirect_url: oauth2::RedirectUrl,
    /// Comma-separated GitHub user ids. Unset or empty keeps sign-in open.
    allowed_github_ids: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .with_thread_names(true)
        .with_thread_ids(true)
        .init();
    let config = envy::from_env::<Config>()?;

    let allowlist = auth::Allowlist::parse(config.allowed_github_ids.as_deref())?;
    match allowlist.allowed_count() {
        Some(count) => tracing::info!(count, "sign-in restricted to ALLOWED_GITHUB_IDS"),
        None => tracing::info!("sign-in open to every GitHub account"),
    }
    let oauth = auth::OAuthSettings {
        client_id: config.github_client_id,
        client_secret: config.github_client_secret,
        redirect_url: config.redirect_url,
        endpoints: auth::GitHubEndpoints::github_com(),
    };
    let app = app::build(oauth, allowlist, app::Settings::default())?;
    app.sessions
        .spawn_purger(std::time::Duration::from_secs(60));

    let listener = tokio::net::TcpListener::bind(&config.host_addr).await?;
    axum::serve(listener, app.router).await?;
    Ok(())
}
