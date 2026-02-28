use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::auth::{derive_signing_key, hmac_hex, percent_encode_path};

#[derive(Debug)]
pub enum SyncError {
    Http(reqwest::Error),
    Remote(u16, String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::Remote(status, body) => write!(f, "Remote error {status}: {body}"),
        }
    }
}

impl std::error::Error for SyncError {}

/// Minimal S3 client for pushing objects to a remote S3/MinIO endpoint.
pub struct S3Client {
    http: reqwest::Client,
    endpoint: String,
    host: String,
    access_key: String,
    secret_key: String,
    region: String,
}

impl S3Client {
    pub fn new(endpoint: &str, access_key: &str, secret_key: &str, region: &str) -> Self {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let host = parse_host(&endpoint);
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(false)
            .build()
            .expect("Failed to build HTTP client");
        Self {
            http,
            endpoint,
            host,
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            region: region.into(),
        }
    }

    pub async fn create_bucket(&self, bucket: &str) -> Result<(), SyncError> {
        let path = format!("/{bucket}");
        let resp = self.signed_request(reqwest::Method::PUT, &path, b"", "").await?;
        let status = resp.status().as_u16();
        // 200 = created, 409 = already exists (fine)
        if status == 200 || status == 409 {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(SyncError::Remote(status, body))
        }
    }

    pub async fn delete_bucket(&self, bucket: &str) -> Result<(), SyncError> {
        let path = format!("/{bucket}");
        let resp = self
            .signed_request(reqwest::Method::DELETE, &path, b"", "")
            .await?;
        let status = resp.status().as_u16();
        // 204 = deleted, 404 = already gone (fine), 409 = not empty (skip)
        if status == 204 || status == 404 || status == 409 {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(SyncError::Remote(status, body))
        }
    }

    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> Result<(), SyncError> {
        let path = format!("/{bucket}/{key}");
        let resp = self
            .signed_request(reqwest::Method::PUT, &path, data, content_type)
            .await?;
        let status = resp.status().as_u16();
        if status == 200 {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(SyncError::Remote(status, body))
        }
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), SyncError> {
        let path = format!("/{bucket}/{key}");
        let resp = self
            .signed_request(reqwest::Method::DELETE, &path, b"", "")
            .await?;
        let status = resp.status().as_u16();
        // 204 = deleted, 404 = already gone
        if status == 204 || status == 404 {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(SyncError::Remote(status, body))
        }
    }

    pub async fn head_bucket(&self, bucket: &str) -> Result<bool, SyncError> {
        let path = format!("/{bucket}");
        let resp = self
            .signed_request(reqwest::Method::HEAD, &path, b"", "")
            .await?;
        Ok(resp.status().as_u16() == 200)
    }

    async fn signed_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &[u8],
        content_type: &str,
    ) -> Result<reqwest::Response, SyncError> {
        let url = format!("{}{}", self.endpoint, path);

        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();

        let payload_hash = hex::encode(Sha256::digest(body));

        // Split path and query
        let (uri_path, query_string) = match path.split_once('?') {
            Some((p, q)) => (p, q),
            None => (path, ""),
        };

        let canonical_uri = percent_encode_path(uri_path);

        // Build canonical headers — must be sorted by header name
        let mut canonical_headers = format!(
            "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            self.host, payload_hash, amz_date
        );
        let mut signed_headers = "host;x-amz-content-sha256;x-amz-date".to_string();

        if !content_type.is_empty() {
            canonical_headers = format!(
                "content-type:{}\nhost:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
                content_type, self.host, payload_hash, amz_date
            );
            signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date".to_string();
        }

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method, canonical_uri, query_string, canonical_headers, signed_headers, payload_hash
        );

        let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let scope = format!("{}/{}/s3/aws4_request", date_stamp, self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date, scope, canonical_hash
        );

        let signing_key = derive_signing_key(&self.secret_key, &date_stamp, &self.region);
        let signature = hmac_hex(&signing_key, string_to_sign.as_bytes());

        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, scope, signed_headers, signature
        );

        let mut builder = self
            .http
            .request(method, &url)
            .header("Authorization", &auth)
            .header("x-amz-date", &amz_date)
            .header("x-amz-content-sha256", &payload_hash)
            .header("Host", &self.host);

        if !content_type.is_empty() {
            builder = builder.header("Content-Type", content_type);
        }

        if !body.is_empty() {
            builder = builder.body(body.to_vec());
        }

        builder.send().await.map_err(SyncError::Http)
    }
}

fn parse_host(endpoint: &str) -> String {
    let without_scheme = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}
