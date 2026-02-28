use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct Metrics {
    pub requests_total: AtomicU64,
    pub requests_get: AtomicU64,
    pub requests_put: AtomicU64,
    pub requests_delete: AtomicU64,
    pub requests_head: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub errors_total: AtomicU64,
    pub bucket_stats: RwLock<HashMap<String, BucketMetrics>>,
    pub start_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct BucketMetrics {
    pub requests: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            requests_total: AtomicU64::new(0),
            requests_get: AtomicU64::new(0),
            requests_put: AtomicU64::new(0),
            requests_delete: AtomicU64::new(0),
            requests_head: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            bucket_stats: RwLock::new(HashMap::new()),
            start_time: chrono::Utc::now(),
        })
    }

    pub fn inc_request(&self, method: &str) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        match method {
            "GET" => self.requests_get.fetch_add(1, Ordering::Relaxed),
            "PUT" => self.requests_put.fetch_add(1, Ordering::Relaxed),
            "DELETE" => self.requests_delete.fetch_add(1, Ordering::Relaxed),
            "HEAD" => self.requests_head.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }

    pub fn add_bytes_in(&self, n: u64) {
        self.bytes_in.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_bytes_out(&self, n: u64) {
        self.bytes_out.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_error(&self) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn inc_bucket_request(&self, bucket: &str, bytes_in: u64, bytes_out: u64) {
        let mut stats = self.bucket_stats.write().await;
        let entry = stats.entry(bucket.to_string()).or_default();
        entry.requests += 1;
        entry.bytes_in += bytes_in;
        entry.bytes_out += bytes_out;
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.requests_total.load(Ordering::Relaxed),
            requests_get: self.requests_get.load(Ordering::Relaxed),
            requests_put: self.requests_put.load(Ordering::Relaxed),
            requests_delete: self.requests_delete.load(Ordering::Relaxed),
            requests_head: self.requests_head.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            uptime_secs: (chrono::Utc::now() - self.start_time).num_seconds() as u64,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshot {
    pub requests_total: u64,
    pub requests_get: u64,
    pub requests_put: u64,
    pub requests_delete: u64,
    pub requests_head: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub errors_total: u64,
    pub uptime_secs: u64,
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
