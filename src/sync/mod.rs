pub mod client;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::storage::Storage;
use client::S3Client;

/// Events emitted by storage operations for sync replication.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    CreateBucket {
        bucket: String,
    },
    DeleteBucket {
        bucket: String,
    },
    PutObject {
        bucket: String,
        key: String,
    },
    DeleteObject {
        bucket: String,
        key: String,
    },
}

/// Configuration for upstream sync.
#[derive(Clone, Debug)]
pub struct SyncConfig {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    /// Optional prefix added to bucket names on the remote.
    /// e.g. prefix "edge01-" maps local "mybucket" to remote "edge01-mybucket"
    pub bucket_prefix: String,
}

/// Sync status counters, readable from admin/UI.
#[derive(Debug)]
pub struct SyncStatus {
    pub events_sent: AtomicU64,
    pub events_failed: AtomicU64,
    pub events_pending: Arc<AtomicU64>,
    pub last_sync_epoch: AtomicU64,
    pub enabled: bool,
    pub endpoint: String,
}

impl SyncStatus {
    pub fn new(enabled: bool, endpoint: &str) -> Arc<Self> {
        Arc::new(Self {
            events_sent: AtomicU64::new(0),
            events_failed: AtomicU64::new(0),
            events_pending: Arc::new(AtomicU64::new(0)),
            last_sync_epoch: AtomicU64::new(0),
            enabled,
            endpoint: endpoint.into(),
        })
    }

    pub fn snapshot(&self) -> SyncSnapshot {
        SyncSnapshot {
            enabled: self.enabled,
            endpoint: self.endpoint.clone(),
            events_sent: self.events_sent.load(Ordering::Relaxed),
            events_failed: self.events_failed.load(Ordering::Relaxed),
            events_pending: self.events_pending.load(Ordering::Relaxed),
            last_sync_epoch: self.last_sync_epoch.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncSnapshot {
    pub enabled: bool,
    pub endpoint: String,
    pub events_sent: u64,
    pub events_failed: u64,
    pub events_pending: u64,
    pub last_sync_epoch: u64,
}

/// Creates the sync channel and returns (sender, receiver).
pub fn channel() -> (mpsc::UnboundedSender<SyncEvent>, mpsc::UnboundedReceiver<SyncEvent>) {
    mpsc::unbounded_channel()
}

/// Background sync manager that processes events and pushes to upstream.
pub struct SyncManager {
    client: S3Client,
    storage: Arc<Storage>,
    rx: mpsc::UnboundedReceiver<SyncEvent>,
    status: Arc<SyncStatus>,
    bucket_prefix: String,
}

impl SyncManager {
    pub fn new(
        config: SyncConfig,
        storage: Arc<Storage>,
        rx: mpsc::UnboundedReceiver<SyncEvent>,
        status: Arc<SyncStatus>,
    ) -> Self {
        let client = S3Client::new(
            &config.endpoint,
            &config.access_key,
            &config.secret_key,
            &config.region,
        );
        Self {
            client,
            storage,
            rx,
            status,
            bucket_prefix: config.bucket_prefix,
        }
    }

    fn remote_bucket(&self, local_bucket: &str) -> String {
        if self.bucket_prefix.is_empty() {
            local_bucket.to_string()
        } else {
            format!("{}{}", self.bucket_prefix, local_bucket)
        }
    }

    /// Run the sync loop. This blocks forever, processing events as they arrive.
    pub async fn run(mut self) {
        info!(
            endpoint = %self.status.endpoint,
            "Sync manager started, replicating to upstream"
        );

        while let Some(event) = self.rx.recv().await {
            self.status.events_pending.fetch_sub(1, Ordering::Relaxed);
            self.process_with_retry(event).await;
        }

        info!("Sync manager stopped (channel closed)");
    }

    async fn process_with_retry(&self, event: SyncEvent) {
        const MAX_RETRIES: u32 = 5;
        const BASE_DELAY: Duration = Duration::from_secs(1);

        for attempt in 0..=MAX_RETRIES {
            match self.process_event(&event).await {
                Ok(()) => {
                    self.status.events_sent.fetch_add(1, Ordering::Relaxed);
                    self.status.last_sync_epoch.store(
                        chrono::Utc::now().timestamp() as u64,
                        Ordering::Relaxed,
                    );
                    return;
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let delay = BASE_DELAY * 2u32.pow(attempt);
                        warn!(
                            attempt = attempt + 1,
                            max = MAX_RETRIES,
                            delay_ms = delay.as_millis() as u64,
                            error = %e,
                            event = ?event,
                            "Sync failed, retrying"
                        );
                        tokio::time::sleep(delay).await;
                    } else {
                        error!(
                            error = %e,
                            event = ?event,
                            "Sync failed after {MAX_RETRIES} retries, dropping event"
                        );
                        self.status.events_failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    async fn process_event(&self, event: &SyncEvent) -> Result<(), client::SyncError> {
        match event {
            SyncEvent::CreateBucket { bucket } => {
                let remote = self.remote_bucket(bucket);
                // Ensure bucket exists on remote
                if !self.client.head_bucket(&remote).await? {
                    self.client.create_bucket(&remote).await?;
                    info!(local = %bucket, remote = %remote, "Synced bucket creation");
                }
                Ok(())
            }
            SyncEvent::DeleteBucket { bucket } => {
                let remote = self.remote_bucket(bucket);
                self.client.delete_bucket(&remote).await?;
                info!(local = %bucket, remote = %remote, "Synced bucket deletion");
                Ok(())
            }
            SyncEvent::PutObject { bucket, key } => {
                let remote_bucket = self.remote_bucket(bucket);
                // Read from local storage
                match self.storage.get_object(bucket, key).await {
                    Ok((meta, data)) => {
                        // Ensure remote bucket exists
                        if !self.client.head_bucket(&remote_bucket).await.unwrap_or(false) {
                            self.client.create_bucket(&remote_bucket).await.ok();
                        }
                        self.client
                            .put_object(&remote_bucket, key, &data, &meta.content_type)
                            .await?;
                        info!(
                            bucket = %bucket,
                            key = %key,
                            size = data.len(),
                            "Synced object to upstream"
                        );
                        Ok(())
                    }
                    Err(_) => {
                        // Object was deleted locally before sync could read it — skip
                        Ok(())
                    }
                }
            }
            SyncEvent::DeleteObject { bucket, key } => {
                let remote = self.remote_bucket(bucket);
                self.client.delete_object(&remote, key).await?;
                info!(bucket = %bucket, key = %key, "Synced object deletion to upstream");
                Ok(())
            }
        }
    }
}
