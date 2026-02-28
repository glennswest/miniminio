pub mod bucket;
pub mod error;
pub mod object;

use axum::routing::{any, delete, get, head, post, put};
use axum::Router;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        // List buckets
        .route("/", get(bucket::list_buckets))
        // Bucket operations
        .route(
            "/:bucket",
            get(bucket::get_bucket)
                .put(bucket::create_bucket)
                .delete(bucket::delete_bucket)
                .head(bucket::head_bucket)
                .post(bucket::post_bucket),
        )
        // Object operations
        .route(
            "/:bucket/*key",
            get(object::get_object)
                .put(object::put_object)
                .delete(object::delete_object)
                .head(object::head_object)
                .post(object::post_object),
        )
}
