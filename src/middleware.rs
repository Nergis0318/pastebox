use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use std::sync::Arc;

use crate::AppState;

const SESSION_COOKIE: &str = "pastebox_admin";

pub async fn require_admin(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = request
        .headers()
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                if c.starts_with(SESSION_COOKIE) {
                    let val = &c[SESSION_COOKIE.len() + 1..];
                    Some(val.to_string())
                } else {
                    None
                }
            })
        });

    if let Some(t) = token {
        let valid = state.admin.validate_session(&t).unwrap_or(false);
        if valid {
            return Ok(next.run(request).await);
        }
    }

    let is_browser = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|h| h.to_str().ok())
        .map(|a| a.contains("text/html"))
        .unwrap_or(false);

    if is_browser {
        Ok(Redirect::to("/admin/login").into_response())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub fn base_url_from_headers_map(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok());
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok());
    let forwarded_host = headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok());

    crate::util::request_base_url(scheme, host, forwarded_proto, forwarded_host)
}
