use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Internal failure. The details go to the server log only; the client gets an
/// opaque id that can be matched against that log entry.
#[derive(Debug)]
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let error_id = format!("{:016x}", getrandom::u64().unwrap_or_default());
        tracing::error!(error_id = %error_id, error = ?self.0, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("internal server error (id: {error_id})"),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
