use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

use crate::s3::error::S3Error;
use crate::xml::XmlWriter;
use crate::AppState;

/// GET /:bucket/*key
pub async fn get_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("GET");

    // ListParts
    if let Some(upload_id) = params.get("uploadId") {
        return list_parts(&state, &bucket, &key, upload_id).await;
    }

    let key = normalize_key(&key);
    let (meta, data) = state.storage.get_object(&bucket, &key).await?;

    state
        .metrics
        .add_bytes_out(data.len() as u64);
    state
        .metrics
        .inc_bucket_request(&bucket, 0, data.len() as u64)
        .await;

    // Handle Range requests
    if let Some(range_header) = headers.get("range").and_then(|v| v.to_str().ok()) {
        if let Some((start, end)) = parse_range(range_header, data.len() as u64) {
            let slice = &data[start as usize..=end as usize];
            return Ok((
                StatusCode::PARTIAL_CONTENT,
                [
                    ("content-type", meta.content_type.as_str()),
                    ("etag", &format!("\"{}\"", meta.etag)),
                    ("last-modified", &meta.last_modified),
                    ("content-length", &slice.len().to_string()),
                    (
                        "content-range",
                        &format!("bytes {start}-{end}/{}", data.len()),
                    ),
                    ("accept-ranges", "bytes"),
                ],
                Bytes::copy_from_slice(slice),
            )
                .into_response());
        }
    }

    Ok((
        StatusCode::OK,
        [
            ("content-type", meta.content_type.as_str()),
            ("etag", &format!("\"{}\"", meta.etag)),
            ("last-modified", &meta.last_modified),
            ("content-length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
        ],
        data,
    )
        .into_response())
}

/// PUT /:bucket/*key
pub async fn put_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("PUT");

    // Upload Part
    if let (Some(upload_id), Some(part_number)) =
        (params.get("uploadId"), params.get("partNumber"))
    {
        let part_num: i32 = part_number
            .parse()
            .map_err(|_| S3Error::InvalidArgument("Invalid partNumber".into()))?;
        return upload_part(&state, &upload_id, part_num, &body).await;
    }

    // CopyObject
    if let Some(copy_source) = headers
        .get("x-amz-copy-source")
        .and_then(|v| v.to_str().ok())
    {
        return copy_object(&state, copy_source, &bucket, &key).await;
    }

    let key = normalize_key(&key);
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| guess_content_type(&key))
        .to_string();

    // Collect user metadata (x-amz-meta-*)
    let mut user_metadata = HashMap::new();
    for (name, value) in headers.iter() {
        if let Some(meta_key) = name.as_str().strip_prefix("x-amz-meta-") {
            if let Ok(v) = value.to_str() {
                user_metadata.insert(meta_key.to_string(), v.to_string());
            }
        }
    }

    state.metrics.add_bytes_in(body.len() as u64);
    state
        .metrics
        .inc_bucket_request(&bucket, body.len() as u64, 0)
        .await;

    let meta = state
        .storage
        .put_object(&bucket, &key, &body, &content_type, user_metadata)
        .await?;

    Ok((
        StatusCode::OK,
        [
            ("etag", format!("\"{}\"", meta.etag).as_str()),
            ("x-amz-request-id", "miniminio"),
        ],
        "",
    )
        .into_response())
}

/// DELETE /:bucket/*key
pub async fn delete_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("DELETE");

    // Abort multipart upload
    if let Some(upload_id) = params.get("uploadId") {
        state.storage.abort_multipart_upload(upload_id).await?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let key = normalize_key(&key);
    state.storage.delete_object(&bucket, &key).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// HEAD /:bucket/*key
pub async fn head_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("HEAD");
    let key = normalize_key(&key);
    let meta = state.storage.head_object(&bucket, &key).await?;

    Ok((
        StatusCode::OK,
        [
            ("content-type", meta.content_type.as_str()),
            ("etag", &format!("\"{}\"", meta.etag)),
            ("last-modified", &meta.last_modified),
            ("content-length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
        ],
    )
        .into_response())
}

/// POST /:bucket/*key — CreateMultipartUpload or CompleteMultipartUpload
pub async fn post_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("POST");

    let key = normalize_key(&key);

    // CreateMultipartUpload
    if params.contains_key("uploads") {
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| guess_content_type(&key))
            .to_string();
        let upload_id = state
            .storage
            .create_multipart_upload(&bucket, &key, &content_type)
            .await?;

        let mut w = XmlWriter::new();
        w.declaration()
            .open_s3("InitiateMultipartUploadResult")
            .elem("Bucket", &bucket)
            .elem("Key", &key)
            .elem("UploadId", &upload_id)
            .close("InitiateMultipartUploadResult");

        return Ok((
            StatusCode::OK,
            [("content-type", "application/xml")],
            w.finish(),
        )
            .into_response());
    }

    // CompleteMultipartUpload
    if let Some(upload_id) = params.get("uploadId") {
        return complete_multipart(&state, &bucket, &key, upload_id, &body).await;
    }

    Err(S3Error::MethodNotAllowed)
}

async fn upload_part(
    state: &AppState,
    upload_id: &str,
    part_number: i32,
    data: &[u8],
) -> Result<Response, S3Error> {
    state.metrics.add_bytes_in(data.len() as u64);
    let etag = state.storage.upload_part(upload_id, part_number, data).await?;

    Ok((
        StatusCode::OK,
        [("etag", format!("\"{}\"", etag).as_str())],
        "",
    )
        .into_response())
}

async fn complete_multipart(
    state: &AppState,
    bucket: &str,
    key: &str,
    upload_id: &str,
    body: &[u8],
) -> Result<Response, S3Error> {
    #[derive(serde::Deserialize, Debug)]
    struct CompleteRequest {
        #[serde(rename = "Part")]
        parts: Vec<PartEntry>,
    }
    #[derive(serde::Deserialize, Debug)]
    struct PartEntry {
        #[serde(rename = "PartNumber")]
        part_number: i32,
        #[serde(rename = "ETag")]
        etag: String,
    }

    let body_str = std::str::from_utf8(body).map_err(|e| S3Error::InvalidArgument(e.to_string()))?;
    let request: CompleteRequest = quick_xml::de::from_str(body_str)
        .map_err(|e| S3Error::InvalidArgument(format!("Invalid XML: {e}")))?;

    let parts: Vec<(i32, String)> = request
        .parts
        .into_iter()
        .map(|p| (p.part_number, p.etag.trim_matches('"').to_string()))
        .collect();

    let meta = state
        .storage
        .complete_multipart_upload(upload_id, &parts)
        .await?;

    let mut w = XmlWriter::new();
    w.declaration()
        .open_s3("CompleteMultipartUploadResult")
        .elem("Location", &format!("/{bucket}/{key}"))
        .elem("Bucket", bucket)
        .elem("Key", key)
        .elem("ETag", &format!("\"{}\"", meta.etag))
        .close("CompleteMultipartUploadResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

async fn list_parts(
    state: &AppState,
    bucket: &str,
    key: &str,
    upload_id: &str,
) -> Result<Response, S3Error> {
    let parts = state.storage.list_parts(upload_id).await?;

    let mut w = XmlWriter::new();
    w.declaration().open_s3("ListPartsResult");
    w.elem("Bucket", bucket);
    w.elem("Key", key);
    w.elem("UploadId", upload_id);

    for part in &parts {
        w.open("Part");
        w.elem_i32("PartNumber", part.part_number);
        w.elem("ETag", &format!("\"{}\"", part.etag));
        w.elem_u64("Size", part.size);
        w.elem("LastModified", &part.last_modified);
        w.close("Part");
    }

    w.close("ListPartsResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

async fn copy_object(
    state: &AppState,
    source: &str,
    dst_bucket: &str,
    dst_key: &str,
) -> Result<Response, S3Error> {
    // Source format: /bucket/key or bucket/key
    let source = source.strip_prefix('/').unwrap_or(source);
    let (src_bucket, src_key) = source
        .split_once('/')
        .ok_or_else(|| S3Error::InvalidArgument("Invalid copy source".into()))?;

    let meta = state
        .storage
        .copy_object(src_bucket, src_key, dst_bucket, dst_key)
        .await?;

    let mut w = XmlWriter::new();
    w.declaration()
        .open_s3("CopyObjectResult")
        .elem("LastModified", &meta.last_modified)
        .elem("ETag", &format!("\"{}\"", meta.etag))
        .close("CopyObjectResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

fn normalize_key(key: &str) -> String {
    // axum wildcard may include leading slash
    let key = key.strip_prefix('/').unwrap_or(key);
    percent_encoding::percent_decode_str(key)
        .decode_utf8_lossy()
        .to_string()
}

fn guess_content_type(key: &str) -> &str {
    mime_guess::from_path(key)
        .first_raw()
        .unwrap_or("application/octet-stream")
}

fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let range = header.strip_prefix("bytes=")?;
    let (start_str, end_str) = range.split_once('-')?;

    if start_str.is_empty() {
        // suffix range: -N means last N bytes
        let suffix: u64 = end_str.parse().ok()?;
        let start = total.saturating_sub(suffix);
        Some((start, total - 1))
    } else if end_str.is_empty() {
        let start: u64 = start_str.parse().ok()?;
        if start >= total {
            return None;
        }
        Some((start, total - 1))
    } else {
        let start: u64 = start_str.parse().ok()?;
        let end: u64 = end_str.parse().ok()?;
        let end = end.min(total - 1);
        if start > end {
            return None;
        }
        Some((start, end))
    }
}
