use std::sync::Arc;

use askama::Template;
use axum::{
    Form,
    extract::State,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use crate::AppState;
use crate::errors::AppError;
use crate::storage::admin::AdminPasteItem;
use crate::templates::{AdminListTemplate, AdminLoginTemplate, AdminSetupTemplate};

const SESSION_COOKIE: &str = "pastebox_admin";

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct SetupForm {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct DeleteForm {
    id: String,
}

pub async fn list(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let pastes = state.pastes.list_pastes()?;
    let items: Vec<AdminPasteItem> = pastes
        .into_iter()
        .map(|m| AdminPasteItem {
            id: m.id,
            created_at: m.created_at,
            expires_at: m.expires_at,
            data_policy: m.data_policy,
            size: m.size,
            content_type: m.content_type,
            protected: m.password_hash.is_some(),
        })
        .collect();

    let template = AdminListTemplate { pastes: items };
    let html = template
        .render()
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Html(html))
}

pub async fn setup_form(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    if state.admin.admin_exists()? {
        return Err(AppError::Forbidden);
    }
    let template = AdminSetupTemplate { error: None };
    let html = template
        .render()
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Html(html).into_response())
}

pub async fn setup_submit(
    State(state): State<Arc<AppState>>,
    Form(form): Form<SetupForm>,
) -> Result<Response, AppError> {
    if state.admin.admin_exists()? {
        return Err(AppError::Forbidden);
    }

    if form.username.is_empty() || form.password.is_empty() {
        let template = AdminSetupTemplate {
            error: Some("Username and password required".into()),
        };
        let html = template
            .render()
            .map_err(|e| AppError::Internal(e.into()))?;
        return Ok(Html(html).into_response());
    }

    state.admin.create_admin(&form.username, &form.password)?;

    let token = state.admin.create_session()?;
    let cookie = format!("{SESSION_COOKIE}={token}; Path=/admin; HttpOnly; SameSite=Lax");

    let mut resp = Redirect::to("/admin").into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    Ok(resp)
}

pub async fn login_form() -> impl IntoResponse {
    let template = AdminLoginTemplate { error: None };
    Html(template.render().unwrap())
}

pub async fn login_submit(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let ok = state
        .admin
        .authenticate_admin(&form.username, &form.password)?;
    if !ok {
        let template = AdminLoginTemplate {
            error: Some("Invalid username or password".into()),
        };
        let html = template
            .render()
            .map_err(|e| AppError::Internal(e.into()))?;
        return Ok(Html(html).into_response());
    }

    let token = state.admin.create_session()?;
    let cookie = format!("{SESSION_COOKIE}={token}; Path=/admin; HttpOnly; SameSite=Lax");

    let mut resp = Redirect::to("/admin").into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    Ok(resp)
}

pub async fn logout(
    headers: axum::http::HeaderMap,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let token = headers
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                if c.starts_with(SESSION_COOKIE) {
                    Some(c[SESSION_COOKIE.len() + 1..].to_string())
                } else {
                    None
                }
            })
        });

    if let Some(t) = token {
        let _ = state.admin.delete_session(&t);
    }

    let cookie = format!("{SESSION_COOKIE}=; Path=/admin; HttpOnly; SameSite=Lax; Max-Age=0");
    let mut resp = Redirect::to("/admin/login").into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    resp
}

pub async fn admin_delete(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    match state.pastes.admin_delete(&form.id) {
        Ok(()) => Redirect::to("/admin").into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found\n").into_response(),
    }
}
