use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Clone, Debug)]
#[command(name = "miniminio", about = "Minimal S3-compatible object storage")]
pub struct Config {
    /// Data directory for object storage
    #[arg(long, env = "MINIMINIO_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

    /// Listen port
    #[arg(long, env = "MINIMINIO_PORT", default_value_t = 9000)]
    pub port: u16,

    /// Listen address
    #[arg(long, env = "MINIMINIO_HOST", default_value = "0.0.0.0")]
    pub host: String,

    /// Access key (username)
    #[arg(long, env = "MINIMINIO_ACCESS_KEY", default_value = "minioadmin")]
    pub access_key: String,

    /// Secret key (password)
    #[arg(long, env = "MINIMINIO_SECRET_KEY", default_value = "minioadmin")]
    pub secret_key: String,

    /// S3 region name
    #[arg(long, env = "MINIMINIO_REGION", default_value = "us-east-1")]
    pub region: String,

    /// Hours before incomplete multipart uploads are cleaned up
    #[arg(long, env = "MINIMINIO_MULTIPART_EXPIRY", default_value_t = 24)]
    pub multipart_expiry_hours: u64,

    // --- Upstream sync ---

    /// Upstream S3 endpoint to replicate to (e.g. http://master-minio:9000).
    /// If not set, sync is disabled.
    #[arg(long, env = "MINIMINIO_SYNC_ENDPOINT")]
    pub sync_endpoint: Option<String>,

    /// Upstream access key
    #[arg(long, env = "MINIMINIO_SYNC_ACCESS_KEY")]
    pub sync_access_key: Option<String>,

    /// Upstream secret key
    #[arg(long, env = "MINIMINIO_SYNC_SECRET_KEY")]
    pub sync_secret_key: Option<String>,

    /// Upstream region
    #[arg(long, env = "MINIMINIO_SYNC_REGION", default_value = "us-east-1")]
    pub sync_region: String,

    /// Prefix added to bucket names on the upstream (e.g. "edge01-")
    #[arg(long, env = "MINIMINIO_SYNC_BUCKET_PREFIX", default_value = "")]
    pub sync_bucket_prefix: String,
}

impl Config {
    /// Returns true if upstream sync is configured.
    pub fn sync_enabled(&self) -> bool {
        self.sync_endpoint.is_some()
    }

    /// Build SyncConfig from the CLI config. Returns None if sync is disabled.
    pub fn sync_config(&self) -> Option<crate::sync::SyncConfig> {
        let endpoint = self.sync_endpoint.as_ref()?;
        Some(crate::sync::SyncConfig {
            endpoint: endpoint.clone(),
            access_key: self
                .sync_access_key
                .clone()
                .unwrap_or_else(|| self.access_key.clone()),
            secret_key: self
                .sync_secret_key
                .clone()
                .unwrap_or_else(|| self.secret_key.clone()),
            region: self.sync_region.clone(),
            bucket_prefix: self.sync_bucket_prefix.clone(),
        })
    }
}
