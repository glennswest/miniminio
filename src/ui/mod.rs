use axum::body::Bytes;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use std::collections::HashMap;

use crate::admin::metrics::format_bytes;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui", get(dashboard))
        .route("/ui/login", get(login_page).post(login_post))
        .route("/ui/buckets", get(buckets_page))
        .route("/ui/buckets/:bucket", get(objects_page))
        .route("/ui/buckets/:bucket/create", post(create_bucket_action))
        .route("/ui/buckets/:bucket/delete", post(delete_bucket_action))
        .route(
            "/ui/buckets/:bucket/upload",
            get(upload_page).post(upload_action),
        )
        .route(
            "/ui/buckets/:bucket/delete-object",
            post(delete_object_action),
        )
        .route("/ui/admin", get(admin_page))
        .route("/ui/admin/cleanup", post(cleanup_action))
}

fn page(title: &str, content: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — MiniMinio</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #f5f5f5; color: #333; }}
.layout {{ display: flex; min-height: 100vh; }}
.sidebar {{ width: 220px; background: #1a1a2e; color: #fff; padding: 20px 0; flex-shrink: 0; }}
.sidebar h1 {{ font-size: 20px; padding: 0 20px 20px; border-bottom: 1px solid #333; margin-bottom: 10px; }}
.sidebar h1 span {{ color: #e94560; }}
.sidebar a {{ display: block; padding: 10px 20px; color: #ccc; text-decoration: none; font-size: 14px; }}
.sidebar a:hover, .sidebar a.active {{ background: #16213e; color: #fff; }}
.main {{ flex: 1; padding: 30px; max-width: 1200px; }}
.main h2 {{ margin-bottom: 20px; color: #1a1a2e; }}
.card {{ background: #fff; border-radius: 8px; padding: 20px; margin-bottom: 20px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
.card h3 {{ margin-bottom: 12px; color: #1a1a2e; font-size: 16px; }}
.stats {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin-bottom: 24px; }}
.stat {{ background: #fff; border-radius: 8px; padding: 20px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
.stat .label {{ font-size: 12px; color: #888; text-transform: uppercase; letter-spacing: 1px; }}
.stat .value {{ font-size: 28px; font-weight: 700; color: #1a1a2e; margin-top: 4px; }}
table {{ width: 100%; border-collapse: collapse; }}
th {{ text-align: left; padding: 10px 12px; border-bottom: 2px solid #eee; font-size: 12px; text-transform: uppercase; color: #888; letter-spacing: 0.5px; }}
td {{ padding: 10px 12px; border-bottom: 1px solid #eee; font-size: 14px; }}
tr:hover td {{ background: #f8f9fa; }}
a.link {{ color: #e94560; text-decoration: none; }}
a.link:hover {{ text-decoration: underline; }}
.btn {{ display: inline-block; padding: 8px 16px; border-radius: 4px; border: none; cursor: pointer; font-size: 14px; text-decoration: none; }}
.btn-primary {{ background: #e94560; color: #fff; }}
.btn-primary:hover {{ background: #c73e54; }}
.btn-danger {{ background: #dc3545; color: #fff; }}
.btn-danger:hover {{ background: #bd2130; }}
.btn-sm {{ padding: 4px 10px; font-size: 12px; }}
input[type="text"], input[type="password"], input[type="file"] {{ padding: 8px 12px; border: 1px solid #ddd; border-radius: 4px; font-size: 14px; width: 100%; max-width: 400px; }}
input[type="text"]:focus, input[type="password"]:focus {{ outline: none; border-color: #e94560; }}
.form-group {{ margin-bottom: 16px; }}
.form-group label {{ display: block; margin-bottom: 4px; font-size: 14px; font-weight: 500; }}
.breadcrumb {{ margin-bottom: 20px; font-size: 14px; color: #888; }}
.breadcrumb a {{ color: #e94560; text-decoration: none; }}
.empty {{ text-align: center; padding: 40px; color: #888; }}
.flash {{ padding: 12px 16px; border-radius: 4px; margin-bottom: 16px; }}
.flash-success {{ background: #d4edda; color: #155724; }}
.flash-error {{ background: #f8d7da; color: #721c24; }}
.inline-form {{ display: inline; }}
</style>
</head>
<body>
<div class="layout">
<nav class="sidebar">
<h1><span>Mini</span>Minio</h1>
<a href="/ui">Dashboard</a>
<a href="/ui/buckets">Buckets</a>
<a href="/ui/admin">Admin</a>
</nav>
<div class="main">
{content}
</div>
</div>
</body>
</html>"#,
    ))
}

async fn dashboard(State(state): State<AppState>) -> Html<String> {
    let total = state.storage.total_stats().await.unwrap_or_default();
    let buckets = state.storage.list_buckets().await.unwrap_or_default();
    let snap = state.metrics.snapshot();

    page(
        "Dashboard",
        &format!(
            r#"<h2>Dashboard</h2>
<div class="stats">
<div class="stat"><div class="label">Buckets</div><div class="value">{}</div></div>
<div class="stat"><div class="label">Objects</div><div class="value">{}</div></div>
<div class="stat"><div class="label">Storage Used</div><div class="value">{}</div></div>
<div class="stat"><div class="label">Requests</div><div class="value">{}</div></div>
</div>
<div class="stats">
<div class="stat"><div class="label">Data In</div><div class="value">{}</div></div>
<div class="stat"><div class="label">Data Out</div><div class="value">{}</div></div>
<div class="stat"><div class="label">Uptime</div><div class="value">{}</div></div>
<div class="stat"><div class="label">Errors</div><div class="value">{}</div></div>
</div>
<div class="card">
<h3>Recent Buckets</h3>
{}
</div>"#,
            buckets.len(),
            total.object_count,
            format_bytes(total.total_size),
            snap.requests_total,
            format_bytes(snap.bytes_in),
            format_bytes(snap.bytes_out),
            format_uptime(snap.uptime_secs),
            snap.errors_total,
            if buckets.is_empty() {
                "<div class=\"empty\">No buckets yet. <a href=\"/ui/buckets\" class=\"link\">Create one</a></div>".to_string()
            } else {
                let mut table = String::from(
                    "<table><tr><th>Name</th><th>Created</th><th>Objects</th><th>Size</th></tr>",
                );
                for b in buckets.iter().take(10) {
                    let stats = state.storage.bucket_stats(&b.name).await.unwrap_or_default();
                    table.push_str(&format!(
                        r#"<tr><td><a href="/ui/buckets/{}" class="link">{}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>"#,
                        b.name, b.name, &b.created[..10], stats.object_count, format_bytes(stats.total_size)
                    ));
                }
                table.push_str("</table>");
                table
            }
        ),
    )
}

async fn login_page() -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html><head>
<meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Login — MiniMinio</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #1a1a2e; display: flex; justify-content: center; align-items: center; min-height: 100vh; }}
.login-box {{ background: #fff; padding: 40px; border-radius: 8px; width: 360px; box-shadow: 0 4px 20px rgba(0,0,0,0.3); }}
.login-box h1 {{ text-align: center; margin-bottom: 24px; color: #1a1a2e; }}
.login-box h1 span {{ color: #e94560; }}
.form-group {{ margin-bottom: 16px; }}
.form-group label {{ display: block; margin-bottom: 4px; font-size: 14px; }}
.form-group input {{ width: 100%; padding: 10px; border: 1px solid #ddd; border-radius: 4px; font-size: 14px; }}
.btn {{ width: 100%; padding: 12px; background: #e94560; color: #fff; border: none; border-radius: 4px; font-size: 16px; cursor: pointer; }}
.btn:hover {{ background: #c73e54; }}
</style>
</head><body>
<div class="login-box">
<h1><span>Mini</span>Minio</h1>
<form method="post" action="/ui/login">
<div class="form-group"><label>Access Key</label><input type="text" name="access_key" required></div>
<div class="form-group"><label>Secret Key</label><input type="password" name="secret_key" required></div>
<button type="submit" class="btn">Login</button>
</form>
</div>
</body></html>"#
    ))
}

async fn login_post(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<HashMap<String, String>>,
) -> Response {
    let ak = form.get("access_key").map(|s| s.as_str()).unwrap_or("");
    let sk = form.get("secret_key").map(|s| s.as_str()).unwrap_or("");
    if ak == state.config.access_key && sk == state.config.secret_key {
        Redirect::to("/ui").into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Html("<h2>Invalid credentials</h2><a href=\"/ui/login\">Try again</a>".to_string())).into_response()
    }
}

async fn buckets_page(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let buckets = state.storage.list_buckets().await.unwrap_or_default();
    let flash = params.get("msg").map(|m| format!(r#"<div class="flash flash-success">{m}</div>"#)).unwrap_or_default();
    let error = params.get("error").map(|m| format!(r#"<div class="flash flash-error">{m}</div>"#)).unwrap_or_default();

    let mut table = String::from(
        r#"<table><tr><th>Name</th><th>Created</th><th>Objects</th><th>Size</th><th>Actions</th></tr>"#,
    );
    for b in &buckets {
        let stats = state.storage.bucket_stats(&b.name).await.unwrap_or_default();
        table.push_str(&format!(
            r#"<tr>
<td><a href="/ui/buckets/{name}" class="link">{name}</a></td>
<td>{created}</td>
<td>{objects}</td>
<td>{size}</td>
<td>
<form class="inline-form" method="post" action="/ui/buckets/{name}/delete" onsubmit="return confirm('Delete bucket {name}?')">
<button type="submit" class="btn btn-danger btn-sm">Delete</button>
</form>
</td>
</tr>"#,
            name = b.name,
            created = &b.created[..10.min(b.created.len())],
            objects = stats.object_count,
            size = format_bytes(stats.total_size),
        ));
    }
    table.push_str("</table>");

    page(
        "Buckets",
        &format!(
            r#"<h2>Buckets</h2>
{flash}{error}
<div class="card">
<h3>Create Bucket</h3>
<form method="post" action="/ui/buckets/new/create" style="display:flex;gap:8px;align-items:end">
<div class="form-group" style="margin:0"><label>Bucket Name</label><input type="text" name="name" required pattern="[a-z0-9][a-z0-9.\-]{{2,62}}" title="3-63 chars, lowercase alphanumeric, hyphens, periods"></div>
<button type="submit" class="btn btn-primary">Create</button>
</form>
</div>
<div class="card">
<h3>All Buckets ({count})</h3>
{table_or_empty}
</div>"#,
            count = buckets.len(),
            table_or_empty = if buckets.is_empty() {
                "<div class=\"empty\">No buckets yet</div>".to_string()
            } else {
                table
            }
        ),
    )
}

async fn objects_page(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if !state.storage.bucket_exists(&bucket).await {
        return Redirect::to("/ui/buckets?error=Bucket+not+found").into_response();
    }

    let prefix = params.get("prefix").map(|s| s.as_str()).unwrap_or("");
    let flash = params.get("msg").map(|m| format!(r#"<div class="flash flash-success">{m}</div>"#)).unwrap_or_default();

    let result = state
        .storage
        .list_objects(&bucket, prefix, "/", 1000, "", "")
        .await
        .unwrap_or(crate::storage::metadata::ListObjectsResult {
            objects: vec![],
            common_prefixes: vec![],
            is_truncated: false,
            next_continuation_token: None,
        });

    let mut rows = String::new();

    // Show parent prefix link
    if !prefix.is_empty() {
        let parent = if let Some(pos) = prefix[..prefix.len()-1].rfind('/') {
            &prefix[..=pos]
        } else {
            ""
        };
        rows.push_str(&format!(
            r#"<tr><td><a href="/ui/buckets/{bucket}?prefix={parent}" class="link">..</a></td><td>—</td><td>—</td><td></td></tr>"#
        ));
    }

    // Show folders (common prefixes)
    for cp in &result.common_prefixes {
        let display = cp.strip_prefix(prefix).unwrap_or(cp);
        rows.push_str(&format!(
            r#"<tr><td><a href="/ui/buckets/{bucket}?prefix={cp}" class="link">{display}</a></td><td>—</td><td>Folder</td><td></td></tr>"#
        ));
    }

    // Show objects
    for obj in &result.objects {
        let display = obj.key.strip_prefix(prefix).unwrap_or(&obj.key);
        rows.push_str(&format!(
            r#"<tr>
<td><a href="/{bucket}/{key}" class="link" target="_blank">{display}</a></td>
<td>{size}</td>
<td>{modified}</td>
<td>
<form class="inline-form" method="post" action="/ui/buckets/{bucket}/delete-object" onsubmit="return confirm('Delete {display}?')">
<input type="hidden" name="key" value="{key}">
<button type="submit" class="btn btn-danger btn-sm">Delete</button>
</form>
</td>
</tr>"#,
            key = obj.key,
            display = display,
            size = format_bytes(obj.size),
            modified = &obj.last_modified[..19.min(obj.last_modified.len())],
            bucket = bucket,
        ));
    }

    let breadcrumb = build_breadcrumb(&bucket, prefix);

    page(
        &format!("{bucket}"),
        &format!(
            r#"<div class="breadcrumb">{breadcrumb}</div>
{flash}
<div class="card" style="display:flex;gap:8px;align-items:center">
<a href="/ui/buckets/{bucket}/upload?prefix={prefix}" class="btn btn-primary">Upload File</a>
</div>
<div class="card">
<h3>Objects</h3>
{table_or_empty}
</div>"#,
            table_or_empty = if rows.is_empty() {
                "<div class=\"empty\">No objects in this location</div>".to_string()
            } else {
                format!("<table><tr><th>Name</th><th>Size</th><th>Modified</th><th>Actions</th></tr>{rows}</table>")
            }
        ),
    )
    .into_response()
}

async fn upload_page(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let prefix = params.get("prefix").map(|s| s.as_str()).unwrap_or("");
    page(
        "Upload",
        &format!(
            r#"<div class="breadcrumb"><a href="/ui/buckets">Buckets</a> / <a href="/ui/buckets/{bucket}">{bucket}</a> / Upload</div>
<div class="card">
<h3>Upload File</h3>
<form method="post" action="/ui/buckets/{bucket}/upload?prefix={prefix}" enctype="multipart/form-data">
<div class="form-group"><label>Key prefix</label><input type="text" name="prefix" value="{prefix}" placeholder="e.g. photos/2024/"></div>
<div class="form-group"><label>File</label><input type="file" name="file" required></div>
<button type="submit" class="btn btn-primary">Upload</button>
</form>
</div>"#,
        ),
    )
}

async fn upload_action(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    mut multipart: Multipart,
) -> Response {
    let mut prefix = String::new();
    let mut filename = String::new();
    let mut data = Vec::new();
    let mut content_type = "application/octet-stream".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "prefix" => {
                prefix = field.text().await.unwrap_or_default();
            }
            "file" => {
                filename = field.file_name().unwrap_or("unnamed").to_string();
                if let Some(ct) = field.content_type() {
                    content_type = ct.to_string();
                }
                data = field.bytes().await.unwrap_or_default().to_vec();
            }
            _ => {}
        }
    }

    if filename.is_empty() || data.is_empty() {
        return Redirect::to(&format!(
            "/ui/buckets/{bucket}?prefix={prefix}"
        ))
        .into_response();
    }

    let key = format!("{prefix}{filename}");
    match state
        .storage
        .put_object(&bucket, &key, &data, &content_type, HashMap::new())
        .await
    {
        Ok(_) => Redirect::to(&format!(
            "/ui/buckets/{bucket}?prefix={prefix}&msg=Uploaded+{filename}"
        ))
        .into_response(),
        Err(e) => Redirect::to(&format!(
            "/ui/buckets/{bucket}?prefix={prefix}&error={e}"
        ))
        .into_response(),
    }
}

async fn create_bucket_action(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<HashMap<String, String>>,
) -> Response {
    let name = form.get("name").map(|s| s.as_str()).unwrap_or("");
    if name.is_empty() {
        return Redirect::to("/ui/buckets?error=Name+required").into_response();
    }
    match state.storage.create_bucket(name).await {
        Ok(()) => Redirect::to(&format!("/ui/buckets?msg=Bucket+{name}+created")).into_response(),
        Err(e) => Redirect::to(&format!("/ui/buckets?error={e}")).into_response(),
    }
}

async fn delete_bucket_action(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Response {
    match state.storage.delete_bucket(&bucket).await {
        Ok(()) => {
            Redirect::to(&format!("/ui/buckets?msg=Bucket+{bucket}+deleted")).into_response()
        }
        Err(e) => Redirect::to(&format!("/ui/buckets?error={e}")).into_response(),
    }
}

async fn delete_object_action(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    axum::Form(form): axum::Form<HashMap<String, String>>,
) -> Response {
    let key = form.get("key").map(|s| s.as_str()).unwrap_or("");
    if key.is_empty() {
        return Redirect::to(&format!("/ui/buckets/{bucket}")).into_response();
    }

    let prefix = if let Some(pos) = key.rfind('/') {
        &key[..=pos]
    } else {
        ""
    };

    match state.storage.delete_object(&bucket, key).await {
        Ok(()) => Redirect::to(&format!(
            "/ui/buckets/{bucket}?prefix={prefix}&msg=Deleted"
        ))
        .into_response(),
        Err(e) => Redirect::to(&format!(
            "/ui/buckets/{bucket}?prefix={prefix}&error={e}"
        ))
        .into_response(),
    }
}

async fn admin_page(State(state): State<AppState>) -> Html<String> {
    let snap = state.metrics.snapshot();
    let total = state.storage.total_stats().await.unwrap_or_default();
    let buckets = state.storage.list_buckets().await.unwrap_or_default();

    let mut bucket_rows = String::new();
    for b in &buckets {
        let stats = state.storage.bucket_stats(&b.name).await.unwrap_or_default();
        let bm = state.metrics.bucket_stats.read().await;
        let bmetrics = bm.get(&b.name);
        bucket_rows.push_str(&format!(
            r#"<tr><td>{name}</td><td>{objects}</td><td>{size}</td><td>{requests}</td><td>{bin}</td><td>{bout}</td></tr>"#,
            name = b.name,
            objects = stats.object_count,
            size = format_bytes(stats.total_size),
            requests = bmetrics.map(|m| m.requests).unwrap_or(0),
            bin = format_bytes(bmetrics.map(|m| m.bytes_in).unwrap_or(0)),
            bout = format_bytes(bmetrics.map(|m| m.bytes_out).unwrap_or(0)),
        ));
    }

    page(
        "Admin",
        &format!(
            r#"<h2>Administration</h2>
<div class="stats">
<div class="stat"><div class="label">Version</div><div class="value">v{version}</div></div>
<div class="stat"><div class="label">Uptime</div><div class="value">{uptime}</div></div>
<div class="stat"><div class="label">Total Storage</div><div class="value">{storage}</div></div>
<div class="stat"><div class="label">Total Objects</div><div class="value">{objects}</div></div>
</div>

<div class="card">
<h3>Request Statistics</h3>
<table>
<tr><th>Metric</th><th>Value</th></tr>
<tr><td>Total Requests</td><td>{req_total}</td></tr>
<tr><td>GET Requests</td><td>{req_get}</td></tr>
<tr><td>PUT Requests</td><td>{req_put}</td></tr>
<tr><td>DELETE Requests</td><td>{req_del}</td></tr>
<tr><td>HEAD Requests</td><td>{req_head}</td></tr>
<tr><td>Errors</td><td>{errors}</td></tr>
<tr><td>Data In</td><td>{data_in}</td></tr>
<tr><td>Data Out</td><td>{data_out}</td></tr>
</table>
</div>

<div class="card">
<h3>Per-Bucket Statistics</h3>
{bucket_table}
</div>

<div class="card">
<h3>Maintenance</h3>
<p style="margin-bottom:12px">Clean up expired multipart uploads and orphaned metadata files.</p>
<form method="post" action="/ui/admin/cleanup">
<button type="submit" class="btn btn-primary">Run Cleanup</button>
</form>
</div>"#,
            version = env!("CARGO_PKG_VERSION"),
            uptime = format_uptime(snap.uptime_secs),
            storage = format_bytes(total.total_size),
            objects = total.object_count,
            req_total = snap.requests_total,
            req_get = snap.requests_get,
            req_put = snap.requests_put,
            req_del = snap.requests_delete,
            req_head = snap.requests_head,
            errors = snap.errors_total,
            data_in = format_bytes(snap.bytes_in),
            data_out = format_bytes(snap.bytes_out),
            bucket_table = if bucket_rows.is_empty() {
                "<div class=\"empty\">No buckets</div>".to_string()
            } else {
                format!(
                    "<table><tr><th>Bucket</th><th>Objects</th><th>Size</th><th>Requests</th><th>Data In</th><th>Data Out</th></tr>{bucket_rows}</table>"
                )
            },
        ),
    )
}

async fn cleanup_action(State(state): State<AppState>) -> Response {
    let mp =
        crate::storage::cleanup::cleanup_multipart(&state.storage, state.config.multipart_expiry_hours)
            .await;
    let orphan = crate::storage::cleanup::cleanup_orphaned_metadata(&state.storage).await;
    Redirect::to(&format!(
        "/ui/admin?msg=Cleaned+{mp}+multipart+{orphan}+orphans"
    ))
    .into_response()
}

fn build_breadcrumb(bucket: &str, prefix: &str) -> String {
    let mut bc = format!(r#"<a href="/ui/buckets">Buckets</a> / <a href="/ui/buckets/{bucket}">{bucket}</a>"#);
    if !prefix.is_empty() {
        let parts: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();
        let mut path = String::new();
        for (i, part) in parts.iter().enumerate() {
            path.push_str(part);
            path.push('/');
            if i < parts.len() - 1 {
                bc.push_str(&format!(
                    r#" / <a href="/ui/buckets/{bucket}?prefix={path}">{part}</a>"#
                ));
            } else {
                bc.push_str(&format!(" / {part}"));
            }
        }
    }
    bc
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {mins}m")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m {secs_rem}s", secs_rem = secs % 60)
    }
}
