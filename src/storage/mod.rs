pub mod cleanup;
pub mod metadata;

use crate::s3::error::S3Error;
use crate::sync::SyncEvent;
use metadata::*;

use chrono::Utc;
use md5::{Digest, Md5};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Storage {
    data_dir: PathBuf,
    sync_tx: Option<mpsc::UnboundedSender<SyncEvent>>,
    sync_pending: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
}

impl Storage {
    pub async fn new(data_dir: PathBuf) -> Result<Self, S3Error> {
        fs::create_dir_all(&data_dir)
            .await
            .map_err(|e| S3Error::InternalError(format!("Failed to create data dir: {e}")))?;
        fs::create_dir_all(data_dir.join(".multipart"))
            .await
            .map_err(|e| S3Error::InternalError(format!("Failed to create multipart dir: {e}")))?;
        Ok(Self {
            data_dir,
            sync_tx: None,
            sync_pending: None,
        })
    }

    /// Attach a sync channel. Events will be emitted for all write operations.
    pub fn with_sync(
        mut self,
        tx: mpsc::UnboundedSender<SyncEvent>,
        pending_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        self.sync_tx = Some(tx);
        self.sync_pending = Some(pending_counter);
        self
    }

    fn emit_sync(&self, event: SyncEvent) {
        if let Some(tx) = &self.sync_tx {
            if let Some(counter) = &self.sync_pending {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            let _ = tx.send(event);
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn bucket_path(&self, bucket: &str) -> PathBuf {
        self.data_dir.join(bucket)
    }

    fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.data_dir.join(bucket).join(key)
    }

    fn meta_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.data_dir
            .join(bucket)
            .join(".miniminio")
            .join(format!("{key}.meta"))
    }

    fn multipart_dir(&self, upload_id: &str) -> PathBuf {
        self.data_dir.join(".multipart").join(upload_id)
    }

    fn validate_bucket_name(name: &str) -> Result<(), S3Error> {
        if name.len() < 3 || name.len() > 63 {
            return Err(S3Error::InvalidBucketName(
                "Bucket name must be 3-63 characters".into(),
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        {
            return Err(S3Error::InvalidBucketName(
                "Bucket name must contain only lowercase letters, numbers, hyphens, periods"
                    .into(),
            ));
        }
        if !name.as_bytes()[0].is_ascii_alphanumeric() {
            return Err(S3Error::InvalidBucketName(
                "Bucket name must start with letter or number".into(),
            ));
        }
        if name.starts_with('.') || name.contains("..") {
            return Err(S3Error::InvalidBucketName(
                "Invalid bucket name format".into(),
            ));
        }
        Ok(())
    }

    fn validate_key(key: &str) -> Result<(), S3Error> {
        if key.is_empty() {
            return Err(S3Error::InvalidArgument("Key cannot be empty".into()));
        }
        if key.len() > 1024 {
            return Err(S3Error::InvalidArgument("Key too long".into()));
        }
        for component in Path::new(key).components() {
            match component {
                Component::Normal(_) => {}
                _ => {
                    return Err(S3Error::InvalidArgument(
                        "Invalid key: path traversal not allowed".into(),
                    ))
                }
            }
        }
        Ok(())
    }

    // --- Bucket operations ---

    pub async fn create_bucket(&self, name: &str) -> Result<(), S3Error> {
        Self::validate_bucket_name(name)?;
        let path = self.bucket_path(name);
        if path.exists() {
            return Err(S3Error::BucketAlreadyExists(name.into()));
        }
        fs::create_dir_all(&path)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        fs::create_dir_all(path.join(".miniminio"))
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        // Store creation timestamp
        let info = BucketInfo {
            name: name.into(),
            created: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        };
        let info_path = path.join(".miniminio").join("bucket.json");
        let data = serde_json::to_vec_pretty(&info)
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        fs::write(info_path, data)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        self.emit_sync(SyncEvent::CreateBucket {
            bucket: name.into(),
        });
        Ok(())
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<(), S3Error> {
        let path = self.bucket_path(name);
        if !path.exists() {
            return Err(S3Error::NoSuchBucket(name.into()));
        }
        // Check if bucket is empty (only .miniminio dir allowed)
        let mut entries = fs::read_dir(&path)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?
        {
            let fname = entry.file_name();
            if fname != ".miniminio" {
                return Err(S3Error::BucketNotEmpty(name.into()));
            }
        }
        fs::remove_dir_all(&path)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        self.emit_sync(SyncEvent::DeleteBucket {
            bucket: name.into(),
        });
        Ok(())
    }

    pub async fn bucket_exists(&self, name: &str) -> bool {
        self.bucket_path(name).is_dir()
    }

    pub async fn list_buckets(&self) -> Result<Vec<BucketInfo>, S3Error> {
        let mut buckets = Vec::new();
        let mut entries = fs::read_dir(&self.data_dir)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?
        {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') {
                continue;
            }
            let ft = entry
                .file_type()
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
            if !ft.is_dir() {
                continue;
            }
            let info_path = entry.path().join(".miniminio").join("bucket.json");
            let info = if info_path.exists() {
                let data = fs::read(&info_path)
                    .await
                    .map_err(|e| S3Error::InternalError(e.to_string()))?;
                serde_json::from_slice(&data).unwrap_or(BucketInfo {
                    name: fname.clone(),
                    created: "1970-01-01T00:00:00.000Z".into(),
                })
            } else {
                BucketInfo {
                    name: fname.clone(),
                    created: "1970-01-01T00:00:00.000Z".into(),
                }
            };
            buckets.push(info);
        }
        buckets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(buckets)
    }

    // --- Object operations ---

    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        data: &[u8],
        content_type: &str,
        user_metadata: HashMap<String, String>,
    ) -> Result<ObjectMetadata, S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }
        Self::validate_key(key)?;

        let obj_path = self.object_path(bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
        }

        // Write object data
        fs::write(&obj_path, data)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        // Compute ETag (MD5)
        let etag = format!("{:x}", Md5::digest(data));
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

        let meta = ObjectMetadata {
            key: key.into(),
            size: data.len() as u64,
            etag: etag.clone(),
            content_type: content_type.into(),
            last_modified: now,
            user_metadata,
        };

        // Write metadata
        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
        }
        let meta_data =
            serde_json::to_vec(&meta).map_err(|e| S3Error::InternalError(e.to_string()))?;
        fs::write(&meta_path, meta_data)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        self.emit_sync(SyncEvent::PutObject {
            bucket: bucket.into(),
            key: key.into(),
        });

        Ok(meta)
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(ObjectMetadata, Vec<u8>), S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }
        Self::validate_key(key)?;

        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey(key.into()));
        }

        let data = fs::read(&obj_path)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        let meta = self.load_metadata(bucket, key).await?;

        Ok((meta, data))
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMetadata, S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }
        Self::validate_key(key)?;

        let obj_path = self.object_path(bucket, key);
        if !obj_path.is_file() {
            return Err(S3Error::NoSuchKey(key.into()));
        }

        self.load_metadata(bucket, key).await
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }
        Self::validate_key(key)?;

        let obj_path = self.object_path(bucket, key);
        if obj_path.is_file() {
            fs::remove_file(&obj_path).await.ok();
        }
        let meta_path = self.meta_path(bucket, key);
        if meta_path.is_file() {
            fs::remove_file(&meta_path).await.ok();
        }

        // Clean up empty parent directories (but not the bucket dir itself)
        let bucket_dir = self.bucket_path(bucket);
        let mut current = obj_path.parent().map(|p| p.to_path_buf());
        while let Some(dir) = current {
            if dir == bucket_dir {
                break;
            }
            // Try to remove empty dir (fails if not empty, which is fine)
            if fs::remove_dir(&dir).await.is_err() {
                break;
            }
            current = dir.parent().map(|p| p.to_path_buf());
        }

        self.emit_sync(SyncEvent::DeleteObject {
            bucket: bucket.into(),
            key: key.into(),
        });

        Ok(())
    }

    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: &str,
        max_keys: usize,
        start_after: &str,
        continuation_token: &str,
    ) -> Result<ListObjectsResult, S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }

        let bucket_dir = self.bucket_path(bucket);
        let mut all_keys = Vec::new();
        self.walk_dir(&bucket_dir, &bucket_dir, &mut all_keys)
            .await?;
        all_keys.sort();

        // Apply prefix filter
        let filtered: Vec<String> = all_keys
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .collect();

        // Determine start position
        let start = if !continuation_token.is_empty() {
            continuation_token.to_string()
        } else {
            start_after.to_string()
        };

        let mut objects = Vec::new();
        let mut common_prefixes = Vec::new();
        let mut seen_prefixes = std::collections::HashSet::new();

        for key in &filtered {
            if !start.is_empty() && key.as_str() <= start.as_str() {
                continue;
            }

            if !delimiter.is_empty() {
                let after_prefix = &key[prefix.len()..];
                if let Some(pos) = after_prefix.find(delimiter) {
                    let cp = format!("{}{}", prefix, &after_prefix[..=pos]);
                    if seen_prefixes.insert(cp.clone()) {
                        common_prefixes.push(cp);
                    }
                    continue;
                }
            }

            if objects.len() >= max_keys {
                break;
            }

            match self.load_metadata(bucket, key).await {
                Ok(meta) => objects.push(meta),
                Err(_) => {
                    // Metadata missing, reconstruct from file
                    let path = self.object_path(bucket, key);
                    if let Ok(fs_meta) = fs::metadata(&path).await {
                        objects.push(ObjectMetadata {
                            key: key.clone(),
                            size: fs_meta.len(),
                            etag: String::new(),
                            content_type: "application/octet-stream".into(),
                            last_modified: Utc::now()
                                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                                .to_string(),
                            user_metadata: HashMap::new(),
                        });
                    }
                }
            }
        }

        let is_truncated = objects.len() >= max_keys;
        let next_token = if is_truncated {
            objects.last().map(|o| o.key.clone())
        } else {
            None
        };

        Ok(ListObjectsResult {
            objects,
            common_prefixes,
            is_truncated,
            next_continuation_token: next_token,
        })
    }

    async fn walk_dir(
        &self,
        base: &Path,
        dir: &Path,
        keys: &mut Vec<String>,
    ) -> Result<(), S3Error> {
        let mut entries = match fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                Box::pin(self.walk_dir(base, &path, keys)).await?;
            } else if path.is_file() {
                if let Ok(rel) = path.strip_prefix(base) {
                    keys.push(rel.to_string_lossy().to_string());
                }
            }
        }
        Ok(())
    }

    async fn load_metadata(&self, bucket: &str, key: &str) -> Result<ObjectMetadata, S3Error> {
        let meta_path = self.meta_path(bucket, key);
        let data = fs::read(&meta_path)
            .await
            .map_err(|_| S3Error::NoSuchKey(key.into()))?;
        serde_json::from_slice(&data).map_err(|e| S3Error::InternalError(e.to_string()))
    }

    pub async fn copy_object(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
    ) -> Result<ObjectMetadata, S3Error> {
        let (meta, data) = self.get_object(src_bucket, src_key).await?;
        self.put_object(dst_bucket, dst_key, &data, &meta.content_type, meta.user_metadata)
            .await
    }

    // --- Multipart operations ---

    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
    ) -> Result<String, S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }
        Self::validate_key(key)?;

        let upload_id = uuid::Uuid::new_v4().to_string();
        let mp_dir = self.multipart_dir(&upload_id);
        fs::create_dir_all(mp_dir.join("parts"))
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        let info = MultipartUpload {
            upload_id: upload_id.clone(),
            bucket: bucket.into(),
            key: key.into(),
            initiated: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        };
        let info_data =
            serde_json::to_vec(&info).map_err(|e| S3Error::InternalError(e.to_string()))?;
        fs::write(mp_dir.join("upload.json"), info_data)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        // Store content type for later
        fs::write(mp_dir.join("content_type"), content_type.as_bytes())
            .await
            .ok();

        Ok(upload_id)
    }

    pub async fn upload_part(
        &self,
        upload_id: &str,
        part_number: i32,
        data: &[u8],
    ) -> Result<String, S3Error> {
        let mp_dir = self.multipart_dir(upload_id);
        if !mp_dir.exists() {
            return Err(S3Error::NoSuchUpload(upload_id.into()));
        }

        let etag = format!("{:x}", Md5::digest(data));
        let part_path = mp_dir.join("parts").join(part_number.to_string());
        fs::write(&part_path, data)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        // Store part metadata
        let info = PartInfo {
            part_number,
            size: data.len() as u64,
            etag: etag.clone(),
            last_modified: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        };
        let info_data =
            serde_json::to_vec(&info).map_err(|e| S3Error::InternalError(e.to_string()))?;
        fs::write(
            mp_dir
                .join("parts")
                .join(format!("{part_number}.meta")),
            info_data,
        )
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

        Ok(etag)
    }

    pub async fn complete_multipart_upload(
        &self,
        upload_id: &str,
        parts: &[(i32, String)],
    ) -> Result<ObjectMetadata, S3Error> {
        let mp_dir = self.multipart_dir(upload_id);
        if !mp_dir.exists() {
            return Err(S3Error::NoSuchUpload(upload_id.into()));
        }

        // Load upload info
        let info_data = fs::read(mp_dir.join("upload.json"))
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        let info: MultipartUpload =
            serde_json::from_slice(&info_data).map_err(|e| S3Error::InternalError(e.to_string()))?;

        // Verify parts are in order
        for window in parts.windows(2) {
            if window[0].0 >= window[1].0 {
                return Err(S3Error::InvalidPartOrder);
            }
        }

        // Assemble the object
        let obj_path = self.object_path(&info.bucket, &info.key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
        }

        let mut file = fs::File::create(&obj_path)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        let mut total_size: u64 = 0;
        let mut md5_digests = Vec::new();

        for (part_num, _etag) in parts {
            let part_path = mp_dir.join("parts").join(part_num.to_string());
            if !part_path.exists() {
                return Err(S3Error::InvalidPart);
            }
            let data = fs::read(&part_path)
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
            md5_digests.extend_from_slice(&Md5::digest(&data));
            total_size += data.len() as u64;
            file.write_all(&data)
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
        }
        file.flush()
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        // Compute multipart ETag: md5(concat(part_md5s))-N
        let combined_hash = format!("{:x}-{}", Md5::digest(&md5_digests), parts.len());

        // Load content type
        let content_type = fs::read_to_string(mp_dir.join("content_type"))
            .await
            .unwrap_or_else(|_| "application/octet-stream".into());

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let meta = ObjectMetadata {
            key: info.key.clone(),
            size: total_size,
            etag: combined_hash,
            content_type,
            last_modified: now,
            user_metadata: HashMap::new(),
        };

        // Write metadata
        let meta_path = self.meta_path(&info.bucket, &info.key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| S3Error::InternalError(e.to_string()))?;
        }
        let meta_data =
            serde_json::to_vec(&meta).map_err(|e| S3Error::InternalError(e.to_string()))?;
        fs::write(&meta_path, meta_data)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;

        // Clean up multipart staging
        fs::remove_dir_all(&mp_dir).await.ok();

        self.emit_sync(SyncEvent::PutObject {
            bucket: info.bucket.clone(),
            key: info.key.clone(),
        });

        Ok(meta)
    }

    pub async fn abort_multipart_upload(&self, upload_id: &str) -> Result<(), S3Error> {
        let mp_dir = self.multipart_dir(upload_id);
        if !mp_dir.exists() {
            return Err(S3Error::NoSuchUpload(upload_id.into()));
        }
        fs::remove_dir_all(&mp_dir)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        Ok(())
    }

    pub async fn list_parts(&self, upload_id: &str) -> Result<Vec<PartInfo>, S3Error> {
        let mp_dir = self.multipart_dir(upload_id);
        if !mp_dir.exists() {
            return Err(S3Error::NoSuchUpload(upload_id.into()));
        }

        let parts_dir = mp_dir.join("parts");
        let mut parts = Vec::new();
        let mut entries = fs::read_dir(&parts_dir)
            .await
            .map_err(|e| S3Error::InternalError(e.to_string()))?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".meta") {
                let data = fs::read(entry.path())
                    .await
                    .map_err(|e| S3Error::InternalError(e.to_string()))?;
                if let Ok(info) = serde_json::from_slice::<PartInfo>(&data) {
                    parts.push(info);
                }
            }
        }
        parts.sort_by_key(|p| p.part_number);
        Ok(parts)
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUpload>, S3Error> {
        let mp_base = self.data_dir.join(".multipart");
        let mut uploads = Vec::new();
        let mut entries = match fs::read_dir(&mp_base).await {
            Ok(e) => e,
            Err(_) => return Ok(uploads),
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let info_path = entry.path().join("upload.json");
            if let Ok(data) = fs::read(&info_path).await {
                if let Ok(info) = serde_json::from_slice::<MultipartUpload>(&data) {
                    if info.bucket == bucket {
                        uploads.push(info);
                    }
                }
            }
        }
        uploads.sort_by(|a, b| a.initiated.cmp(&b.initiated));
        Ok(uploads)
    }

    pub async fn get_multipart_upload_info(
        &self,
        upload_id: &str,
    ) -> Result<MultipartUpload, S3Error> {
        let mp_dir = self.multipart_dir(upload_id);
        let data = fs::read(mp_dir.join("upload.json"))
            .await
            .map_err(|_| S3Error::NoSuchUpload(upload_id.into()))?;
        serde_json::from_slice(&data).map_err(|e| S3Error::InternalError(e.to_string()))
    }

    // --- Stats ---

    pub async fn bucket_stats(&self, bucket: &str) -> Result<BucketStats, S3Error> {
        if !self.bucket_exists(bucket).await {
            return Err(S3Error::NoSuchBucket(bucket.into()));
        }
        let bucket_dir = self.bucket_path(bucket);
        let mut stats = BucketStats::default();
        self.count_dir(&bucket_dir, &mut stats).await;
        Ok(stats)
    }

    async fn count_dir(&self, dir: &Path, stats: &mut BucketStats) {
        let mut entries = match fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                Box::pin(self.count_dir(&path, stats)).await;
            } else if path.is_file() {
                stats.object_count += 1;
                if let Ok(m) = fs::metadata(&path).await {
                    stats.total_size += m.len();
                }
            }
        }
    }

    pub async fn total_stats(&self) -> Result<BucketStats, S3Error> {
        let buckets = self.list_buckets().await?;
        let mut total = BucketStats::default();
        for b in &buckets {
            if let Ok(s) = self.bucket_stats(&b.name).await {
                total.object_count += s.object_count;
                total.total_size += s.total_size;
            }
        }
        Ok(total)
    }
}
