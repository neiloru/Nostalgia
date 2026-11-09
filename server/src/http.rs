//! Shared request plumbing: state, authentication extractors, JSON bodies and file responses.

use crate::auth::{ADMIN_COOKIE, Auth, bearer, cookie};
use crate::config::Settings;
use crate::db::Db;
use crate::error::{HttpError, HttpResult};
use crate::events::EventHub;
use crate::igdb::Igdb;
use crate::jobs::Jobs;
use crate::library::Library;
use crate::saves::Saves;
use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts, Query, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::response::{IntoResponse, Response};
use nostalgia_shared::User;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::services::ServeFile;

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub db: Db,
    pub auth: Arc<Auth>,
    pub library: Library,
    pub saves: Saves,
    pub events: Arc<EventHub>,
    pub jobs: Arc<Jobs>,
    pub igdb: Arc<Igdb>,
}

/// `X-Device` header (first 100 characters).
pub fn device(headers: &HeaderMap) -> Option<String> {
    headers.get("x-device").and_then(|v| v.to_str().ok()).map(|s| crate::util::js_slice(s, 100))
}

/// A signed-in user (`Authorization: Bearer` or `?t=`).
pub struct AuthUser {
    pub user: User,
    pub token: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let query = Query::<HashMap<String, String>>::try_from_uri(&parts.uri).map(|q| q.0).unwrap_or_default();
        let token = bearer(&parts.headers, &query);
        let user = token.as_deref().and_then(|t| state.auth.user_for_token(t));
        match (user, token) {
            (Some(user), Some(token)) => Ok(AuthUser { user, token }),
            _ => Err(HttpError::new(401, "Not authenticated")),
        }
    }
}

/// The admin cookie of the web interface.
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if state.auth.is_admin(cookie(&parts.headers, ADMIN_COOKIE).as_deref()) {
            Ok(Admin)
        } else {
            Err(HttpError::new(401, "Admin login required"))
        }
    }
}

/// A JSON body like `express.json()`: parsed only for JSON content types, `null` otherwise.
pub struct JsonBody(pub Value);

impl JsonBody {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k)
    }
}

impl<S: Send + Sync> FromRequest<S> for JsonBody {
    type Rejection = HttpError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let is_json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| {
                let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
                ct == "application/json" || ct.ends_with("+json")
            })
            .unwrap_or(false);
        let bytes = axum::body::to_bytes(req.into_body(), 2 * 1024 * 1024).await.map_err(|_| HttpError::new(413, "request entity too large"))?;
        if !is_json || bytes.is_empty() {
            return Ok(JsonBody(Value::Null));
        }
        serde_json::from_slice(&bytes).map(JsonBody).map_err(|e| HttpError::new(400, e.to_string()))
    }
}

/// Send a file with Range / Last-Modified support (pause / resume of downloads).
pub async fn send_file(path: &Path, req_headers: &HeaderMap, extra: &[(header::HeaderName, String)]) -> HttpResult<Response> {
    let mut req = Request::builder().method(Method::GET).uri("/").body(Body::empty()).map_err(HttpError::internal)?;
    for h in [header::RANGE, header::IF_RANGE, header::IF_MODIFIED_SINCE, header::IF_UNMODIFIED_SINCE] {
        if let Some(v) = req_headers.get(&h) {
            req.headers_mut().insert(h, v.clone());
        }
    }
    let res = ServeFile::new(path).oneshot(req).await.map_err(HttpError::internal)?;
    let mut res = res.into_response();
    for (k, v) in extra {
        if let Ok(v) = HeaderValue::from_str(v) {
            res.headers_mut().insert(k.clone(), v);
        }
    }
    Ok(res)
}

/// Content-Disposition for a download, like Express' `attachment; filename*=UTF-8''…`.
pub fn attachment(name: &str) -> (header::HeaderName, String) {
    (header::CONTENT_DISPOSITION, format!("attachment; filename*=UTF-8''{}", crate::util::encode_uri_component(name)))
}
