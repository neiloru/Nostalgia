use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// An error with an HTTP status, answered as `{ "error": message }` like the Node.js server.
#[derive(Debug, Clone)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

impl HttpError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        HttpError { status, message: message.into() }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        HttpError::new(404, message)
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        HttpError::new(500, e.to_string())
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HttpError {}

impl From<rusqlite::Error> for HttpError {
    fn from(e: rusqlite::Error) -> Self {
        HttpError::internal(e)
    }
}

impl From<std::io::Error> for HttpError {
    fn from(e: std::io::Error) -> Self {
        HttpError::internal(e)
    }
}

impl From<anyhow::Error> for HttpError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<HttpError>() {
            Ok(h) => h,
            Err(e) => HttpError::internal(e),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        if self.status >= 500 {
            crate::log::error(&self.message);
        }
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(json!({ "error": self.message }))).into_response()
    }
}

pub type HttpResult<T> = Result<T, HttpError>;
