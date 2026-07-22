# .ossgallery Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename all crates/types from `ossgalley` to `s3-gallery`, move `.ossgallery` to `.s3-gallery` with DB/lock at bucket root, and add `host_id` to DB schema for single-DB multi-host support.

**Architecture:** Rename crate directories first, then rename types globally, then update paths and schema, then thread `host_id` through all queries.

**Tech Stack:** Rust, cargo, git mv

---

## File structure map

### Rename operations (Task 1)
- `crates/ossgalley-core/` → `crates/s3-gallery-core/`
- `crates/ossgalley-cli/` → `crates/s3-gallery-cli/`
- `crates/ossgalley-web/` → `crates/s3-gallery-web/`

### Files modified by task

| Task | Files |
|------|-------|
| 1: Rename dirs + Cargo.toml | `Cargo.toml` (workspace), `crates/*/Cargo.toml` (3 files), `tests/Cargo.toml` |
| 2: Rename types + crate refs | `error.rs`, `lib.rs` (core + cli + web), `cli.rs`, `main.rs`, all `*.rs` files referencing `OssgalleyError`/`ossgalley_*` |
| 3: HostIdentifier paths | `s3/config.rs`, `s3/lock.rs`, `scan/scanner.rs` |
| 4: Schema + models | `db/schema.rs`, `db/models.rs`, `db/status.rs` |
| 5: Scanner host_id | `scan/scanner.rs` |
| 6: CLI commands | `cli.rs`, `cmd_init.rs`, `cmd_scan.rs`, `cmd_serve.rs`, `cmd_db.rs`, `cmd_view.rs`, `main.rs` |
| 7: View queries | `view/mod.rs`, `view/ls.rs`, `view/stat.rs`, `view/search.rs`, `view/timeline.rs`, `view/tags.rs`, `view/duplicates.rs`, `view/export.rs`, `view/tree.rs` |
| 8: Web handlers | `web/state.rs`, `web/router.rs`, `web/handlers/*.rs` |
| 9: Build + test | (verification only) |

---

### Task 1: Rename crate directories and update Cargo.toml

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/ossgalley-core/Cargo.toml`
- Modify: `crates/ossgalley-cli/Cargo.toml`
- Modify: `crates/ossgalley-web/Cargo.toml`
- Modify: `tests/Cargo.toml` (test crate)

- [ ] **Step 1: Rename directories with git mv**

```bash
cd /Users/macbook/Project/s3-gallery
git mv crates/ossgalley-core crates/s3-gallery-core
git mv crates/ossgalley-cli crates/s3-gallery-cli
git mv crates/ossgalley-web crates/s3-gallery-web
```

- [ ] **Step 2: Update workspace root `Cargo.toml`**

Change members to:
```toml
[workspace]
resolver = "2"
members = [
    "crates/s3-gallery-core",
    "crates/s3-gallery-cli",
    "crates/s3-gallery-web",
]

[package]
name = "s3-gallery-tests"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
tempfile = "3"
tokio = { version = "1", features = ["full"] }
s3-gallery-core = { path = "crates/s3-gallery-core" }
uuid = { version = "1", features = ["v4"] }
aws-sdk-s3 = "1.138.1"
aws-config = "1.5.0"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
async-trait = "0.1"
```

- [ ] **Step 3: Update `crates/s3-gallery-core/Cargo.toml`**

```toml
[package]
name = "s3-gallery-core"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
aws-sdk-s3 = "1.138.1"
aws-config = "1.5.0"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }
async-trait = "0.1"
tracing = "0.1"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1.0"
tempfile = "3"
```

- [ ] **Step 4: Update `crates/s3-gallery-cli/Cargo.toml`**

```toml
[package]
name = "s3-gallery-cli"
version = "0.1.0"
edition = "2021"

[dependencies]
s3-gallery-core = { path = "../s3-gallery-core" }
s3-gallery-web = { path = "../s3-gallery-web" }
axum = "0.7"
minijinja = "2"
tower-http = { version = "0.5", features = ["cors", "trace"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
clap = { version = "4", features = ["derive", "env"] }
tokio = { version = "1", features = ["full"] }
tracing-subscriber = "0.3"
```

- [ ] **Step 5: Update `crates/s3-gallery-web/Cargo.toml`**

```toml
[package]
name = "s3-gallery-web"
version = "0.1.0"
edition = "2021"

[dependencies]
s3-gallery-core = { path = "../s3-gallery-core" }
minijinja = "2"
tracing = "0.1"
```

- [ ] **Step 6: Build to verify Cargo.toml changes**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success (may show errors about missing modules — that's OK, the crate names resolve)

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: rename crate directories and Cargo.toml packages

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Rename types and crate references across all crates

**Files:**
- Modify: `crates/s3-gallery-core/src/error.rs` — rename `OssgalleyError` → `S3GalleryError`
- Modify: `crates/s3-gallery-core/src/lib.rs` — update module exports
- Modify: `crates/s3-gallery-cli/src/main.rs` — update binary name
- Modify: `crates/s3-gallery-cli/src/cli.rs` — update binary name, env vars
- Modify: All `*.rs` files referencing `OssgalleyError`, `ossgalley_core`, `ossgalley_web`, `ossgalley::`

- [ ] **Step 1: Rename error type in `error.rs`**

Replace `OssgalleyError` with `S3GalleryError` in the enum definition. The `pub type Result<T>` alias stays as `Result<T>`.

```rust
#[derive(Debug, Error)]
pub enum S3GalleryError {
    #[error("S3 operation failed: {0}")]
    S3Error(String),
    // ... all variants stay the same, just rename the enum
}
```

- [ ] **Step 2: Bulk rename across all Rust source files**

Run sed commands to rename all type references:

```bash
cd /Users/macbook/Project/s3-gallery

# Rename error type
find crates tests -name '*.rs' -exec sed -i '' 's/OssgalleyError/S3GalleryError/g' {} +

# Rename crate references in use statements
find crates tests -name '*.rs' -exec sed -i '' 's/ossgalley_core/s3_gallery_core/g' {} +
find crates tests -name '*.rs' -exec sed -i '' 's/ossgalley_web/s3_gallery_web/g' {} +

# Rename logging targets
find crates tests -name '*.rs' -exec sed -i '' 's/ossgalley::/s3_gallery::/g' {} +
```

- [ ] **Step 3: Update binary name in `cli.rs`**

```rust
#[command(name = "s3-gallery")]
#[command(about = "S3 media file gallery browser", long_about = None)]
```

- [ ] **Step 4: Update env var names in `cli.rs`**

```rust
#[arg(short = 'e', long, default_value = "https://localhost:9000", env = "S3_GALLERY_ENDPOINT")]
#[arg(short = 'k', long, default_value = "s3oss", env = "S3_GALLERY_ACCESS_KEY")]
#[arg(short = 's', long, default_value = "s3oss1234", env = "S3_GALLERY_SECRET_KEY")]
#[arg(short = 'r', long, default_value = "us-east-1", env = "S3_GALLERY_REGION")]
#[arg(long, global = true, default_value = "s3-gallery.db", env = "S3_GALLERY_DB_PATH")]
```

- [ ] **Step 5: Update `main.rs` tracing subscriber**

Keep the subscriber as-is (it uses the default format), but update any hardcoded references.

- [ ] **Step 6: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success (no errors)

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: rename types and crate references to s3-gallery

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Update HostIdentifier paths (.s3-gallery, bucket root)

**Files:**
- Modify: `crates/s3-gallery-core/src/s3/config.rs`
- Modify: `crates/s3-gallery-core/src/s3/lock.rs`
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`

- [ ] **Step 1: Update `config.rs` — change `.ossgallery` → `.s3-gallery` and db/lock to bucket root**

In `HostIdentifier::build()`, change the path derivation:

```rust
let oss_dir = if prefix_str.is_empty() {
    ".s3-gallery".to_string()
} else {
    format!("{prefix_str}/.s3-gallery")
};

// DB is at bucket root, not under host prefix
let db_path = ObjectKey::new("s3-gallery.db".to_string())
    .map_err(|e| S3GalleryError::Internal(format!("failed to build db_path: {e}")))?;

// Lock is at bucket root
let lock_path = ObjectKey::new("s3-gallery.lock".to_string())
    .map_err(|e| S3GalleryError::Internal(format!("failed to build lock_path: {e}")))?;

// Config is under host prefix
let config_path = ObjectKey::new(format!("{oss_dir}/host.config.json"))
    .map_err(|e| S3GalleryError::Internal(format!("failed to build config_path: {e}")))?;

// .s3-gallery dir is under host prefix
let s3_gallery_dir = ObjectKey::new(format!("{oss_dir}/"))
    .map_err(|e| S3GalleryError::Internal(format!("failed to build ossgallery_dir: {e}")))?;
```

Update the struct field names and doc comments accordingly:
- `ossgallery_dir: ObjectKey` → `s3_gallery_dir: ObjectKey`
- Update doc comment: `/// .s3-gallery directory path: {prefix}/.s3-gallery/`
- Update accessor method name: `pub fn s3_gallery_dir(&self) -> &ObjectKey`

Update all test expectations:
- `"my-camera/.ossgallery/ossgallery.db"` → `"s3-gallery.db"`
- `"my-camera/.ossgallery/db.lock"` → `"s3-gallery.lock"`
- `"my-camera/.ossgallery/host.config.json"` → `"my-camera/.s3-gallery/host.config.json"`
- `"my-camera/.ossgallery/"` → `"my-camera/.s3-gallery/"`
- Nested prefix: `"cameras/backyard/.ossgallery/..."` → `"cameras/backyard/.s3-gallery/..."`

- [ ] **Step 2: Update lock path in `s3/lock.rs`**

The lock file is now at `s3-gallery.lock` (bucket root), not `{prefix}/.ossgallery/db.lock`. Update the lock module to accept the lock key directly (it already does — the caller passes the key), so only the scanner needs updating.

- [ ] **Step 3: Update scanner to filter `.s3-gallery` instead of `.ossgallery`**

In `scan/scanner.rs`, update the filter:

```rust
let s3_objects: Vec<ObjectSummary> = all_objects
    .into_iter()
    .filter(|obj| {
        let key = obj.key.as_str();
        !key.contains("/.s3-gallery/") && !key.starts_with(".s3-gallery/")
    })
    .collect();
```

Also update the lock key path construction in the scanner:

```rust
// Lock key is at bucket root
let lock_key = ObjectKey::new("s3-gallery.lock".to_string())
    .map_err(|e| S3GalleryError::Internal(format!("Failed to create lock key: {}", e)))?;
```

- [ ] **Step 4: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: update HostIdentifier to .s3-gallery paths, db/lock at bucket root

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Update DB schema and models (add host_id)

**Files:**
- Modify: `crates/s3-gallery-core/src/db/schema.rs`
- Modify: `crates/s3-gallery-core/src/db/models.rs`
- Modify: `crates/s3-gallery-core/src/db/status.rs`

- [ ] **Step 1: Update `schema.rs` — add host_id to files and scan_metadata**

Replace the `files` table DDL:

```sql
CREATE TABLE IF NOT EXISTS files (
    host_id TEXT NOT NULL,
    key TEXT NOT NULL,
    etag TEXT NOT NULL,
    size INTEGER NOT NULL,
    last_modified TEXT NOT NULL,
    content_type TEXT,
    file_type TEXT NOT NULL,
    metadata_state TEXT NOT NULL DEFAULT 'pending',
    is_deleted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (host_id, key)
);
```

Replace the `scan_metadata` table DDL:

```sql
CREATE TABLE IF NOT EXISTS scan_metadata (
    host_id TEXT NOT NULL PRIMARY KEY,
    last_scanned_key TEXT,
    last_scanned_at TEXT,
    total_files INTEGER,
    total_size INTEGER,
    db_schema_version INTEGER NOT NULL DEFAULT 1
);
```

Add an index for host_id:

```sql
CREATE INDEX IF NOT EXISTS idx_files_host_id ON files(host_id);
```

Update the `INSERT OR IGNORE INTO scan_metadata` to include `host_id`:

```sql
-- For the default row (legacy), use a placeholder host_id
INSERT OR IGNORE INTO scan_metadata (host_id, db_schema_version) VALUES ('default', 1);
```

Update the schema version comment to indicate v2, and update the test expectations.

- [ ] **Step 2: Update `models.rs` — add host_id to FileEntry and ScanMetadata**

```rust
pub struct FileEntry {
    pub host_id: String,       // NEW
    pub key: String,
    pub etag: String,
    pub size: i64,
    pub last_modified: String,
    pub content_type: Option<String>,
    pub file_type: String,
    pub metadata_state: String,
    pub is_deleted: bool,
}
```

Update `insert` SQL:
```sql
INSERT INTO files (host_id, key, etag, size, last_modified, content_type, file_type, \
                   metadata_state, is_deleted) \
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
```

Update `upsert` SQL similarly. Update `list_by_prefix` to filter by host_id:

```sql
SELECT * FROM files WHERE host_id = ? AND key LIKE ? || '%' AND is_deleted = 0 ORDER BY key
```

```rust
pub async fn list_by_prefix(pool: &SqlitePool, host_id: &str, prefix: &str) -> Result<Vec<FileEntry>> {
    sqlx::query_as::<_, FileEntry>(
        "SELECT * FROM files WHERE host_id = ? AND key LIKE ? || '%' AND is_deleted = 0 ORDER BY key",
    )
    .bind(host_id)
    .bind(prefix)
    .fetch_all(pool)
    .await
    // ...
}
```

Similarly update `list_by_file_type` to include `host_id` filter.

Update `ScanMetadata`:

```rust
pub struct ScanMetadata {
    pub host_id: String,       // NEW — primary key
    pub last_scanned_key: Option<String>,
    pub last_scanned_at: Option<String>,
    pub total_files: Option<i64>,
    pub total_size: Option<i64>,
    pub db_schema_version: i64,
}
```

Update `ScanMetadata::get` to accept `host_id`:

```rust
pub async fn get(pool: &SqlitePool, host_id: &str) -> Result<ScanMetadata> {
    sqlx::query_as::<_, ScanMetadata>(
        "SELECT * FROM scan_metadata WHERE host_id = ? LIMIT 1"
    )
    .bind(host_id)
    // ...
}
```

Update `ScanMetadata::update` — add `host_id` to the WHERE clause:

```sql
UPDATE scan_metadata SET last_scanned_key = ?, last_scanned_at = ?, \
       total_files = ?, total_size = ?, db_schema_version = ? \
WHERE host_id = ?
```

- [ ] **Step 3: Update all test code in `models.rs`**

Add `host_id: "test-host".to_string()` to all `FileEntry` and `ScanMetadata` test constructions.

- [ ] **Step 4: Update `db/status.rs` — simplify**

The `check_db_status` function no longer needs to check remote S3. Simplify to just check if the local DB exists:

```rust
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum DbStatus {
    LocalExists,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DbAction {
    UseLocal,
    FullScan,
    Abort(String),
}

pub async fn check_db_status(cache_path: &Path) -> Result<DbStatus> {
    if cache_path.exists() {
        Ok(DbStatus::LocalExists)
    } else {
        Ok(DbStatus::None)
    }
}

pub fn decide_action(status: &DbStatus, readonly: bool) -> DbAction {
    match status {
        DbStatus::LocalExists => DbAction::UseLocal,
        DbStatus::None if !readonly => DbAction::FullScan,
        DbStatus::None => DbAction::Abort(
            "No database found. Run 's3-gallery scan <host>' first.".to_string()
        ),
    }
}
```

- [ ] **Step 5: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add host_id to files and scan_metadata schema

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Update scanner to pass host_id

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`
- Modify: `crates/s3-gallery-core/src/scan/diff.rs` (if needed)
- Modify: `crates/s3-gallery-core/src/scan/mod.rs`

- [ ] **Step 1: Add `host_id` field to `ScanConfig`**

```rust
pub struct ScanConfig {
    pub s3: Arc<dyn S3Client>,
    pub db: SqlitePool,
    pub bucket: BucketName,
    pub prefix: ObjectKey,
    pub host_id: String,       // NEW
    pub concurrency: usize,
    pub extract_metadata: bool,
    pub generate_thumbnails: bool,
    pub client_id: String,
}
```

- [ ] **Step 2: Update FileEntry creation in scanner to include host_id**

In `run_scan`, when creating FileEntry for new objects:

```rust
FileEntry::upsert(&config.db, &FileEntry {
    host_id: config.host_id.clone(),    // NEW
    key: obj.key.as_str().to_string(),
    etag: obj.etag.as_str().to_string(),
    size: obj.size.as_u64() as i64,
    last_modified: obj.last_modified.clone(),
    content_type,
    file_type: file_type.to_string(),
    metadata_state: "pending".to_string(),
    is_deleted: false,
}).await?;
```

Same for changed objects. Update both FileEntry construction sites.

- [ ] **Step 3: Update scan_metadata write**

```rust
let scan_meta = ScanMetadata {
    host_id: config.host_id.clone(),    // NEW
    last_scanned_key: Some(String::new()),
    last_scanned_at: Some(Utc::now().to_rfc3339()),
    total_files: Some(s3_objects.len() as i64),
    total_size: Some(total_size as i64),
    db_schema_version: 1,
};
ScanMetadata::update(&config.db, &scan_meta).await?;
```

- [ ] **Step 4: Update `list_by_prefix` call in scanner**

```rust
let db_entries = FileEntry::list_by_prefix(&config.db, &config.host_id, config.prefix.as_str()).await?;
```

- [ ] **Step 5: Update scanner tests**

Add `host_id: "test".to_string()` to all ScanConfig constructions in tests.

- [ ] **Step 6: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: pass host_id through scanner to FileEntry and ScanMetadata

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: Update CLI commands

**Files:**
- Modify: `crates/s3-gallery-cli/src/cli.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_init.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_db.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_view.rs`
- Modify: `crates/s3-gallery-cli/src/main.rs`

- [ ] **Step 1: Update `cli.rs` — default db path**

Already done in Task 2 (env var rename). Just verify `default_value = "s3-gallery.db"` is correct.

- [ ] **Step 2: Update `cmd_init.rs` — mark as not yet implemented**

Replace the entire function body:

```rust
pub async fn run_init(cli: &Cli, host: &str) -> Result<()> {
    tracing::warn!("init: not yet implemented — will create remote .s3-gallery/host.config.json");
    println!("init for host '{}' is not yet implemented", host);
    Ok(())
}
```

Remove unused imports (`BucketName`, `HostIdentifier`, `create_pool`, `run_migrations`, `HostConfigEntry`, `Utc`, `PathBuf`).

- [ ] **Step 3: Update `cmd_scan.rs` — pass host_id, remove local .ossgallery**

Remove the `create_dir_all` for the local `.ossgallery/` directory. Update the scan call:

```rust
let config = ScanConfig {
    s3: s3_client.clone(),
    db: pool,
    bucket: bucket.clone(),
    prefix: host_id.prefix.clone(),
    host_id: host_id.host_id.clone(),    // NEW
    concurrency: opts.concurrency,
    extract_metadata: opts.extract_metadata,
    generate_thumbnails: opts.with_thumbnails,
    client_id: format!("cli-{}", host),
};
```

If `cli.db_path` doesn't exist, create the parent directory (if any) and the pool:

```rust
let db_path = &cli.db_path;
if let Some(parent) = db_path.parent() {
    std::fs::create_dir_all(parent)?;
}
let pool = create_pool(&db_path).await?;
```

- [ ] **Step 4: Update `cmd_serve.rs` — remove remote DB download, add host_id to AppState**

Simplify the DB check — just open local DB, attempt `db pull` if not exists:

```rust
pub async fn run_serve(cli: &Cli, host: &str, port: u16, readonly: bool) -> Result<()> {
    let bucket = BucketName::new(&cli.bucket)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid host: {}", e)))?;

    let db_path = &cli.db_path;

    // If DB doesn't exist, try pulling from remote
    if !db_path.exists() {
        // Attempt db pull — this will download s3-gallery.db from bucket root
        let config = OssConfig::validate(
            bucket.clone(),
            &cli.endpoint,
            &cli.region,
            &cli.access_key,
            &cli.secret_key,
            10,
        )?;
        let real = RealS3Client::from_config(&config);
        let s3 = Arc::new(real) as Arc<dyn S3Client>;

        let db_key = ObjectKey::new("s3-gallery.db")
            .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        match s3.get_object(&bucket, &db_key).await {
            Ok(data) => {
                if let Some(parent) = db_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&db_path, &data)?;
                tracing::info!("DB downloaded from remote.");
            }
            Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
                return Err(S3GalleryError::NotFound(
                    "No database found. Run 's3-gallery scan <host>' first.".to_string()
                ));
            }
            Err(e) => return Err(e),
        }
    }

    // Create pool, run migrations
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    // Create S3 client
    let config = OssConfig::validate(
        bucket.clone(),
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
    let remote_view = RemoteView::new(local_view.db().clone(), s3, bucket.clone());

    // Register templates
    let mut env = minijinja::Environment::new();
    let errors = s3_gallery_web::register_templates(&mut env);
    if !errors.is_empty() {
        tracing::warn!("{} template(s) failed to register: {:?}", errors.len(), errors);
    }

    // Build AppState with host_id
    let app_state = AppState {
        templates: Arc::new(env),
        local_view: Arc::new(local_view),
        remote_view: Arc::new(remote_view),
        bucket,
        host_id: host_id.host_id.clone(),    // NEW
    };

    // ... rest unchanged
}
```

- [ ] **Step 5: Update `cmd_db.rs` — use bucket root paths**

Update `db pull` to download from `s3-gallery.db`:
```rust
DbCommands::Pull => {
    let db_key = ObjectKey::new("s3-gallery.db")
        .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
    let data = s3.get_object(&bucket, &db_key).await?;
    // ...
}
```

Update `db push` to upload to `s3-gallery.db`:
```rust
DbCommands::Push => {
    let db_key = ObjectKey::new("s3-gallery.db")
        .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
    let data = std::fs::read(&db_path)?;
    s3.put_object(&bucket, &db_key, &data).await?;
    // ...
}
```

Update `db status` to use `s3-gallery.db` key:
```rust
DbCommands::Status => {
    let db_key = ObjectKey::new("s3-gallery.db")
        .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
    let remote_meta = s3.head_object(&bucket, &db_key).await;
    // ...
}
```

Update `db lock/unlock` to use `s3-gallery.lock`:
```rust
DbCommands::Lock => {
    let lock_key = ObjectKey::new("s3-gallery.lock")
        .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
    // ...
}
```

- [ ] **Step 6: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: update CLI commands for new DB and path design

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: Update view queries to filter by host_id

**Files:**
- Modify: `crates/s3-gallery-core/src/view/mod.rs`
- Modify: `crates/s3-gallery-core/src/view/ls.rs`
- Modify: `crates/s3-gallery-core/src/view/stat.rs`
- Modify: `crates/s3-gallery-core/src/view/search.rs`
- Modify: `crates/s3-gallery-core/src/view/timeline.rs`
- Modify: `crates/s3-gallery-core/src/view/tags.rs`
- Modify: `crates/s3-gallery-core/src/view/duplicates.rs`
- Modify: `crates/s3-gallery-core/src/view/export.rs`
- Modify: `crates/s3-gallery-core/src/view/tree.rs`

- [ ] **Step 1: Add `host_id` parameter to `LocalView` methods**

In `view/mod.rs`, add `host_id: &str` to each method:

```rust
impl LocalView {
    pub async fn list_directory(
        &self,
        host_id: &str,
        prefix: &str,
        sort_by: crate::types::SortField,
        sort_order: crate::types::SortOrder,
    ) -> crate::error::Result<Vec<ls::LsEntry>> {
        ls::list_directory(&self.db, host_id, prefix, sort_by, sort_order).await
    }

    pub async fn get_stats(&self, host_id: &str) -> crate::error::Result<stat::FileStats> {
        stat::get_stats(&self.db, host_id).await
    }

    pub async fn search_by_name(&self, host_id: &str, query: &str) -> crate::error::Result<search::SearchResult> {
        search::search_by_name(&self.db, host_id, query).await
    }

    pub async fn search_by_tag(&self, host_id: &str, tag_name: &str) -> crate::error::Result<search::SearchResult> {
        search::search_by_tag(&self.db, host_id, tag_name).await
    }

    pub async fn get_timeline(&self, host_id: &str) -> crate::error::Result<Vec<timeline::TimelineEntry>> {
        timeline::get_timeline(&self.db, host_id).await
    }

    pub async fn list_tags(&self, host_id: &str) -> crate::error::Result<Vec<crate::db::models::TagEntry>> {
        tags::list_tags(&self.db, host_id).await
    }

    pub async fn get_files_by_tag(
        &self,
        host_id: &str,
        tag_name: &str,
    ) -> crate::error::Result<Vec<crate::db::models::FileEntry>> {
        tags::get_files_by_tag(&self.db, host_id, tag_name).await
    }

    pub async fn find_duplicates(&self, host_id: &str) -> crate::error::Result<Vec<duplicates::DuplicateGroup>> {
        duplicates::find_duplicates(&self.db, host_id).await
    }

    pub async fn export_files(
        &self,
        host_id: &str,
        format: export::ExportFormat,
    ) -> crate::error::Result<String> {
        export::export_files(&self.db, host_id, format).await
    }

    pub async fn build_tree(&self, host_id: &str, root_prefix: &str) -> crate::error::Result<tree::TreeNode> {
        tree::build_tree(&self.db, host_id, root_prefix).await
    }
}
```

- [ ] **Step 2: Update each view module to accept `host_id` and filter queries**

For example, `view/ls.rs`:

```rust
pub async fn list_directory(
    db: &SqlitePool,
    host_id: &str,
    prefix: &str,
    sort_by: SortField,
    sort_order: SortOrder,
) -> Result<Vec<LsEntry>> {
    // Add WHERE host_id = ? to all SQL queries
    sqlx::query_as::<_, LsEntry>(
        "SELECT key, size, last_modified, file_type, content_type \
         FROM files WHERE host_id = ? AND key LIKE ? || '%' AND is_deleted = 0 \
         ORDER BY ..."
    )
    .bind(host_id)
    .bind(prefix)
    // ...
}
```

Similarly for all other view modules — add `host_id: &str` parameter and `WHERE host_id = ?` to all SQL queries.

- [ ] **Step 3: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add host_id filter to all view queries

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 8: Update web handlers for host_id passthrough

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/state.rs`
- Modify: `crates/s3-gallery-cli/src/web/router.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/browse.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/dashboard.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/download.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/duplicates.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/file_detail.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/gallery.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/search.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/settings.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/stats.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/tags.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/timeline.rs`

- [ ] **Step 1: Add `host_id` to `AppState`**

```rust
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub local_view: Arc<LocalView>,
    pub remote_view: Arc<RemoteView>,
    pub bucket: BucketName,
    pub host_id: String,    // NEW
}
```

- [ ] **Step 2: Update each handler to pass `host_id` to view methods**

For example, `handlers/browse.rs`:

```rust
async fn browse(
    State(state): State<AppState>,
    Query(params): Query<BrowseParams>,
) -> impl IntoResponse {
    let entries = state.local_view.list_directory(
        &state.host_id,
        &params.path.unwrap_or_default(),
        // ...
    ).await;
    // ...
}
```

Similarly for all other handlers that call `local_view` methods — pass `&state.host_id` as the first argument.

- [ ] **Step 3: Build to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: thread host_id through web handlers

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 9: Full build, test, and clippy

**Files:** None (verification only)

- [ ] **Step 1: Full workspace build**

Run: `cd /Users/macbook/Project/s3-gallery && cargo build 2>&1`
Expected: all crates build successfully

- [ ] **Step 2: Run all tests**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 3: Check clippy**

Run: `cd /Users/macbook/Project/s3-gallery && cargo clippy --all-targets 2>&1`
Expected: no warnings (or only pre-existing ones unrelated to our changes)

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: final build, test, and clippy cleanup

Co-Authored-By: Claude <noreply@anthropic.com>"
```