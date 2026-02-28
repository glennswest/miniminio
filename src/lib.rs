pub mod admin;
pub mod auth;
pub mod config;
pub mod s3;
pub mod storage;
pub mod ui;
pub mod xml;

use std::sync::Arc;

use admin::metrics::Metrics;
use config::Config;
use storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub storage: Arc<Storage>,
    pub metrics: Arc<Metrics>,
}
