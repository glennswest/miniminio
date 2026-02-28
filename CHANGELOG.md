# Changelog

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
