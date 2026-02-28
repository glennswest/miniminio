use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

use crate::s3::error::S3Error;
use crate::xml::XmlWriter;
use crate::AppState;

/// GET / — ListBuckets
pub async fn list_buckets(State(state): State<AppState>) -> Result<Response, S3Error> {
    state.metrics.inc_request("GET");
    let buckets = state.storage.list_buckets().await?;

    let mut w = XmlWriter::new();
    w.declaration().open_s3("ListAllMyBucketsResult");
    w.open("Owner")
        .elem("ID", "miniminio")
        .elem("DisplayName", "miniminio")
        .close("Owner");
    w.open("Buckets");
    for b in &buckets {
        w.open("Bucket")
            .elem("Name", &b.name)
            .elem("CreationDate", &b.created)
            .close("Bucket");
    }
    w.close("Buckets").close("ListAllMyBucketsResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

/// PUT /:bucket — CreateBucket
pub async fn create_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("PUT");
    state.storage.create_bucket(&bucket).await?;
    tracing::info!(bucket = %bucket, "Bucket created");

    Ok((
        StatusCode::OK,
        [("location", format!("/{bucket}").as_str())],
        "",
    )
        .into_response())
}

/// DELETE /:bucket — DeleteBucket
pub async fn delete_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("DELETE");
    state.storage.delete_bucket(&bucket).await?;
    tracing::info!(bucket = %bucket, "Bucket deleted");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// HEAD /:bucket — HeadBucket
pub async fn head_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("HEAD");
    if !state.storage.bucket_exists(&bucket).await {
        return Err(S3Error::NoSuchBucket(bucket));
    }
    Ok((StatusCode::OK, [("x-amz-bucket-region", "us-east-1")]).into_response())
}

/// GET /:bucket — ListObjectsV1/V2 or ListMultipartUploads or GetBucketLocation
pub async fn get_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("GET");

    // GetBucketLocation
    if params.contains_key("location") {
        return get_bucket_location(&state, &bucket).await;
    }

    // ListMultipartUploads
    if params.contains_key("uploads") {
        return list_multipart_uploads(&state, &bucket).await;
    }

    // ListObjectsV2
    let list_type = params.get("list-type").map(|s| s.as_str()).unwrap_or("1");
    let prefix = params.get("prefix").map(|s| s.as_str()).unwrap_or("");
    let delimiter = params.get("delimiter").map(|s| s.as_str()).unwrap_or("");
    let max_keys: usize = params
        .get("max-keys")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let start_after = params
        .get("start-after")
        .map(|s| s.as_str())
        .unwrap_or("");
    let continuation_token = params
        .get("continuation-token")
        .map(|s| s.as_str())
        .unwrap_or("");

    let result = state
        .storage
        .list_objects(&bucket, prefix, delimiter, max_keys, start_after, continuation_token)
        .await?;

    let mut w = XmlWriter::new();
    w.declaration().open_s3("ListBucketResult");
    w.elem("Name", &bucket);
    w.elem("Prefix", prefix);

    if list_type == "2" {
        w.elem_u64("KeyCount", result.objects.len() as u64);
    }

    w.elem_u64("MaxKeys", max_keys as u64);
    if !delimiter.is_empty() {
        w.elem("Delimiter", delimiter);
    }
    w.elem_bool("IsTruncated", result.is_truncated);

    if let Some(ref token) = result.next_continuation_token {
        w.elem("NextContinuationToken", token);
    }

    for obj in &result.objects {
        w.open("Contents");
        w.elem("Key", &obj.key);
        w.elem("LastModified", &obj.last_modified);
        w.elem("ETag", &format!("\"{}\"", obj.etag));
        w.elem_u64("Size", obj.size);
        w.elem("StorageClass", "STANDARD");
        w.close("Contents");
    }

    for cp in &result.common_prefixes {
        w.open("CommonPrefixes").elem("Prefix", cp).close("CommonPrefixes");
    }

    w.close("ListBucketResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

/// POST /:bucket — DeleteObjects (batch) or other POST operations
pub async fn post_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, S3Error> {
    state.metrics.inc_request("POST");

    if params.contains_key("delete") {
        return delete_objects(&state, &bucket, &body).await;
    }

    Err(S3Error::MethodNotAllowed)
}

async fn get_bucket_location(state: &AppState, bucket: &str) -> Result<Response, S3Error> {
    if !state.storage.bucket_exists(bucket).await {
        return Err(S3Error::NoSuchBucket(bucket.into()));
    }
    let mut w = XmlWriter::new();
    w.declaration()
        .open_s3("LocationConstraint")
        .raw(&state.config.region)
        .close("LocationConstraint");
    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

async fn list_multipart_uploads(state: &AppState, bucket: &str) -> Result<Response, S3Error> {
    let uploads = state.storage.list_multipart_uploads(bucket).await?;

    let mut w = XmlWriter::new();
    w.declaration().open_s3("ListMultipartUploadsResult");
    w.elem("Bucket", bucket);

    for upload in &uploads {
        w.open("Upload");
        w.elem("Key", &upload.key);
        w.elem("UploadId", &upload.upload_id);
        w.elem("Initiated", &upload.initiated);
        w.open("Initiator")
            .elem("ID", "miniminio")
            .elem("DisplayName", "miniminio")
            .close("Initiator");
        w.open("Owner")
            .elem("ID", "miniminio")
            .elem("DisplayName", "miniminio")
            .close("Owner");
        w.elem("StorageClass", "STANDARD");
        w.close("Upload");
    }

    w.close("ListMultipartUploadsResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}

async fn delete_objects(
    state: &AppState,
    bucket: &str,
    body: &[u8],
) -> Result<Response, S3Error> {
    // Parse the Delete XML request
    #[derive(serde::Deserialize, Debug)]
    struct DeleteRequest {
        #[serde(rename = "Object")]
        objects: Vec<DeleteObject>,
        #[serde(rename = "Quiet", default)]
        quiet: Option<bool>,
    }
    #[derive(serde::Deserialize, Debug)]
    struct DeleteObject {
        #[serde(rename = "Key")]
        key: String,
    }

    let body_str =
        std::str::from_utf8(body).map_err(|e| S3Error::InvalidArgument(e.to_string()))?;
    let request: DeleteRequest = quick_xml::de::from_str(body_str)
        .map_err(|e| S3Error::InvalidArgument(format!("Invalid XML: {e}")))?;

    let quiet = request.quiet.unwrap_or(false);

    let mut w = XmlWriter::new();
    w.declaration().open_s3("DeleteResult");

    for obj in &request.objects {
        match state.storage.delete_object(bucket, &obj.key).await {
            Ok(()) => {
                if !quiet {
                    w.open("Deleted").elem("Key", &obj.key).close("Deleted");
                }
            }
            Err(e) => {
                w.open("Error")
                    .elem("Key", &obj.key)
                    .elem("Code", "InternalError")
                    .elem("Message", &e.to_string())
                    .close("Error");
            }
        }
    }

    w.close("DeleteResult");

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        w.finish(),
    )
        .into_response())
}
