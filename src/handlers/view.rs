use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::errors::AppError;
use crate::templates::ViewTemplate;

#[derive(Deserialize, Default)]
pub struct ViewParams {
    pub raw: Option<String>,
    pub password: Option<String>,
    pub delete: Option<String>,
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<ViewParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if !crate::storage::paste::valid_id(&id) {
        return Err(AppError::NotFound);
    }

    // Check for deletion first
    if let Some(token) = params.delete {
        state.pastes.delete(&id, &token)?;
        return Ok((StatusCode::OK, "deleted\n").into_response());
    }

    let (meta, content) = state.pastes.open(&id)?;

    // Check password
    if let Some(ref pw_hash) = meta.password_hash {
        let provided = params.password.as_deref().or_else(|| {
            headers
                .get("paste-password")
                .and_then(|v| v.to_str().ok())
        });

        match provided {
            Some(pw) => {
                let hash = hex::encode(Sha256::digest(pw.as_bytes()));
                if hash != *pw_hash {
                    return Err(AppError::Forbidden);
                }
            }
            None => return Err(AppError::Forbidden),
        }
    }

    let is_text = crate::util::looks_like_text(&content);
    let is_raw = params.raw.as_deref() == Some("1");
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    let is_browser = crate::util::is_browser_request(user_agent);

    if is_raw || !is_browser {
        // Raw response - return content directly
        let mut resp = Response::new(axum::body::Body::from(content));
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            meta.content_type.parse().unwrap_or(header::HeaderValue::from_static("text/plain")),
        );
        if !is_text && !is_raw && is_browser {
            resp.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", id))
                    .unwrap_or(header::HeaderValue::from_static("attachment")),
            );
        }
        Ok(resp)
    } else if is_text {
        // HTML viewer
        let content_str = String::from_utf8_lossy(&content).to_string();
        let size = format_size(meta.size);
        let template = ViewTemplate {
            id,
            content: content_str,
            content_type: meta.content_type,
            size,
            expires_at: meta.expires_at,
            is_text: true,
        };
        let html = template.render().map_err(|e| AppError::Internal(e.into()))?;
        Ok(Html(html).into_response())
    } else {
        // Binary content - download
        let mut resp = Response::new(axum::body::Body::from(content));
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            meta.content_type.parse().unwrap_or(header::HeaderValue::from_static("application/octet-stream")),
        );
        resp.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", id))
                .unwrap_or(header::HeaderValue::from_static("attachment")),
        );
        Ok(resp)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
