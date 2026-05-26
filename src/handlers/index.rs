use std::sync::Arc;

use askama::Template;
use axum::{
    extract::State,
    http::HeaderMap,
    response::Html,
};

use crate::templates::IndexTemplate;
use crate::AppState;

pub async fn get(
    State(_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, crate::errors::AppError> {
    let base_url = crate::middleware::base_url_from_headers_map(&headers);
    let template = IndexTemplate { base_url };
    let html = template.render().map_err(|e| {
        crate::errors::AppError::Internal(e.into())
    })?;
    Ok(Html(html))
}
