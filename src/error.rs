//! One error type for the handlers: anything unexpected becomes a `500` with the
//! cause logged, so a render failure never looks like a spider failure.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct AppError(anyhow::Error);

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        AppError(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("internal error: {:#}", self.0),
        )
            .into_response()
    }
}
