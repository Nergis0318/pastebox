mod config;
mod errors;
mod handlers;
mod middleware;
mod storage;
mod templates;
mod util;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router, middleware as axum_middleware,
    routing::{get, post},
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use config::Config;
use storage::admin::AdminStore;
use storage::lock::LockManager;
use storage::paste::PasteStore;

pub struct AppState {
    pub config: Config,
    pub pastes: PasteStore,
    pub admin: AdminStore,
    pub locks: LockManager,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(
        addr = %config.listen_addr,
        data_dir = %config.data_dir.display(),
        expire_days = config.expire_days,
        "starting pastebox"
    );

    let pastes = PasteStore::new(&config)?;
    let admin = AdminStore::new(&config)?;
    let locks = LockManager::new();

    let bind_addr = config.listen_addr;
    let state = Arc::new(AppState {
        config,
        pastes,
        admin,
        locks,
    });

    // Start cleanup background task
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            if let Err(e) = cleanup_state.pastes.cleanup_expired() {
                tracing::error!(?e, "cleanup error");
            }
        }
    });

    // Admin routes (require auth)
    let admin_routes = Router::new()
        .route("/admin", get(handlers::admin::list))
        .route("/admin/delete", post(handlers::admin::admin_delete))
        .route("/admin/logout", get(handlers::admin::logout))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_admin,
        ));

    // Public admin routes (no auth)
    let public_admin = Router::new()
        .route(
            "/admin/setup",
            get(handlers::admin::setup_form).post(handlers::admin::setup_submit),
        )
        .route(
            "/admin/login",
            get(handlers::admin::login_form).post(handlers::admin::login_submit),
        );

    // Main routes
    let app = Router::new()
        .route(
            "/",
            get(handlers::index::get)
                .post(handlers::upload::handle)
                .put(handlers::upload::handle),
        )
        .route("/{id}", get(handlers::view::get))
        .merge(admin_routes)
        .merge(public_admin)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("listening on {}", bind_addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutting down");
}
