# Serve Auto-Detect Bucket from DB Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `serve` command should auto-detect the bucket name from the local database, instead of requiring `--bucket` on the CLI.

**Architecture:** Add a `bucket` column to `host_config` table. During `scan`, persist the bucket name. During `serve`, read it back. `--bucket` becomes optional for `serve` (falls back to DB), but remains required for `scan` and `db` commands.

**Tech Stack:** SQLite (sqlx), Rust, clap CLI

---

## File Map

| File | Responsibility | Change |
|------|---------------|--------|
| `crates/s3-gallery-core/src/db/schema.rs` | DB migrations | Add ALTER TABLE for bucket column |
| `crates/s3-gallery-core/src/db/models.rs` | HostConfigEntry model | Add bucket field, add upsert_bucket method, update insert/update SQL |
| `crates/s3-gallery-cli/src/cli.rs` | CLI argument definitions | Make `--bucket` Option<String> |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Scan command handler | Save bucket to DB after scan, validate bucket is provided |
| `crates/s3-gallery-cli/src/cmd_serve.rs` | Serve command handler | Read bucket from DB if not in CLI |
| `crates/s3-gallery-cli/src/cmd_db.rs` | DB command handler | Validate bucket is provided |

---

### Task 1: DB schema migration + model updates

**Files:**
- Modify: `crates/s3-gallery-core/src/db/schema.rs:75-80`
- Modify: `crates/s3-gallery-core/src/db/models.rs:17-100`

**Context:** The `host_config` table exists but has no `bucket` column. The `HostConfigEntry` struct has no `bucket` field. We need both, plus a migration that adds the column to existing databases, and a new `upsert_bucket` method.

- [ ] **Step 1: Add ALTER TABLE migration to schema.rs**

After the `host_config` CREATE TABLE block (line 46), add the ALTER TABLE migration:

```rust
// Attempt to add bucket column to host_config (ignore if already exists)
let _ = sqlx::query("ALTER TABLE host_config ADD COLUMN bucket TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await;
```

- [ ] **Step 2: Add `bucket` field to HostConfigEntry struct**

```rust
pub struct HostConfigEntry {
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub created_at: String,
    /// The OSS bucket name this host was scanned from.
    pub bucket: String,
}
```

- [ ] **Step 3: Update INSERT SQL in HostConfigEntry::insert**

Change the insert query to include the `bucket` column:

```rust
"INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket) \
 VALUES (?, ?, ?, ?, ?, ?)"
```

And add a `.bind(&entry.bucket)` after the created_at bind.

- [ ] **Step 4: Update UPDATE SQL in HostConfigEntry::update**

Change the update query to include `bucket`:

```rust
"UPDATE host_config SET host_name = ?, host_type = ?, description = ?, created_at = ?, bucket = ? \
 WHERE host_id = ?"
```

And add `.bind(&entry.bucket)` after the created_at bind.

- [ ] **Step 5: Add `upsert_bucket` method to HostConfigEntry**

```rust
/// Insert or update the bucket field for a host.
///
/// Creates a new row if the host_id doesn't exist, or updates only the
/// bucket field if it does.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the database operation fails.
pub async fn upsert_bucket(
    pool: &SqlitePool,
    host_id: &str,
    bucket: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket) \
         VALUES (?, 'unknown', 'unknown', '', datetime('now'), ?) \
         ON CONFLICT(host_id) DO UPDATE SET bucket = excluded.bucket"
    )
    .bind(host_id)
    .bind(bucket)
    .execute(pool)
    .await
    .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert bucket: {e}")))?;
    Ok(())
}
```

- [ ] **Step 6: Update test to include bucket field**

In `test_host_config_crud()` (line 669), add `bucket: "".to_string()` to the test entry:

```rust
let entry = HostConfigEntry {
    host_id: "host-01".to_string(),
    host_name: "Primary Host".to_string(),
    host_type: "local".to_string(),
    description: "Main storage host".to_string(),
    created_at: "2026-01-01T00:00:00Z".to_string(),
    bucket: "".to_string(),
};
```

- [ ] **Step 7: Build and test**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 8: Commit**

```bash
git add crates/s3-gallery-core/src/db/schema.rs crates/s3-gallery-core/src/db/models.rs
git commit -m "feat: add bucket column to host_config table and upsert_bucket method"
```

---

### Task 2: CLI change — make `--bucket` optional

**Files:**
- Modify: `crates/s3-gallery-cli/src/cli.rs:44-46`

**Context:** The `--bucket` argument is currently a required `String`. It needs to become `Option<String>` so that `serve` can omit it. The `scan` and `db` commands will validate the bucket is present at the handler level.

- [ ] **Step 1: Change `bucket` field to `Option<String>`**

```rust
/// OSS bucket name
#[arg(short = 'b', long)]
pub bucket: Option<String>,
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation errors in cmd_scan.rs, cmd_serve.rs, cmd_db.rs (they use `&cli.bucket` as `&str`)

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/cli.rs
git commit -m "feat: make --bucket optional at CLI level (Option<String>)"
```

---

### Task 3: Scan command — validate bucket + save to DB

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs:14-61`

**Context:** The scan command needs to validate that `--bucket` is provided, then save the bucket name to `host_config` after a successful scan.

- [ ] **Step 1: Add bucket validation at top of `run_scan`**

Replace the current `BucketName::new(&cli.bucket)` with:

```rust
let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
    S3GalleryError::InvalidConfig(
        "--bucket is required for scan. Use: s3-gallery scan --bucket <name> <host>".to_string(),
    )
})?;
let bucket = BucketName::new(bucket_str)
    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
```

- [ ] **Step 2: Add import for HostConfigEntry**

Add to the imports at the top:

```rust
use s3_gallery_core::db::models::HostConfigEntry;
```

- [ ] **Step 3: Save bucket to DB after scan**

After the `println!("  DB pushed to remote.");` line (line 58), add:

```rust
// Save bucket name to host_config for serve to auto-detect
HostConfigEntry::upsert_bucket(&pool, &host_id.host_id, bucket_str).await?;
```

Note: `pool` was moved into `ScanConfig` at line 35. We need to save the bucket **before** the scan config consumes `pool`, or restructure the code. Looking at the flow:

```rust
let config = ScanConfig {
    s3: s3_client.clone(),
    db: pool,  // pool is moved here
    ...
};
let result = core_run_scan(config).await?;
```

The `pool` is moved into `ScanConfig`. But `core_run_scan` consumes `ScanConfig` and returns `ScanResult`. The `ScanResult` doesn't return the pool. So we need to save the bucket before the pool is moved.

**Fix:** Move the `upsert_bucket` call to before the `ScanConfig` creation, or clone the pool before moving it.

The simplest approach: clone the pool before creating ScanConfig:

```rust
let db_pool = pool.clone();  // clone before move
let config = ScanConfig {
    s3: s3_client.clone(),
    db: pool,  // original moved here
    ...
};
let result = core_run_scan(config).await?;
// ...
HostConfigEntry::upsert_bucket(&db_pool, &host_id.host_id, bucket_str).await?;
```

- [ ] **Step 4: Update `create_s3_client` to handle Option<String>**

```rust
async fn create_s3_client(cli: &Cli) -> Result<Arc<dyn S3Client>> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig(
            "--bucket is required for scan".to_string(),
        )
    })?;
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let config = OssConfig::validate(
        bucket,
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let client = RealS3Client::from_config(&config);
    Ok(Arc::new(client))
}
```

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1 | tail -5`
Expected: No errors

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "feat: save bucket to host_config after scan"
```

---

### Task 4: DB commands — validate bucket is provided

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_db.rs:13-14,88-96`

**Context:** The `db` commands (push, pull, lock, unlock) still require `--bucket`. Validate it at the start of `run_db` and in `create_s3_client`.

- [ ] **Step 1: Add bucket validation at top of `run_db`**

Replace:

```rust
let bucket = BucketName::new(&cli.bucket)
    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
```

With:

```rust
let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
    S3GalleryError::InvalidConfig(
        "--bucket is required for db commands. Use: s3-gallery db --bucket <name> <host> <command>"
            .to_string(),
    )
})?;
let bucket = BucketName::new(bucket_str)
    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
```

- [ ] **Step 2: Update `create_s3_client` in cmd_db.rs** (same pattern as cmd_scan)

```rust
async fn create_s3_client(cli: &Cli) -> Result<Arc<dyn S3Client>> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig(
            "--bucket is required for db commands".to_string(),
        )
    })?;
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let config = OssConfig::validate(
        bucket,
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let client = RealS3Client::from_config(&config);
    Ok(Arc::new(client))
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build 2>&1 | tail -5`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_db.rs
git commit -m "feat: validate --bucket is provided for db commands"
```

---

### Task 5: Serve command — read bucket from DB

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs:22-86`

**Context:** The serve command currently creates the S3 client before opening the DB. The new flow: open DB first, read bucket from `host_config` (fallback to CLI `--bucket`), then create the S3 client.

- [ ] **Step 1: Add import for HostConfigEntry**

Add to imports:

```rust
use s3_gallery_core::db::models::HostConfigEntry;
```

- [ ] **Step 2: Restructure `run_serve` to open DB first**

Replace the current `run_serve` body with:

```rust
pub async fn run_serve(cli: &Cli, host: &str, port: u16, readonly: bool) -> Result<()> {
    let db_path = &cli.db_path;

    // Check DB status
    let status = check_db_status(db_path).await?;
    let action = decide_action(&status, readonly);

    match action {
        DbAction::UseLocal => {
            // Local DB exists, use it
        }
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

    // Resolve bucket: CLI arg > DB > error
    let bucket_name = if let Some(ref b) = cli.bucket {
        BucketName::new(b)
            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?
    } else {
        let config = HostConfigEntry::get(&pool, host)
            .await
            .map_err(|_| {
                S3GalleryError::InvalidConfig(
                    "No bucket specified and no scan data found. \
                     Use --bucket or run 's3-gallery scan <host>' first."
                        .to_string(),
                )
            })?;
        if config.bucket.is_empty() {
            return Err(S3GalleryError::InvalidConfig(
                "No bucket specified and no bucket in scan data. \
                 Use --bucket or run 's3-gallery scan <host>' first."
                    .to_string(),
            ));
        }
        BucketName::new(&config.bucket)
            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket in DB: {}", e)))?
    };

    // Create S3 client with resolved bucket
    let config = OssConfig::validate(
        bucket_name.clone(),
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let real = RealS3Client::from_config(&config);
    let s3 = Arc::new(real) as Arc<dyn S3Client>;

    // Create views
    let local_view = LocalView::new(pool);
    let remote_view = RemoteView::new(local_view.db().clone(), s3, bucket_name.clone());

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
    let host_id = HostIdentifier::new(bucket_name.clone(), host)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid host: {}", e)))?;

    let app_state = AppState {
        templates: Arc::new(env),
        local_view: Arc::new(local_view),
        remote_view: Arc::new(remote_view),
        bucket: bucket_name,
        host_id: host_id.host_id.clone(),
    };

    // Create router and start server
    let app = create_router(app_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("s3-gallery web server starting on {}", addr);
    tracing::info!("  Host: {}", host);
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

- [ ] **Step 3: Build and test**

Run: `cargo build 2>&1 | tail -5`
Expected: No errors

Run: `cargo test --workspace 2>&1 | tail -10`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_serve.rs
git commit -m "feat: serve auto-detects bucket from host_config in DB"
```

---

### Task 6: Final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1`
Expected: No warnings

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