use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("paste has expired")]
    Gone,
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body): (_, String) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found\n".into()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden\n".into()),
            AppError::Gone => (StatusCode::GONE, "gone\n".into()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized\n".into()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, format!("{msg}\n")),
            AppError::Internal(e) => {
                tracing::error!(?e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n".into())
            }
        };
        (status, body).into_response()
    }
}
