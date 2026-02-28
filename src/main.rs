use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::Router;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use miniminio::admin;
use miniminio::admin::metrics::Metrics;
use miniminio::auth;
use miniminio::config::Config;
use miniminio::s3;
use miniminio::storage::cleanup;
use miniminio::storage::Storage;
use miniminio::sync::{self, SyncManager, SyncStatus};
use miniminio::ui;
use miniminio::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::parse();

    info!(
        data_dir = %config.data_dir.display(),
        port = config.port,
        "Starting MiniMinio"
    );

    // Initialize storage
    let mut storage = Storage::new(config.data_dir.clone())
        .await
        .expect("Failed to initialize storage");

    let metrics = Metrics::new();

    // Set up upstream sync if configured
    let sync_config = config.sync_config();
    let sync_status = if let Some(ref sc) = sync_config {
        SyncStatus::new(true, &sc.endpoint)
    } else {
        SyncStatus::new(false, "")
    };

    if let Some(ref sc) = sync_config {
        let (tx, rx) = sync::channel();
        storage = storage.with_sync(tx, sync_status.events_pending.clone());

        let storage_arc = Arc::new(storage);

        let manager = SyncManager::new(sc.clone(), storage_arc.clone(), rx, sync_status.clone());
        tokio::spawn(manager.run());

        info!(
            endpoint = %sc.endpoint,
            bucket_prefix = %sc.bucket_prefix,
            "Upstream sync enabled"
        );

        build_and_serve(config, storage_arc, metrics, sync_status).await;
    } else {
        let storage_arc = Arc::new(storage);
        info!("Upstream sync disabled (no --sync-endpoint)");
        build_and_serve(config, storage_arc, metrics, sync_status).await;
    }
}

async fn build_and_serve(
    config: Config,
    storage: Arc<Storage>,
    metrics: Arc<Metrics>,
    sync_status: Arc<SyncStatus>,
) {
    // Spawn background cleanup
    cleanup::spawn_cleanup_task(storage.clone(), 1, config.multipart_expiry_hours);

    let state = AppState {
        config: Arc::new(config.clone()),
        storage,
        metrics,
        sync_status,
    };

    let app = Router::new()
        .merge(s3::router())
        .merge(admin::router())
        .merge(ui::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024 * 1024))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid listen address");

    info!(%addr, "MiniMinio listening");
    info!("  S3 API:  http://{addr}");
    info!("  Web UI:  http://{addr}/ui");
    info!("  Health:  http://{addr}/health");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server failed");
}
