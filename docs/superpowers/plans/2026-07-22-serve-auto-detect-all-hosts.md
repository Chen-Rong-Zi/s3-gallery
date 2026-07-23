# Serve Auto-Detect All Hosts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `serve` command should auto-detect all hosts from the database instead of requiring a `host` argument.

**Architecture:** Add `endpoint` and `region` columns to `host_config`. During `scan`, persist all three connection parameters (bucket, endpoint, region). During `serve`, read all hosts from DB, group by endpoint to create S3 clients, and serve files from all hosts. The `--prefix` option filters by directory prefix.

**Tech Stack:** SQLite (sqlx), Rust, clap CLI, axum

---

## File Map

| File | Responsibility | Change |
|------|---------------|--------|
| `crates/s3-gallery-core/src/db/schema.rs` | DB migrations | Add ALTER TABLE for endpoint/region columns |
| `crates/s3-gallery-core/src/db/models.rs` | HostConfigEntry model | Add endpoint/region fields, add list_all, add upsert_host_config, remove upsert_bucket |
| `crates/s3-gallery-cli/src/cli.rs` | CLI argument definitions | Remove host from Serve, add --prefix |
| `crates/s3-gallery-cli/src/main.rs` | Entry point | Update Serve match arm |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Scan command | Save endpoint/region to DB, use upsert_host_config |
| `crates/s3-gallery-cli/src/cmd_serve.rs` | Serve command | Complete rewrite for multi-host auto-detect |
| `crates/s3-gallery-cli/src/web/state.rs` | AppState struct | Replace with multi-host state (db, hosts, s3_clients, prefix) |
| `crates/s3-gallery-cli/src/web/handlers/download.rs` | Download handler | Parse host_id from key, use correct S3 client |
| `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs` | Thumbnail handler | Parse host_id from key, use correct S3 client |
| `crates/s3-gallery-cli/src/web/handlers/file_detail.rs` | File detail handler | Parse host_id from key |
| `crates/s3-gallery-cli/src/web/handlers/browse.rs` | Browse handler | Query across all hosts |
| `crates/s3-gallery-cli/src/web/handlers/gallery.rs` | Gallery handler | Query across all hosts |
| `crates/s3-gallery-cli/src/web/handlers/dashboard.rs` | Dashboard handler | Return host list from state |
| `crates/s3-gallery-cli/src/web/handlers/{stats,search,tags,...}.rs` | Other handlers | Update host_id reference |
| `tests/db_test.rs` | Integration tests | Update if needed |

---

### Task 1: DB schema migration + model updates (endpoint, region, list_all, upsert_host_config)

**Files:**
- Modify: `crates/s3-gallery-core/src/db/schema.rs:48-51`
- Modify: `crates/s3-gallery-core/src/db/models.rs:17-131`

**Context:** The `host_config` table has `bucket` but needs `endpoint` and `region` columns. The `HostConfigEntry` struct needs these fields. We need a `list_all` method to read all hosts, and `upsert_host_config` to replace `upsert_bucket`.

- [ ] **Step 1: Add ALTER TABLE migrations for endpoint and region**

In `crates/s3-gallery-core/src/db/schema.rs`, after the existing bucket column migration (line 48-51), add:

```rust
// Attempt to add endpoint and region columns to host_config (ignore if already exist)
let _ = sqlx::query("ALTER TABLE host_config ADD COLUMN endpoint TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await;
let _ = sqlx::query("ALTER TABLE host_config ADD COLUMN region TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await;
```

- [ ] **Step 2: Add endpoint and region fields to HostConfigEntry**

Add these fields to the struct:

```rust
pub struct HostConfigEntry {
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub created_at: String,
    pub bucket: String,
    /// The OSS endpoint URL this host was scanned from.
    pub endpoint: String,
    /// The OSS region this host was scanned from.
    pub region: String,
}
```

- [ ] **Step 3: Update INSERT SQL to include endpoint and region**

```rust
"INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket, endpoint, region) \
 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
```

Add `.bind(&entry.endpoint)` and `.bind(&entry.region)` after the bucket bind.

- [ ] **Step 4: Update UPDATE SQL to include endpoint and region**

```rust
"UPDATE host_config SET host_name = ?, host_type = ?, description = ?, created_at = ?, bucket = ?, endpoint = ?, region = ? \
 WHERE host_id = ?"
```

Add `.bind(&entry.endpoint)` and `.bind(&entry.region)` after the bucket bind.

- [ ] **Step 5: Add `list_all` method**

```rust
/// List all host config entries, ordered by host_id.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the database operation fails.
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<HostConfigEntry>> {
    sqlx::query_as::<_, HostConfigEntry>("SELECT * FROM host_config ORDER BY host_id")
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list host configs: {e}")))
}
```

- [ ] **Step 6: Replace `upsert_bucket` with `upsert_host_config`**

```rust
/// Insert or update the host config for a scan result.
///
/// Creates a new row if the host_id doesn't exist, or updates the
/// bucket, endpoint, and region fields if it does.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the database operation fails.
pub async fn upsert_host_config(
    pool: &SqlitePool,
    host_id: &str,
    bucket: &str,
    endpoint: &str,
    region: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket, endpoint, region) \
         VALUES (?, 'unknown', 'unknown', '', datetime('now'), ?, ?, ?) \
         ON CONFLICT(host_id) DO UPDATE SET bucket = excluded.bucket, endpoint = excluded.endpoint, region = excluded.region"
    )
    .bind(host_id)
    .bind(bucket)
    .bind(endpoint)
    .bind(region)
    .execute(pool)
    .await
    .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert host config: {e}")))?;
    Ok(())
}
```

- [ ] **Step 7: Update test to include endpoint and region fields**

In `test_host_config_crud()`, update the test entry:

```rust
let entry = HostConfigEntry {
    host_id: "host-01".to_string(),
    host_name: "Primary Host".to_string(),
    host_type: "local".to_string(),
    description: "Main storage host".to_string(),
    created_at: "2026-01-01T00:00:00Z".to_string(),
    bucket: "".to_string(),
    endpoint: "".to_string(),
    region: "".to_string(),
};
```

Update `test_host_config_upsert_bucket` to test `upsert_host_config` instead:

```rust
#[tokio::test]
async fn test_host_config_upsert_host_config() -> Result<()> {
    let (pool, _dir) = setup_test_db().await?;

    // INSERT path: new host_id creates a row
    HostConfigEntry::upsert_host_config(&pool, "camera-1", "photos-bucket", "https://oss.example.com", "us-east-1").await?;
    let config = HostConfigEntry::get(&pool, "camera-1").await?;
    assert_eq!(config.bucket, "photos-bucket");
    assert_eq!(config.endpoint, "https://oss.example.com");
    assert_eq!(config.region, "us-east-1");

    // UPDATE path: existing host_id updates bucket, endpoint, region
    HostConfigEntry::upsert_host_config(&pool, "camera-1", "new-bucket", "https://oss2.example.com", "eu-west-1").await?;
    let config = HostConfigEntry::get(&pool, "camera-1").await?;
    assert_eq!(config.bucket, "new-bucket");
    assert_eq!(config.endpoint, "https://oss2.example.com");
    assert_eq!(config.region, "eu-west-1");

    Ok(())
}
```

- [ ] **Step 8: Build and test**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add crates/s3-gallery-core/src/db/schema.rs crates/s3-gallery-core/src/db/models.rs
git commit -m "feat: add endpoint/region to host_config, list_all, upsert_host_config"
```

---

### Task 2: CLI changes — remove host from serve, add --prefix

**Files:**
- Modify: `crates/s3-gallery-cli/src/cli.rs:105-117`
- Modify: `crates/s3-gallery-cli/src/main.rs:59-68`

**Context:** The `serve` subcommand no longer needs a `host` argument. Instead, it takes an optional `--prefix` for directory filtering.

- [ ] **Step 1: Update Serve variant in CLI**

```rust
/// Start the web server
#[command(alias = "web")]
Serve {
    /// File prefix filter (e.g. "photos/")
    #[arg(long)]
    prefix: Option<String>,
    /// Web server port
    #[arg(long, default_value = "8080")]
    port: u16,
    /// Read-only mode
    #[arg(long)]
    readonly: bool,
},
```

- [ ] **Step 2: Update main.rs match arm**

```rust
Commands::Serve {
    prefix,
    port,
    readonly,
} => {
    if let Err(e) = cmd_serve::run_serve(&cli, *port, *readonly, prefix.clone()).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation errors in cmd_serve.rs (function signature changed)

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/cli.rs crates/s3-gallery-cli/src/main.rs
git commit -m "feat: remove host arg from serve, add --prefix option"
```

---

### Task 3: Scan command — save endpoint and region to DB

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs:69-70`

**Context:** The scan command currently saves only the bucket name. Now it needs to also save the endpoint and region. Replace `upsert_bucket` with `upsert_host_config`.

- [ ] **Step 1: Replace upsert_bucket call with upsert_host_config**

```rust
// Save host config for serve to auto-detect
HostConfigEntry::upsert_host_config(&db_pool, &host_id.host_id, bucket_str, &cli.endpoint, &cli.region).await?;
```

- [ ] **Step 2: Build and test**

Run: `cargo build 2>&1 | tail -5`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "feat: save endpoint and region to host_config during scan"
```

---

### Task 4: AppState rewrite + serve command rewrite

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/state.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs`

**Context:** The current `AppState` holds a single `LocalView`, `RemoteView`, `bucket`, and `host_id`. The new design needs to hold all hosts from the DB, a map of S3 clients keyed by endpoint, and the database pool. The `serve` command reads all hosts, creates S3 clients, and builds the new AppState.

- [ ] **Step 1: Rewrite AppState**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::s3::client::S3Client;
use sqlx::SqlitePool;

/// Shared application state for multi-host serving.
#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub db: SqlitePool,
    /// All hosts from the database.
    pub hosts: Vec<HostConfigEntry>,
    /// S3 clients keyed by endpoint URL (endpoint "" = CLI default).
    pub s3_clients: HashMap<String, Arc<dyn S3Client>>,
    /// Optional prefix filter from CLI --prefix.
    pub prefix: Option<String>,
    /// CLI endpoint for fallback when host has no stored endpoint.
    pub cli_endpoint: String,
    /// CLI region for fallback when host has no stored region.
    pub cli_region: String,
    /// CLI access key for S3 connections.
    pub access_key: String,
    /// CLI secret key for S3 connections.
    pub secret_key: String,
}

impl AppState {
    /// Find a host config by host_id.
    pub fn get_host(&self, host_id: &str) -> Option<&HostConfigEntry> {
        self.hosts.iter().find(|h| h.host_id == host_id)
    }

    /// Get the effective endpoint for a host (stored or CLI fallback).
    pub fn effective_endpoint(&self, host: &HostConfigEntry) -> &str {
        if host.endpoint.is_empty() { &self.cli_endpoint } else { &host.endpoint }
    }

    /// Get the effective region for a host (stored or CLI fallback).
    pub fn effective_region(&self, host: &HostConfigEntry) -> &str {
        if host.region.is_empty() { &self.cli_region } else { &host.region }
    }

    /// Get the S3 client for a host's endpoint.
    pub fn get_s3_client(&self, host: &HostConfigEntry) -> Option<&Arc<dyn S3Client>> {
        let endpoint = self.effective_endpoint(host);
        self.s3_clients.get(endpoint)
    }
}
```

- [ ] **Step 2: Rewrite `run_serve` in cmd_serve.rs**

```rust
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;

use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::db::status::{check_db_status, decide_action, DbAction};
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::types::BucketName;

use crate::cli::Cli;
use crate::web::router::create_router;
use crate::web::state::AppState;

pub async fn run_serve(cli: &Cli, port: u16, readonly: bool, prefix: Option<String>) -> Result<()> {
    let db_path = &cli.db_path;

    // Check DB status
    let status = check_db_status(db_path).await?;
    let action = decide_action(&status, readonly);

    match action {
        DbAction::UseLocal => { /* Local DB exists, use it */ }
        DbAction::FullScan => {
            return Err(S3GalleryError::NotFound(
                "No database found. Run 's3-gallery scan <host>' first.".to_string(),
            ));
        }
        DbAction::Abort(msg) => {
            return Err(S3GalleryError::Internal(msg));
        }
    }

    // Create pool, run migrations
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;

    // Read all hosts from DB
    let hosts = HostConfigEntry::list_all(&pool).await?;
    if hosts.is_empty() {
        return Err(S3GalleryError::NotFound(
            "No hosts found in database. Run 's3-gallery scan <host>' first.".to_string(),
        ));
    }

    // Group hosts by endpoint and create S3 clients
    let mut s3_clients: HashMap<String, Arc<dyn S3Client>> = HashMap::new();
    for host in &hosts {
        let endpoint = if host.endpoint.is_empty() { &cli.endpoint } else { &host.endpoint };
        let region = if host.region.is_empty() { &cli.region } else { &host.region };

        if !s3_clients.contains_key(endpoint) {
            // Use a placeholder bucket; OssConfig needs one but we pass
            // the real bucket at request time via S3Client methods.
            let placeholder = BucketName::new("placeholder")
                .map_err(|_| S3GalleryError::Internal("invalid placeholder".to_string()))?;
            let config = OssConfig::validate(
                placeholder,
                endpoint,
                region,
                &cli.access_key,
                &cli.secret_key,
                10,
            )?;
            let client = RealS3Client::from_config(&config);
            s3_clients.insert(endpoint.clone(), Arc::new(client) as Arc<dyn S3Client>);
        }
    }

    // Register templates from s3-gallery-web
    let mut env = minijinja::Environment::new();
    let errors = s3_gallery_web::register_templates(&mut env);
    if !errors.is_empty() {
        tracing::warn!(
            "{} template(s) failed to register: {:?}",
            errors.len(),
            errors
        );
    }

    // Build AppState
    let app_state = AppState {
        templates: Arc::new(env),
        db: pool,
        hosts,
        s3_clients,
        prefix,
        cli_endpoint: cli.endpoint.clone(),
        cli_region: cli.region.clone(),
        access_key: cli.access_key.clone(),
        secret_key: cli.secret_key.clone(),
    };

    // Create router and start server
    let app = create_router(app_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("s3-gallery web server starting on {}", addr);
    tracing::info!("  Hosts: {}", app_state.hosts.iter().map(|h| h.host_id.as_str()).collect::<Vec<_>>().join(", "));
    tracing::info!("  Read-only: {}", readonly);

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| S3GalleryError::Internal(format!("Failed to bind to {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| S3GalleryError::Internal(format!("Server error: {e}")))?;

    Ok(())
}
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: Compilation errors in handlers (they still reference old AppState fields)

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/state.rs crates/s3-gallery-cli/src/cmd_serve.rs
git commit -m "feat: rewrite AppState and serve command for multi-host support"
```

---

### Task 5: Update download, thumbnail, and file_detail handlers

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/download.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/file_detail.rs`

**Context:** These handlers need to parse the host_id from the key (first segment before '/'), look up the host config to find the correct bucket, and use the correct S3 client. The key insight: the key's first segment is the host_id (e.g., key="photos/2023/autumn.jpg" → host_id="photos").

- [ ] **Step 1: Update download handler**

The download handler needs to:
1. Parse host_id from the key (first segment before '/')
2. Look up the host in state.hosts
3. Find the file in the DB using host_id + key
4. Get the S3 client for the host's endpoint
5. Download from S3

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use s3_gallery_core::{
    db::models::FileEntry,
    error::S3GalleryError,
    types::{BucketName, ObjectKey},
};

use crate::web::state::AppState;

/// Extract the host_id from the first segment of a key.
/// e.g., "photos/2023/autumn.jpg" -> ("photos", "2023/autumn.jpg")
fn split_host_and_key(key: &str) -> Option<(&str, &str)> {
    let slash = key.find('/')?;
    let host_id = &key[..slash];
    let rest = &key[slash + 1..];
    // The full key stored in DB includes the host prefix
    Some((host_id, key))
}

fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
}

fn content_type_for_download(file: &FileEntry) -> &str {
    match file.content_type.as_deref() {
        Some(ct) if !ct.is_empty() => ct,
        _ => "application/octet-stream",
    }
}

pub async fn download(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    tracing::info!(handler = "download", key = %key, "download requested");

    // Parse host_id from key
    let (host_id, full_key) = match split_host_and_key(&key) {
        Some(pair) => pair,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid key\",\"detail\":\"Key must start with host_id/\"}}").into_bytes(),
            ).into_response();
        }
    };

    // Find the host
    let host = match state.get_host(host_id) {
        Some(h) => h,
        None => {
            return (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"host not found\",\"detail\":\"No host: {host_id}\"}}").into_bytes(),
            ).into_response();
        }
    };

    let pool = &state.db;

    // Fetch file metadata from the database
    let file = match FileEntry::get_by_key(pool, host_id, full_key).await {
        Ok(f) => f,
        Err(e) => {
            return match e {
                S3GalleryError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    [("content-type", "application/json")],
                    format!("{{\"error\":\"file not found\",\"detail\":\"No file with key: {key}\"}}").into_bytes(),
                ).into_response(),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [("content-type", "application/json")],
                    format!("{{\"error\":\"database error\",\"detail\":\"{}\"}}", e).into_bytes(),
                ).into_response(),
            };
        }
    };

    // Validate the object key
    let object_key = match ObjectKey::new(full_key) {
        Ok(k) => k,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid key\",\"detail\":\"{e}\"}}").into_bytes(),
            ).into_response();
        }
    };

    // Get the S3 client and bucket for this host
    let bucket_name = match BucketName::new(&host.bucket) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid bucket\",\"detail\":\"{e}\"}}").into_bytes(),
            ).into_response();
        }
    };

    let s3 = match state.get_s3_client(host) {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"no S3 client\",\"detail\":\"No S3 client for host: {host_id}\"}}").into_bytes(),
            ).into_response();
        }
    };

    // Fetch file content from S3
    match s3.get_object(&bucket_name, &object_key).await {
        Ok(data) => {
            tracing::info!(handler = "download", key = %key, size = %data.len(), "file downloaded");
            let filename = file_name_from_key(&key);
            let content_type = content_type_for_download(&file);
            let content_length = file.size.to_string();

            (
                StatusCode::OK,
                [
                    ("content-type", content_type),
                    ("content-disposition", &format!("attachment; filename=\"{filename}\"")),
                    ("content-length", &content_length),
                ],
                data,
            ).into_response()
        }
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"file not found\",\"detail\":\"No file on S3: {key}\"}}").into_bytes(),
            ).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"S3 fetch failed\",\"detail\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into_bytes(),
            ).into_response()
        }
    }
}
```

- [ ] **Step 2: Update thumbnail handler**

Similar pattern: parse host_id from key, find the S3 client, generate thumbnail on-the-fly.

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use s3_gallery_core::{
    db::models::ThumbnailEntry,
    error::S3GalleryError,
    thumbnail::generator::generate_thumbnail,
    types::{BucketName, ObjectKey},
};
use chrono::Utc;

use crate::web::state::AppState;

fn split_host_and_key(key: &str) -> Option<(&str, &str)> {
    let slash = key.find('/')?;
    let host_id = &key[..slash];
    Some((host_id, key))
}

pub async fn thumbnail(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    tracing::info!(handler = "thumbnail", key = %key, "serving thumbnail");

    let pool = &state.db;

    // Try the database cache first
    match ThumbnailEntry::get(pool, &key).await {
        Ok(entry) => {
            return (
                StatusCode::OK,
                [
                    ("content-type", "image/jpeg"),
                    ("cache-control", "public, max-age=31536000"),
                ],
                entry.data,
            ).into_response();
        }
        Err(S3GalleryError::NotFound(_)) => { /* fall through to generate */ }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"database error\",\"detail\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into_bytes(),
            ).into_response();
        }
    }

    // Parse host_id from key
    let (host_id, full_key) = match split_host_and_key(&key) {
        Some(pair) => pair,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid key\"}}").into_bytes(),
            ).into_response();
        }
    };

    let host = match state.get_host(host_id) {
        Some(h) => h,
        None => {
            return (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"host not found\",\"detail\":\"{host_id}\"}}").into_bytes(),
            ).into_response();
        }
    };

    let object_key = match ObjectKey::new(full_key) {
        Ok(k) => k,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid key\",\"detail\":\"{e}\"}}").into_bytes(),
            ).into_response();
        }
    };

    let bucket_name = match BucketName::new(&host.bucket) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid bucket\",\"detail\":\"{e}\"}}").into_bytes(),
            ).into_response();
        }
    };

    let s3 = match state.get_s3_client(host) {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"no S3 client\",\"detail\":\"{host_id}\"}}").into_bytes(),
            ).into_response();
        }
    };

    // Fetch from S3 and generate thumbnail
    match s3.get_object(&bucket_name, &object_key).await {
        Ok(data) => {
            match generate_thumbnail(&data) {
                Ok(thumbnail_data) => {
                    // Cache locally
                    let _ = ThumbnailEntry::insert(pool, &ThumbnailEntry {
                        file_key: key.clone(),
                        data: thumbnail_data.clone(),
                        format: "jpeg".to_string(),
                        width: None,
                        height: None,
                        cached_at: Utc::now().to_rfc3339(),
                    }).await;

                    (
                        StatusCode::OK,
                        [
                            ("content-type", "image/jpeg"),
                            ("cache-control", "public, max-age=31536000"),
                        ],
                        thumbnail_data,
                    ).into_response()
                }
                Err(e) => {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [("content-type", "application/json")],
                        format!("{{\"error\":\"thumbnail generation failed\",\"detail\":\"{e}\"}}").into_bytes(),
                    ).into_response()
                }
            }
        }
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"file not found\",\"detail\":\"{key}\"}}").into_bytes(),
            ).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"S3 fetch failed\",\"detail\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into_bytes(),
            ).into_response()
        }
    }
}
```

- [ ] **Step 3: Update file_detail handler**

```rust
pub async fn file_detail(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    // Parse host_id from key
    let host_id = match key.find('/') {
        Some(slash) => &key[..slash],
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid key","detail":"Key must contain host_id/"}))).into_response();
        }
    };

    let pool = &state.db;

    // Fetch the file entry by its key
    let file = match FileEntry::get_by_key(pool, host_id, &key).await {
        Ok(file) => file,
        Err(e) => {
            return match e {
                S3GalleryError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error":"file not found","detail":format!("No file with key: {key}")})),
                ).into_response(),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"database error","detail":e.to_string()})),
                ).into_response(),
            };
        }
    };

    // Rest of the handler — fetch metadata, build context, render template

    // Fetch metadata entries for this file
    let metadata_entries = match MetadataEntry::get_by_file_key(pool, &key).await {
        Ok(entries) => entries,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"failed to fetch metadata","detail":e.to_string()}))).into_response();
        }
    };

    // Group metadata by namespace
    let mut metadata_by_namespace: BTreeMap<String, Vec<MetadataItem>> = BTreeMap::new();
    for entry in &metadata_entries {
        metadata_by_namespace.entry(entry.namespace.clone()).or_default().push(MetadataItem {
            key: entry.key.clone(),
            value: entry.value.clone(),
        });
    }

    let has_thumbnail = ThumbnailEntry::get(pool, &key).await.is_ok();
    let file_name = file_name_from_key(&key);
    let file_size_formatted = format_file_size(file.size);

    let context = json!({
        "file": {
            "key": file.key, "etag": file.etag, "size": file.size,
            "size_formatted": file_size_formatted, "last_modified": file.last_modified,
            "content_type": file.content_type, "file_type": file.file_type,
            "metadata_state": file.metadata_state, "name": file_name,
        },
        "metadata_by_namespace": metadata_by_namespace,
        "has_thumbnail": has_thumbnail,
    });

    match render_template(&state, "file_detail.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}
```

- [ ] **Step 4: Build to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: No errors in these files

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/download.rs crates/s3-gallery-cli/src/web/handlers/thumbnail.rs crates/s3-gallery-cli/src/web/handlers/file_detail.rs
git commit -m "feat: update download/thumbnail/file_detail handlers for multi-host"
```

---

### Task 6: Update browse, gallery, dashboard, and other handlers

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/browse.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/gallery.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/dashboard.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/search.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/tags.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/timeline.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/stats.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/duplicates.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/settings.rs`

**Context:** These handlers currently use `state.local_view.db()` and `state.host_id`. The new AppState replaces `local_view` with `db` directly, and `host_id` is no longer a single value. For browse/gallery, queries should be performed across all hosts. For the dashboard, show the host list.

- [ ] **Step 1: Update browse handler**

Replace `state.local_view.db()` with `&state.db` and `state.host_id` with a host_id parsed from the path prefix.

```rust
// Replace:
// let pool = state.local_view.db();
// state.local_view.list_directory(&state.host_id, path, ...)
//
// With:
let pool = &state.db;
// If path starts with a known host_id, use that host_id
// Otherwise, search across all hosts
let (host_id, prefix) = if let Some(slash) = path.find('/') {
    let first_seg = &path[..slash];
    if state.get_host(first_seg).is_some() {
        (first_seg.to_string(), path.to_string())
    } else {
        // Search across all hosts by matching key prefix
        // We can query using key LIKE pattern
        ("all".to_string(), path.to_string())
    }
} else if !path.is_empty() && state.get_host(path).is_some() {
    (path.to_string(), format!("{}/", path))
} else {
    // No specific host, list all top-level directories
    // This lists all host directories as browse roots
    let host_entries: Vec<_> = state.hosts.iter().map(|h| {
        EntryView {
            name: h.host_id.clone(),
            path: h.host_id.clone(),
            size: "-".to_string(),
            file_type: "directory".to_string(),
            last_modified: h.created_at.clone(),
            is_directory: true,
        }
    }).collect();
    // ... render with host list as entries
};
```

The simpler approach: for the browse handler, when no specific host is selected, show the list of hosts as directories. When a host is selected, delegate to the normal list_directory for that host.

For the actual implementation, the simplest correct approach is:

```rust
pub async fn browse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<BrowseQuery>,
) -> impl IntoResponse {
    let path = params.path.as_deref().unwrap_or("");
    // ...

    let pool = &state.db;

    // If path is empty, show root-level host directories
    if path.is_empty() {
        let entry_views: Vec<EntryView> = state.hosts.iter().map(|h| {
            EntryView {
                name: h.host_id.clone(),
                path: h.host_id.clone(),
                size: "-".to_string(),
                file_type: "directory".to_string(),
                last_modified: h.created_at.clone(),
                is_directory: true,
            }
        }).collect();

        // ... render with breadcrumbs empty
        let context = build_context(&entry_views, &[], &sort_by_str, &sort_order_str);
        return match render_template(&state, "browse.html", &context) {
            Ok(html) => html.into_response(),
            Err(response) => *response,
        };
    }

    // Parse host_id from path (first segment)
    let host_id = match path.find('/') {
        Some(slash) => &path[..slash],
        None => path,
    };

    // Validate host_id is known
    if state.get_host(host_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"host not found","detail":format!("No host: {host_id}")})),
        ).into_response();
    }

    // Query files for this host using LocalView pattern
    let entries = match ls::list_directory(pool, host_id, path, sort_field, sort_order).await {
        Ok(entries) => entries,
        Err(e) => { /* error handling */ }
    };
    // Convert LsEntry to EntryView
    let effective_prefix = if path.is_empty() { String::new() } else { format!("{}/", path.trim_end_matches('/')) };
    let entry_views: Vec<EntryView> = entries.iter().map(|entry| {
        let entry_path = if effective_prefix.is_empty() { entry.name.clone() } else { format!("{}{}", effective_prefix, entry.name) };
        EntryView {
            name: entry.name.clone(),
            path: entry_path,
            size: entry.size.to_string(),
            file_type: entry.file_type.to_string(),
            last_modified: entry.last_modified.clone(),
            is_directory: entry.is_directory,
        }
    }).collect();

    let breadcrumbs = generate_breadcrumbs(path);
    let context = build_context(&entry_views, &breadcrumbs, &sort_by_str, &sort_order_str);

    let is_htmx = headers.get("HX-Request").and_then(|v| v.to_str().ok()).is_some_and(|v| v == "true");
    let template_name = if is_htmx { "browse_table.html" } else { "browse.html" };

    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}
```

Note: Need to add `use s3_gallery_core::view::ls;` to imports.

- [ ] **Step 2: Update gallery handler**

Replace `state.local_view.db()` with `&state.db` and `state.host_id` with a query across all hosts:

```rust
// Current: WHERE host_id = ? AND file_type IN (...)
// New: WHERE file_type IN (...) (query across all hosts by removing host_id filter)
```

Remove the `host_id = ?` bind from the SQL query and the `.bind(&state.host_id)` call.

- [ ] **Step 3: Update dashboard handler**

Show the list of hosts from state:

```rust
pub async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let hosts: Vec<_> = state.hosts.iter().map(|h| {
        serde_json::json!({
            "host_id": h.host_id,
            "bucket": h.bucket,
            "endpoint": h.endpoint,
            "region": h.region,
            "created_at": h.created_at,
        })
    }).collect();

    Json(serde_json::json!({
        "status": "ok",
        "service": "s3-gallery-web",
        "hosts": hosts,
        "prefix": state.prefix,
    }))
}
```

- [ ] **Step 4: Update remaining handlers**

For each of these handlers, replace `state.local_view.db()` with `&state.db` and `state.host_id` with a host_id parsed from context or a parameter:

**search.rs** — Replace `state.local_view.db()` with `&state.db`, `state.host_id` can be parsed from query params or search across all hosts.

**tags.rs** — Replace `state.local_view.db()` with `&state.db`, use host_id from query params.

**timeline.rs** — Replace `state.local_view.db()` with `&state.db`, use host_id from query params or search across all hosts.

**stats.rs** — Replace `state.local_view.db()` with `&state.db`, use host_id from query params.

**duplicates.rs** — Replace `state.local_view.db()` with `&state.db`, use host_id from query params.

**settings.rs** — Replace `state.local_view.db()` with `&state.db`.

The pattern for these is:

```rust
// Before:
let pool = state.local_view.db();
// After:
let pool = &state.db;

// Before:
state.local_view.list_directory(&state.host_id, ...)
// After:
s3_gallery_core::view::ls::list_directory(pool, host_id, ...).await
```

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1 | tail -20`
Expected: No errors

Run: `cargo test --workspace 2>&1 | tail -10`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/
git commit -m "feat: update all handlers for multi-host AppState"
```

---

### Task 7: Final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -10`
Expected: No warnings (or only pre-existing ones)

- [ ] **Step 2: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: All tests pass

- [ ] **Step 3: Verify build**

Run: `cargo build --release 2>&1 | tail -5`
Expected: Build succeeds

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore: final verification before merge"
```