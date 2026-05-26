mod config;
mod errors;
mod handlers;
mod middleware;
mod storage;
mod templates;
mod util;

use axum::Router;
use config::Config;
use std::net::SocketAddr;
use storage::lock::LockManager;
use storage::paste::PasteStore;
use storage::admin::AdminStore;
use tracing_subscriber::EnvFilter;

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
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(addr = %config.listen_addr, data_dir = %config.data_dir.display(), "starting pastebox");

    let pastes = PasteStore::new(&config)?;
    let admin = AdminStore::new(&config)?;
    let locks = LockManager::new();

    let state = std::sync::Arc::new(AppState { config, pastes, admin, locks });

    let cleanup_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            if let Err(e) = cleanup_state.pastes.cleanup_expired() {
                tracing::error!(?e, "cleanup error");
            }
        }
    });

    let app = Router::new();
    // Routes added in later tasks

    let listener = tokio::net::TcpListener::bind(&state.config.listen_addr).await?;
    tracing::info!("listening on {}", state.config.listen_addr);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}
