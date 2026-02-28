pub mod metrics;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;

use crate::storage::cleanup;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/ui/api/metrics", get(get_metrics))
        .route("/ui/api/buckets", get(list_buckets_json))
        .route("/ui/api/buckets/:bucket/stats", get(bucket_stats_json))
        .route("/ui/api/server-info", get(server_info))
        .route("/ui/api/cleanup", post(run_cleanup))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let snap = state.metrics.snapshot();
    Json(snap)
}

async fn list_buckets_json(State(state): State<AppState>) -> impl IntoResponse {
    match state.storage.list_buckets().await {
        Ok(buckets) => {
            let mut result = Vec::new();
            for b in &buckets {
                let stats = state.storage.bucket_stats(&b.name).await.unwrap_or_default();
                result.push(json!({
                    "name": b.name,
                    "created": b.created,
                    "objects": stats.object_count,
                    "size": stats.total_size,
                    "size_human": metrics::format_bytes(stats.total_size),
                }));
            }
            Json(json!({ "buckets": result })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn bucket_stats_json(
    State(state): State<AppState>,
    axum::extract::Path(bucket): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.storage.bucket_stats(&bucket).await {
        Ok(stats) => Json(json!({
            "bucket": bucket,
            "objects": stats.object_count,
            "size": stats.total_size,
            "size_human": metrics::format_bytes(stats.total_size),
        }))
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn server_info(State(state): State<AppState>) -> impl IntoResponse {
    let total = state.storage.total_stats().await.unwrap_or_default();
    let buckets = state.storage.list_buckets().await.unwrap_or_default();
    let snap = state.metrics.snapshot();

    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": snap.uptime_secs,
        "buckets": buckets.len(),
        "objects": total.object_count,
        "total_size": total.total_size,
        "total_size_human": metrics::format_bytes(total.total_size),
        "requests_total": snap.requests_total,
        "bytes_in": snap.bytes_in,
        "bytes_out": snap.bytes_out,
    }))
}

async fn run_cleanup(State(state): State<AppState>) -> impl IntoResponse {
    let mp = cleanup::cleanup_multipart(&state.storage, state.config.multipart_expiry_hours).await;
    let orphan = cleanup::cleanup_orphaned_metadata(&state.storage).await;

    Json(json!({
        "multipart_cleaned": mp,
        "orphans_cleaned": orphan,
    }))
}
