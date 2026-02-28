use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tracing::{info, warn};

use crate::storage::Storage;

/// Clean up expired multipart uploads older than `max_age_hours`.
pub async fn cleanup_multipart(storage: &Storage, max_age_hours: u64) -> u64 {
    let mp_dir = storage.data_dir().join(".multipart");
    let mut cleaned = 0u64;

    let mut entries = match fs::read_dir(&mp_dir).await {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let cutoff = chrono::Utc::now()
        - chrono::Duration::hours(max_age_hours as i64);

    while let Ok(Some(entry)) = entries.next_entry().await {
        let upload_dir = entry.path();
        let info_path = upload_dir.join("upload.json");
        if let Ok(data) = fs::read(&info_path).await {
            if let Ok(info) =
                serde_json::from_slice::<crate::storage::metadata::MultipartUpload>(&data)
            {
                if let Ok(initiated) =
                    chrono::DateTime::parse_from_rfc3339(&info.initiated)
                {
                    if initiated < cutoff {
                        info!(
                            upload_id = %info.upload_id,
                            key = %info.key,
                            "Cleaning up expired multipart upload"
                        );
                        if fs::remove_dir_all(&upload_dir).await.is_ok() {
                            cleaned += 1;
                        }
                    }
                }
            }
        }
    }

    cleaned
}

/// Remove orphaned metadata files that have no corresponding object data.
pub async fn cleanup_orphaned_metadata(storage: &Storage) -> u64 {
    let mut cleaned = 0u64;

    let buckets = match storage.list_buckets().await {
        Ok(b) => b,
        Err(_) => return 0,
    };

    for bucket in &buckets {
        let meta_dir = storage
            .data_dir()
            .join(&bucket.name)
            .join(".miniminio");
        cleaned += cleanup_meta_dir(storage.data_dir(), &bucket.name, &meta_dir).await;
    }

    cleaned
}

async fn cleanup_meta_dir(data_dir: &Path, bucket: &str, dir: &Path) -> u64 {
    let mut cleaned = 0u64;
    let mut entries = match fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return 0,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            cleaned += Box::pin(cleanup_meta_dir(data_dir, bucket, &path)).await;
            // Remove empty dirs
            if let Ok(mut d) = fs::read_dir(&path).await {
                if d.next_entry().await.ok().flatten().is_none() {
                    fs::remove_dir(&path).await.ok();
                }
            }
        } else if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
            if fname.ends_with(".meta") {
                // Derive the object path
                let rel = path
                    .strip_prefix(data_dir.join(bucket).join(".miniminio"))
                    .unwrap_or(Path::new(""));
                let key = rel
                    .to_string_lossy()
                    .trim_end_matches(".meta")
                    .to_string();
                let obj_path = data_dir.join(bucket).join(&key);
                if !obj_path.exists() {
                    warn!(bucket = %bucket, key = %key, "Removing orphaned metadata");
                    fs::remove_file(&path).await.ok();
                    cleaned += 1;
                }
            }
        }
    }

    cleaned
}

/// Spawn a background cleanup task that runs periodically.
pub fn spawn_cleanup_task(storage: Arc<Storage>, interval_hours: u64, max_age_hours: u64) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(interval_hours * 3600));
        loop {
            interval.tick().await;
            let mp = cleanup_multipart(&storage, max_age_hours).await;
            let orphan = cleanup_orphaned_metadata(&storage).await;
            if mp > 0 || orphan > 0 {
                info!(
                    multipart_cleaned = mp,
                    orphans_cleaned = orphan,
                    "Cleanup cycle complete"
                );
            }
        }
    });
}
