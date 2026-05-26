use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};

use crate::AppState;
use crate::errors::AppError;

pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let use_password = headers
        .get("usepassword")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "true")
        .unwrap_or(false);

    let data_policy = headers
        .get("data-policy")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            if v == "permanent" {
                "permanent"
            } else {
                "temporary"
            }
        })
        .unwrap_or("temporary");

    let content = body.to_vec();
    let detected_type = crate::util::detect_content_type(&content, Some(content_type));

    if content.is_empty() {
        return Err(AppError::BadRequest("no content provided".into()));
    }

    let password = if use_password {
        Some(crate::util::generate_password())
    } else {
        None
    };

    let delete_token = crate::util::random_token(32);

    let meta = state.pastes.create(
        &content,
        &detected_type,
        password.as_deref(),
        &delete_token,
        data_policy,
    )?;

    let base_url = crate::middleware::base_url_from_headers_map(&headers);
    let paste_url = format!("{}/{}", base_url, meta.id);
    let delete_url = format!("{}?delete={}", paste_url, delete_token);

    let mut response = String::new();
    response.push_str(&format!("{}\n", paste_url));
    if let Some(pw) = password {
        response.push_str(&format!("password: {}\n", pw));
    }
    response.push_str(&format!("expires: {}\n", meta.expires_at));
    response.push_str(&format!("delete: {}\n", delete_url));

    Ok((StatusCode::OK, response))
}
