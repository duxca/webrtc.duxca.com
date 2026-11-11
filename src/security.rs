use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::app::{AppState, HTTPS_ONLY};

/// Rejects state-changing requests that a browser sent from another origin.
/// Requests without `Sec-Fetch-Site` and `Origin` come from non-browser
/// clients, which cannot ride on a victim's cookies, so they pass.
pub fn is_same_origin_request(method: &Method, headers: &HeaderMap, public_origin: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    if let Some(site) = headers.get("sec-fetch-site") {
        return site == "same-origin";
    }
    match headers.get(header::ORIGIN) {
        Some(origin) => origin == public_origin,
        None => true,
    }
}

pub async fn require_same_origin(
    State(app): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if !is_same_origin_request(req.method(), req.headers(), &app.public_origin) {
        tracing::info!(method = %req.method(), path = %req.uri().path(), "rejected cross-origin request");
        return (StatusCode::FORBIDDEN, "cross-origin request rejected").into_response();
    }
    next.run(req).await
}

pub fn content_security_policy(login_form_target: &str) -> anyhow::Result<HeaderValue> {
    Ok(HeaderValue::from_str(&format!(
        "default-src 'none'; script-src 'self'; connect-src 'self'; img-src 'self'; \
         style-src 'self'; base-uri 'none'; form-action 'self' {login_form_target}; \
         frame-ancestors 'none'"
    ))?)
}

pub async fn security_headers(State(app): State<AppState>, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    let fixed: [(HeaderName, &'static str); 6] = [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "no-referrer"),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
        (
            HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
        (
            HeaderName::from_static("permissions-policy"),
            "display-capture=(self), camera=(), microphone=(), geolocation=()",
        ),
    ];
    for (name, value) in fixed {
        headers.insert(name, HeaderValue::from_static(value));
    }
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        app.content_security_policy.clone(),
    );
    if HTTPS_ONLY {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://webrtc.duxca.com";

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| (HeaderName::from_static(k), HeaderValue::from_static(v)))
            .collect()
    }

    #[test]
    fn same_origin_rules() {
        let post = Method::POST;
        assert!(is_same_origin_request(
            &Method::GET,
            &headers(&[("sec-fetch-site", "cross-site")]),
            ORIGIN
        ));
        assert!(is_same_origin_request(
            &post,
            &headers(&[("sec-fetch-site", "same-origin")]),
            ORIGIN
        ));
        assert!(!is_same_origin_request(
            &post,
            &headers(&[("sec-fetch-site", "same-site")]),
            ORIGIN
        ));
        assert!(!is_same_origin_request(
            &post,
            &headers(&[("sec-fetch-site", "cross-site")]),
            ORIGIN
        ));
        assert!(is_same_origin_request(
            &post,
            &headers(&[("origin", ORIGIN)]),
            ORIGIN
        ));
        assert!(!is_same_origin_request(
            &post,
            &headers(&[("origin", "https://evil.duxca.com")]),
            ORIGIN
        ));
        assert!(!is_same_origin_request(
            &post,
            &headers(&[("origin", "null")]),
            ORIGIN
        ));
        assert!(is_same_origin_request(&post, &HeaderMap::new(), ORIGIN));
    }
}
