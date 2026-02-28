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
}
