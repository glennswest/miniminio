use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::Router;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use miniminio::admin;
use miniminio::auth;
use miniminio::config::Config;
use miniminio::s3;
use miniminio::storage::cleanup;
use miniminio::storage::Storage;
use miniminio::ui;
use miniminio::admin::metrics::Metrics;
use miniminio::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing
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
    let storage = Storage::new(config.data_dir.clone())
        .await
        .expect("Failed to initialize storage");
    let storage = Arc::new(storage);

    let metrics = Metrics::new();

    let state = AppState {
        config: Arc::new(config.clone()),
        storage: storage.clone(),
        metrics: metrics.clone(),
    };

    // Spawn background cleanup
    cleanup::spawn_cleanup_task(
        storage.clone(),
        1, // check every hour
        config.multipart_expiry_hours,
    );

    // Build router
    let app = Router::new()
        // S3 API routes (with auth middleware)
        .merge(s3::router())
        // Admin + UI routes
        .merge(admin::router())
        .merge(ui::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024 * 1024)) // 5 GB
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

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}
