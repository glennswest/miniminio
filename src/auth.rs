use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, Uri};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::s3::error::S3Error;

type HmacSha256 = Hmac<Sha256>;

/// Parsed AWS Signature V4 authorization info.
#[derive(Debug)]
struct AuthInfo {
    access_key: String,
    date: String,
    #[allow(dead_code)]
    region: String,
    signed_headers: Vec<String>,
    signature: String,
}

/// Verify an incoming S3 request's AWS Signature V4 auth.
pub fn verify_request(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    access_key: &str,
    secret_key: &str,
    region: &str,
) -> Result<(), S3Error> {
    // Check for anonymous requests (no auth header)
    let auth_header = match headers.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(h) => h,
        None => {
            // Check for query string auth (presigned URLs)
            if uri.query().map_or(false, |q| q.contains("X-Amz-Signature")) {
                return verify_presigned(method, uri, headers, access_key, secret_key, region);
            }
            return Err(S3Error::MissingSecurityHeader);
        }
    };

    let auth = parse_auth_header(auth_header)?;

    if auth.access_key != access_key {
        return Err(S3Error::AccessDenied);
    }

    // Build canonical request
    let canonical_uri = percent_encode_path(uri.path());
    let canonical_query = canonical_query_string(uri);
    let canonical_headers = canonical_headers_string(headers, &auth.signed_headers);
    let signed_headers_str = auth.signed_headers.join(";");

    let payload_hash = headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("UNSIGNED-PAYLOAD");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, canonical_uri, canonical_query, canonical_headers, signed_headers_str, payload_hash
    );

    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    // Build string to sign
    let amz_date = headers
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .ok_or(S3Error::MissingSecurityHeader)?;

    let scope = format!("{}/{}/s3/aws4_request", &auth.date, region);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{}\n{}\n{}", amz_date, scope, canonical_hash);

    // Compute signing key
    let signing_key = derive_signing_key(secret_key, &auth.date, region);
    let expected_sig = hmac_hex(&signing_key, string_to_sign.as_bytes());

    if !constant_time_eq(expected_sig.as_bytes(), auth.signature.as_bytes()) {
        return Err(S3Error::SignatureDoesNotMatch);
    }

    Ok(())
}

fn verify_presigned(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    access_key: &str,
    secret_key: &str,
    region: &str,
) -> Result<(), S3Error> {
    let query_str = uri.query().unwrap_or("");
    let params: BTreeMap<String, String> = url_decode_params(query_str);

    let credential = params
        .get("X-Amz-Credential")
        .ok_or(S3Error::MissingSecurityHeader)?;
    let signed_headers_str = params
        .get("X-Amz-SignedHeaders")
        .ok_or(S3Error::MissingSecurityHeader)?;
    let amz_date = params
        .get("X-Amz-Date")
        .ok_or(S3Error::MissingSecurityHeader)?;
    let provided_sig = params
        .get("X-Amz-Signature")
        .ok_or(S3Error::MissingSecurityHeader)?;

    // Parse credential: access_key/date/region/s3/aws4_request
    let cred_parts: Vec<&str> = credential.split('/').collect();
    if cred_parts.len() != 5 || cred_parts[0] != access_key {
        return Err(S3Error::AccessDenied);
    }
    let date = cred_parts[1];

    let signed_headers: Vec<String> =
        signed_headers_str.split(';').map(String::from).collect();

    // Build canonical query without X-Amz-Signature
    let canonical_query: String = params
        .iter()
        .filter(|(k, _)| k.as_str() != "X-Amz-Signature")
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_uri = percent_encode_path(uri.path());
    let canonical_headers = canonical_headers_string(headers, &signed_headers);
    let signed_headers_joined = signed_headers.join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\nUNSIGNED-PAYLOAD",
        method,
        canonical_uri,
        canonical_query,
        canonical_headers,
        signed_headers_joined
    );

    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let scope = format!("{}/{}/s3/aws4_request", date, region);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{}\n{}\n{}", amz_date, scope, canonical_hash);

    let signing_key = derive_signing_key(secret_key, date, region);
    let expected_sig = hmac_hex(&signing_key, string_to_sign.as_bytes());

    if !constant_time_eq(expected_sig.as_bytes(), provided_sig.as_bytes()) {
        return Err(S3Error::SignatureDoesNotMatch);
    }

    Ok(())
}

fn parse_auth_header(header: &str) -> Result<AuthInfo, S3Error> {
    // AWS4-HMAC-SHA256 Credential=AKID/date/region/s3/aws4_request, SignedHeaders=..., Signature=...
    let header = header
        .strip_prefix("AWS4-HMAC-SHA256 ")
        .ok_or(S3Error::MissingSecurityHeader)?;

    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;

    for part in header.split(", ") {
        if let Some(val) = part.strip_prefix("Credential=") {
            credential = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("SignedHeaders=") {
            signed_headers = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("Signature=") {
            signature = Some(val.to_string());
        }
    }

    let credential = credential.ok_or(S3Error::MissingSecurityHeader)?;
    let signed_headers = signed_headers.ok_or(S3Error::MissingSecurityHeader)?;
    let signature = signature.ok_or(S3Error::MissingSecurityHeader)?;

    // Parse credential: access_key/date/region/service/aws4_request
    let cred_parts: Vec<&str> = credential.split('/').collect();
    if cred_parts.len() != 5 {
        return Err(S3Error::MissingSecurityHeader);
    }

    Ok(AuthInfo {
        access_key: cred_parts[0].into(),
        date: cred_parts[1].into(),
        region: cred_parts[2].into(),
        signed_headers: signed_headers.split(';').map(String::from).collect(),
        signature,
    })
}

fn canonical_query_string(uri: &Uri) -> String {
    let query = match uri.query() {
        Some(q) => q,
        None => return String::new(),
    };
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (pair.to_string(), String::new()),
        };
        params.insert(k, v);
    }
    params
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn canonical_headers_string(headers: &HeaderMap, signed_headers: &[String]) -> String {
    let mut result = String::new();
    for name in signed_headers {
        let value = headers
            .get(name.as_str())
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        result.push_str(name);
        result.push(':');
        result.push_str(value.trim());
        result.push('\n');
    }
    result
}

fn derive_signing_key(secret_key: &str, date: &str, region: &str) -> Vec<u8> {
    let key = format!("AWS4{secret_key}");
    let date_key = hmac_bytes(key.as_bytes(), date.as_bytes());
    let region_key = hmac_bytes(&date_key, region.as_bytes());
    let service_key = hmac_bytes(&region_key, b"s3");
    hmac_bytes(&service_key, b"aws4_request")
}

fn hmac_bytes(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_hex(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_bytes(key, data))
}

fn percent_encode_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }
    let segments: Vec<String> = path
        .split('/')
        .map(|s| uri_encode(s))
        .collect();
    segments.join("/")
}

fn uri_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'~' | b'.' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    result
}

fn url_decode_params(query: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(pair), String::new()),
        };
        map.insert(k, v);
    }
    map
}

fn url_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Axum middleware layer for S3 auth.
pub async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    request: Request<Body>,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, S3Error> {
    // Skip auth for UI routes and health
    let path = request.uri().path();
    if path.starts_with("/ui") || path == "/health" || path == "/favicon.ico" {
        return Ok(next.run(request).await);
    }

    verify_request(
        request.method(),
        request.uri(),
        request.headers(),
        &state.config.access_key,
        &state.config.secret_key,
        &state.config.region,
    )?;

    Ok(next.run(request).await)
}
