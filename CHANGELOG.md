# Changelog

## [v0.2.0] — 2026-02-28

### Added
- Upstream sync replication for hierarchical deployment
  - One-way push replication from local instances to upstream MiniMinio
  - Event-driven sync: bucket create/delete, object put/delete
  - Exponential backoff retry (5 retries, 1s base delay)
  - S3 SigV4-signed client for remote operations
  - Bucket prefix mapping (e.g. `edge01-mybucket` on upstream)
  - Sync status tracking: events sent/failed/pending, last sync timestamp
  - `/ui/api/sync-status` admin endpoint
  - Sync status panel in admin web UI
  - CLI flags: `--sync-endpoint`, `--sync-access-key`, `--sync-secret-key`, `--sync-region`, `--sync-bucket-prefix`
  - Hierarchy support via chaining (edge -> regional -> global)

## [v0.1.0] — 2026-02-28

### Added
- S3-compatible REST API (path-style)
  - Bucket operations: Create, Delete, Head, List
  - Object operations: Get, Put, Delete, Head, Copy, List (v1 & v2)
  - Multipart uploads: Initiate, Upload Part, Complete, Abort, List Parts
  - Batch delete (DeleteObjects)
  - Range requests for partial downloads
- AWS Signature V4 authentication (header and presigned URL)
- Filesystem-backed single-disk storage
- Web UI with dashboard, bucket browser, object browser, file upload
- Admin panel with metrics, per-bucket statistics, cleanup controls
- Background cleanup of expired multipart uploads and orphaned metadata
- Prometheus-compatible metrics tracking
- Static musl binary build (scratch container compatible)
- Dockerfile for minimal container image
