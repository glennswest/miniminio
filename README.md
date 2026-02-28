# MiniMinio

Minimal S3-compatible object storage server. Single binary, single disk, scratch container.

## Features

- **S3 REST API** — Compatible with aws-cli, mc, boto3, and other S3 clients
- **Web UI** — Dashboard, bucket browser, object management, admin panel
- **Auth** — AWS Signature V4 (header-based and presigned URLs)
- **Storage** — Filesystem-backed, single disk
- **Metrics** — Request counting, bandwidth tracking, per-bucket stats
- **Cleanup** — Automatic multipart upload expiry, orphan metadata removal
- **Container** — Static musl binary, runs from scratch image (~5 MB)

## Quick Start

```bash
# Run directly
cargo run -- --data-dir ./data --port 9000

# With custom credentials
cargo run -- --access-key mykey --secret-key mysecret

# All options
miniminio --help
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MINIMINIO_DATA_DIR` | `./data` | Data directory |
| `MINIMINIO_PORT` | `9000` | Listen port |
| `MINIMINIO_HOST` | `0.0.0.0` | Listen address |
| `MINIMINIO_ACCESS_KEY` | `minioadmin` | Access key |
| `MINIMINIO_SECRET_KEY` | `minioadmin` | Secret key |
| `MINIMINIO_REGION` | `us-east-1` | S3 region |
| `MINIMINIO_MULTIPART_EXPIRY` | `24` | Multipart upload expiry (hours) |

## Using with S3 Clients

### aws-cli

```bash
aws --endpoint-url http://localhost:9000 s3 mb s3://mybucket
aws --endpoint-url http://localhost:9000 s3 cp file.txt s3://mybucket/
aws --endpoint-url http://localhost:9000 s3 ls s3://mybucket/
```

### mc (MinIO Client)

```bash
mc alias set local http://localhost:9000 minioadmin minioadmin
mc mb local/mybucket
mc cp file.txt local/mybucket/
mc ls local/mybucket/
```

## Container

```bash
# Build
podman build -t miniminio .

# Run
podman run -p 9000:9000 -v ./data:/data miniminio
```

## Endpoints

| Endpoint | Description |
|----------|-------------|
| `http://host:9000/` | S3 API root (ListBuckets) |
| `http://host:9000/ui` | Web UI dashboard |
| `http://host:9000/ui/buckets` | Bucket management |
| `http://host:9000/ui/admin` | Admin panel |
| `http://host:9000/health` | Health check |
| `http://host:9000/ui/api/metrics` | Metrics JSON |
| `http://host:9000/ui/api/server-info` | Server info JSON |

## Build

```bash
# Debug
cargo build

# Release (static musl)
cargo build --release --target x86_64-unknown-linux-musl

# ARM64
cargo build --release --target aarch64-unknown-linux-musl
```
