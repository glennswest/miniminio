# MiniMinio

Minimal S3-compatible object storage server. Single binary, single disk, scratch container.

## Features

- **S3 REST API** -- Path-style, compatible with aws-cli, mc, boto3, and any S3 client
- **Web UI** -- Dashboard, bucket browser, object management, file upload, admin panel
- **Auth** -- AWS Signature V4 (header-based and presigned URLs)
- **Storage** -- Filesystem-backed, single disk, no dependencies
- **Metrics** -- Request counting, bandwidth tracking, per-bucket statistics
- **Cleanup** -- Automatic multipart upload expiry, orphan metadata removal
- **Container** -- Static musl binary, runs from `scratch` image

## Architecture

```
                    +-------------------+
                    |   S3 Clients      |
                    | (aws-cli, mc,     |
                    |  boto3, etc.)     |
                    +--------+----------+
                             |
                    +--------v----------+
                    |  Auth Middleware   |
                    |  (AWS SigV4)      |
                    +--------+----------+
                             |
              +--------------+--------------+
              |              |              |
     +--------v---+  +------v------+  +----v------+
     |  S3 API    |  |  Web UI     |  |  Admin    |
     |  Router    |  |  (HTML)     |  |  API      |
     +--------+---+  +------+------+  +----+------+
              |              |              |
              +--------------+--------------+
                             |
                    +--------v----------+
                    |  Storage Layer    |
                    |  (Filesystem)     |
                    +--------+----------+
                             |
                    +--------v----------+
                    |  Data Directory   |
                    |  /data            |
                    +-------------------+
```

### Module Layout

```
src/
  main.rs            CLI entry point, server startup
  lib.rs             Library root, AppState definition
  config.rs          CLI/env configuration (clap)
  auth.rs            AWS Signature V4 verification middleware
  xml.rs             S3 XML response builder
  s3/
    mod.rs           S3 API router
    bucket.rs        Bucket operations (create, delete, list, head)
    object.rs        Object operations (get, put, delete, copy, list, multipart)
    error.rs         S3 error types with XML responses
  storage/
    mod.rs           Filesystem storage backend
    metadata.rs      Object/bucket metadata types
    cleanup.rs       Background cleanup tasks
  admin/
    mod.rs           Admin API routes
    metrics.rs       Request/bandwidth/per-bucket metrics
  ui/
    mod.rs           Web UI routes (dashboard, buckets, objects, admin)
```

### Data Directory Layout

```
data/
  .multipart/                    Staging area for multipart uploads
    {upload-id}/
      upload.json                Upload metadata (bucket, key, initiated)
      content_type               Content type for final object
      parts/
        1                        Part data
        1.meta                   Part metadata (number, size, etag)
        2
        2.meta
  {bucket}/
    .miniminio/
      bucket.json                Bucket creation metadata
      {key}.meta                 Object metadata (size, etag, content-type, user-meta)
    {key}                        Object data (raw bytes)
```

## Quick Start

```bash
# Run with defaults (port 9000, data in ./data, credentials minioadmin/minioadmin)
cargo run

# Custom configuration
cargo run -- --data-dir /mnt/storage --port 9000 --access-key mykey --secret-key mysecret

# All options
miniminio --help
```

Open `http://localhost:9000/ui` for the web interface.

## Configuration

All options can be set via CLI flags or environment variables:

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--data-dir` | `MINIMINIO_DATA_DIR` | `./data` | Root data directory |
| `--port` | `MINIMINIO_PORT` | `9000` | Listen port |
| `--host` | `MINIMINIO_HOST` | `0.0.0.0` | Listen address |
| `--access-key` | `MINIMINIO_ACCESS_KEY` | `minioadmin` | S3 access key |
| `--secret-key` | `MINIMINIO_SECRET_KEY` | `minioadmin` | S3 secret key |
| `--region` | `MINIMINIO_REGION` | `us-east-1` | S3 region name |
| `--multipart-expiry-hours` | `MINIMINIO_MULTIPART_EXPIRY` | `24` | Auto-cleanup age for incomplete multipart uploads |

## S3 API Reference

All S3 endpoints use path-style addressing: `http://host:port/{bucket}/{key}`

Authentication: AWS Signature Version 4 (Authorization header or presigned URL query parameters).

### Bucket Operations

| Operation | Method | Path | Description |
|-----------|--------|------|-------------|
| ListBuckets | `GET /` | `/` | List all buckets |
| CreateBucket | `PUT /{bucket}` | `/mybucket` | Create a new bucket |
| DeleteBucket | `DELETE /{bucket}` | `/mybucket` | Delete an empty bucket |
| HeadBucket | `HEAD /{bucket}` | `/mybucket` | Check if bucket exists |
| GetBucketLocation | `GET /{bucket}?location` | `/mybucket?location` | Get bucket region |

### Object Operations

| Operation | Method | Path | Description |
|-----------|--------|------|-------------|
| PutObject | `PUT /{bucket}/{key}` | `/mybucket/file.txt` | Upload an object |
| GetObject | `GET /{bucket}/{key}` | `/mybucket/file.txt` | Download an object |
| HeadObject | `HEAD /{bucket}/{key}` | `/mybucket/file.txt` | Get object metadata |
| DeleteObject | `DELETE /{bucket}/{key}` | `/mybucket/file.txt` | Delete an object |
| CopyObject | `PUT /{bucket}/{key}` | With `x-amz-copy-source` header | Copy an object |
| ListObjectsV2 | `GET /{bucket}?list-type=2` | With optional `prefix`, `delimiter`, `max-keys` | List objects |
| ListObjectsV1 | `GET /{bucket}` | Legacy listing | List objects (legacy) |
| DeleteObjects | `POST /{bucket}?delete` | XML body with keys | Batch delete |

### Multipart Upload Operations

| Operation | Method | Path | Description |
|-----------|--------|------|-------------|
| CreateMultipartUpload | `POST /{bucket}/{key}?uploads` | | Initiate multipart upload |
| UploadPart | `PUT /{bucket}/{key}?uploadId=X&partNumber=N` | | Upload a part |
| CompleteMultipartUpload | `POST /{bucket}/{key}?uploadId=X` | XML body with parts | Assemble final object |
| AbortMultipartUpload | `DELETE /{bucket}/{key}?uploadId=X` | | Cancel and clean up |
| ListParts | `GET /{bucket}/{key}?uploadId=X` | | List uploaded parts |
| ListMultipartUploads | `GET /{bucket}?uploads` | | List in-progress uploads |

### Supported Headers

**Request:**
- `Content-Type` -- Object MIME type (auto-detected from extension if not provided)
- `Range: bytes=start-end` -- Partial object download
- `x-amz-copy-source: /bucket/key` -- Source for CopyObject
- `x-amz-meta-*` -- Custom user metadata

**Response:**
- `ETag` -- MD5 hash of object (quoted), or `md5-N` for multipart
- `Content-Type` -- Object MIME type
- `Content-Length` -- Object size in bytes
- `Last-Modified` -- ISO 8601 timestamp
- `Accept-Ranges: bytes` -- Range request support

### Example: aws-cli

```bash
# Configure credentials
export AWS_ACCESS_KEY_ID=minioadmin
export AWS_SECRET_ACCESS_KEY=minioadmin
export AWS_DEFAULT_REGION=us-east-1

# Bucket operations
aws --endpoint-url http://localhost:9000 s3 mb s3://mybucket
aws --endpoint-url http://localhost:9000 s3 ls
aws --endpoint-url http://localhost:9000 s3 rb s3://mybucket

# Object operations
aws --endpoint-url http://localhost:9000 s3 cp file.txt s3://mybucket/
aws --endpoint-url http://localhost:9000 s3 cp s3://mybucket/file.txt ./downloaded.txt
aws --endpoint-url http://localhost:9000 s3 ls s3://mybucket/
aws --endpoint-url http://localhost:9000 s3 rm s3://mybucket/file.txt

# Recursive operations
aws --endpoint-url http://localhost:9000 s3 sync ./local-dir s3://mybucket/prefix/
aws --endpoint-url http://localhost:9000 s3 rm s3://mybucket/ --recursive
```

### Example: mc (MinIO Client)

```bash
mc alias set local http://localhost:9000 minioadmin minioadmin
mc mb local/mybucket
mc cp file.txt local/mybucket/
mc ls local/mybucket/
mc cat local/mybucket/file.txt
mc rm local/mybucket/file.txt
mc rb local/mybucket
```

### Example: Python (boto3)

```python
import boto3

s3 = boto3.client('s3',
    endpoint_url='http://localhost:9000',
    aws_access_key_id='minioadmin',
    aws_secret_access_key='minioadmin',
    region_name='us-east-1'
)

# Create bucket
s3.create_bucket(Bucket='mybucket')

# Upload
s3.put_object(Bucket='mybucket', Key='hello.txt', Body=b'Hello World')

# Download
obj = s3.get_object(Bucket='mybucket', Key='hello.txt')
print(obj['Body'].read())

# List
for obj in s3.list_objects_v2(Bucket='mybucket')['Contents']:
    print(obj['Key'], obj['Size'])

# Delete
s3.delete_object(Bucket='mybucket', Key='hello.txt')
```

### Example: curl (manual SigV4)

```bash
# Health check (no auth required)
curl http://localhost:9000/health

# For S3 operations, use aws-cli or an SDK which handles SigV4 signing.
# Direct curl requires computing AWS Signature V4 manually.
```

## Admin API

These JSON endpoints are accessible without S3 auth (served under `/ui/api/`):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check (`{"status":"ok"}`) |
| `/ui/api/metrics` | GET | Request/bandwidth metrics |
| `/ui/api/server-info` | GET | Server version, uptime, storage totals |
| `/ui/api/buckets` | GET | Bucket list with per-bucket stats |
| `/ui/api/buckets/{name}/stats` | GET | Single bucket statistics |
| `/ui/api/cleanup` | POST | Trigger manual cleanup of expired multipart uploads and orphaned metadata |

### Metrics Response

```json
{
  "requests_total": 1234,
  "requests_get": 800,
  "requests_put": 300,
  "requests_delete": 100,
  "requests_head": 34,
  "bytes_in": 104857600,
  "bytes_out": 524288000,
  "errors_total": 2,
  "uptime_secs": 86400
}
```

### Server Info Response

```json
{
  "version": "0.1.0",
  "uptime_secs": 86400,
  "buckets": 5,
  "objects": 1234,
  "total_size": 1073741824,
  "total_size_human": "1.00 GB",
  "requests_total": 5000,
  "bytes_in": 104857600,
  "bytes_out": 524288000
}
```

## Web UI

The built-in web UI is available at `http://host:9000/ui`:

| Page | URL | Description |
|------|-----|-------------|
| Dashboard | `/ui` | Storage overview, metrics, recent buckets |
| Buckets | `/ui/buckets` | Create/delete buckets, view stats |
| Objects | `/ui/buckets/{name}` | Browse objects, folder navigation, download links |
| Upload | `/ui/buckets/{name}/upload` | Upload files via browser |
| Admin | `/ui/admin` | Server stats, per-bucket metrics, cleanup controls |
| Login | `/ui/login` | Credential verification page |

## Container

### Build

```bash
# x86_64 scratch image
podman build -t miniminio .

# ARM64 (modify Dockerfile target)
podman build --build-arg TARGET=aarch64-unknown-linux-musl -t miniminio:arm64 .
```

### Run

```bash
podman run -d \
  --name miniminio \
  -p 9000:9000 \
  -v /mnt/storage:/data \
  -e MINIMINIO_ACCESS_KEY=mykey \
  -e MINIMINIO_SECRET_KEY=mysecret \
  miniminio
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: miniminio
spec:
  replicas: 1
  selector:
    matchLabels:
      app: miniminio
  template:
    metadata:
      labels:
        app: miniminio
    spec:
      containers:
      - name: miniminio
        image: miniminio:latest
        ports:
        - containerPort: 9000
        env:
        - name: MINIMINIO_ACCESS_KEY
          valueFrom:
            secretKeyRef:
              name: miniminio-secret
              key: access-key
        - name: MINIMINIO_SECRET_KEY
          valueFrom:
            secretKeyRef:
              name: miniminio-secret
              key: secret-key
        volumeMounts:
        - name: data
          mountPath: /data
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: miniminio-data
```

## Build from Source

```bash
# Debug build
cargo build

# Release (static musl binary, ~5 MB stripped)
cargo build --release --target x86_64-unknown-linux-musl

# ARM64
cargo build --release --target aarch64-unknown-linux-musl

# Run tests
cargo test
```

## What's Not Implemented

MiniMinio is intentionally minimal. These S3 features are **not** supported:

- Virtual-hosted-style addressing (`bucket.host:port/key`)
- Bucket policies and ACLs
- Object versioning
- Lifecycle rules
- Cross-region replication
- Object lock / retention
- Bucket notifications (SNS/SQS/Lambda)
- Select object content (S3 Select)
- Server-side encryption (SSE)
- Multi-disk / erasure coding
- Bucket website hosting
- CORS configuration per bucket
- Tagging
