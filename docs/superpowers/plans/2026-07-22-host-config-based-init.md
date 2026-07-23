# Host Config Based Init Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove `host` CLI argument from `scan` command. Instead, `host_id` and `host_name` are read from `host.config.json` stored in the OSS bucket. A new `init` command creates this config file.

**Architecture:** `init` generates a random UUID as `host_id` and uploads `host.config.json` to the OSS bucket's `.s3-gallery/` directory. `scan` downloads this config file to get the host identity. The old `host` CLI argument becomes an optional `--prefix` for directory scope.

**Tech Stack:** Rust, serde, uuid, OSS/S3

---

## File Map

| File | Responsibility | Change |
|------|---------------|--------|
| `crates/s3-gallery-core/src/s3/config.rs` | HostIdentifier struct | Add Serialize/Deserialize derives |
| `crates/s3-gallery-core/src/types.rs` | BucketName, ObjectKey | Add Serialize/Deserialize derives |
| `crates/s3-gallery-cli/src/cli.rs` | CLI argument definitions | Update Init, Scan commands |
| `crates/s3-gallery-cli/src/main.rs` | Entry point | Update match arms |
| `crates/s3-gallery-cli/src/cmd_init.rs` | Init command | Rewrite to create and upload host.config.json |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Scan command | Remove host arg, read config from OSS |

---

### Task 1: Add serde derives to HostIdentifier, BucketName, ObjectKey

**Files:**
- Modify: `crates/s3-gallery-core/src/s3/config.rs:102`
- Modify: `crates/s3-gallery-core/src/types.rs:17,93`

**Context:** The `HostIdentifier` struct needs to be serializable to JSON so it can be stored as `host.config.json`. The `BucketName` and `ObjectKey` types also need serialization since they are fields of `HostIdentifier`.

- [ ] **Step 1: Add serde derives to BucketName and ObjectKey**

In `crates/s3-gallery-core/src/types.rs`, change the BucketName and ObjectKey structs to derive Serialize and Deserialize:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BucketName(String);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ObjectKey(String);
```

- [ ] **Step 2: Add serde derives to HostIdentifier**

In `crates/s3-gallery-core/src/s3/config.rs`, change:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HostIdentifier {
```

- [ ] **Step 3: Add uuid dependency to s3-gallery-core**

Add to `crates/s3-gallery-core/Cargo.toml`:

```toml
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 4: Build to verify**

Run: `cargo build -p s3-gallery-core 2>&1 | tail -5`
Expected: Build succeeds

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/s3/config.rs crates/s3-gallery-core/src/types.rs crates/s3-gallery-core/Cargo.toml
git commit -m "feat: add serde derives to HostIdentifier, BucketName, ObjectKey"
```

---

### Task 2: CLI changes — update Init and Scan commands

**Files:**
- Modify: `crates/s3-gallery-cli/src/cli.rs:63-90`
- Modify: `crates/s3-gallery-cli/src/main.rs:18-46`

**Context:** The `Init` command needs `--bucket`, `--name`, and optional `--description` instead of `host`. The `Scan` command needs to remove the `host` positional argument and add `--prefix`.

- [ ] **Step 1: Update Init in CLI**

```rust
/// Initialize a host directory and create host.config.json
Init {
    /// OSS bucket name
    #[arg(short = 'b', long)]
    bucket: String,
    /// Human-readable display name for this host
    #[arg(short = 'n', long)]
    name: String,
    /// Optional description
    #[arg(short = 'd', long)]
    description: Option<String>,
},
```

- [ ] **Step 2: Update Scan in CLI**

```rust
/// Scan/update the database
Scan {
    /// File prefix to scan (e.g. "photos/"), defaults to bucket root
    #[arg(long)]
    prefix: Option<String>,
    /// Incremental scan (resume from last position)
    #[arg(long)]
    incremental: bool,
    /// Schedule mode (skip if locked)
    #[arg(long)]
    schedule: bool,
    /// Force scan (ignore lock)
    #[arg(long)]
    force: bool,
    /// Skip metadata extraction
    #[arg(long)]
    no_metadata: bool,
    /// Generate thumbnails (expensive)
    #[arg(long)]
    with_thumbnails: bool,
    /// Concurrency for S3 requests
    #[arg(long, default_value = "10")]
    concurrency: usize,
},
```

- [ ] **Step 3: Update main.rs Init match arm**

```rust
Commands::Init {
    bucket,
    name,
    description,
} => {
    if let Err(e) = cmd_init::run_init(&cli, bucket, name, description.as_deref()).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Update main.rs Scan match arm**

```rust
Commands::Scan {
    prefix,
    incremental,
    schedule,
    force,
    no_metadata,
    with_thumbnails,
    concurrency,
} => {
    let opts = cmd_scan::ScanOptions {
        incremental: *incremental,
        schedule: *schedule,
        force: *force,
        extract_metadata: !no_metadata,
        with_thumbnails: *with_thumbnails,
        concurrency: *concurrency,
    };
    if let Err(e) = cmd_scan::run_scan(&cli, prefix.clone(), opts).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
```

- [ ] **Step 5: Build to verify**

Run: `cargo build 2>&1 | head -10`
Expected: Compilation errors in cmd_init.rs and cmd_scan.rs (signatures changed)

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-cli/src/cli.rs crates/s3-gallery-cli/src/main.rs
git commit -m "feat: update Init and Scan CLI definitions"
```

---

### Task 3: Rewrite init command

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_init.rs`

**Context:** The `init` command needs to generate a UUID, build a `HostIdentifier`, serialize it to JSON, and upload it to the OSS bucket.

- [ ] **Step 1: Rewrite cmd_init.rs**

```rust
use crate::cli::Cli;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::config::{HostIdentifier, OssConfig};
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::types::BucketName;
use std::sync::Arc;

pub async fn run_init(
    cli: &Cli,
    bucket_str: &str,
    name: &str,
    description: Option<&str>,
) -> Result<()> {
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {e}")))?;

    // Generate UUID as host_id
    let host_id = uuid::Uuid::new_v4().to_string();

    // Build HostIdentifier with empty prefix (scan entire bucket)
    let host = HostIdentifier::build(
        host_id,
        name,
        "unknown",
        description.unwrap_or(""),
        bucket.clone(),
        // prefix is empty string = scan entire bucket root
        s3_gallery_core::types::ObjectKey::new("".to_string())
            .map_err(|e| S3GalleryError::Internal(format!("failed to build prefix: {e}")))?,
        chrono::Utc::now().to_rfc3339(),
        1,
    )?;

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&host)
        .map_err(|e| S3GalleryError::Internal(format!("Failed to serialize config: {e}")))?;

    // Create S3 client
    let config = OssConfig::validate(
        bucket.clone(),
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let s3 = Arc::new(RealS3Client::from_config(&config)) as Arc<dyn s3_gallery_core::s3::client::S3Client>;

    // Upload to OSS
    let config_key = host.config_path();
    let json_bytes = json.into_bytes();
    s3.put_object(&bucket, config_key, &json_bytes).await?;

    println!("Host initialized:");
    println!("  host_id: {}", host.host_id);
    println!("  name: {}", host.host_name);
    println!("  bucket: {}", bucket_str);
    println!("  config: {}/.s3-gallery/host.config.json", bucket_str);

    Ok(())
}
```

Note: The `HostIdentifier::build` method is currently private (`fn build`). We need to make it `pub` for this to work. Let me check if it's already public...

Looking at the code, `build` is:
```rust
#[allow(clippy::missing_panics_doc, clippy::too_many_arguments)]
fn build(
```

It's not `pub`. We need to make it `pub` or add a new constructor. The simplest approach is to make `build` `pub`.

- [ ] **Step 2: Make `HostIdentifier::build` public**

In `crates/s3-gallery-core/src/s3/config.rs`, change:
```rust
fn build(
```
to:
```rust
pub fn build(
```

- [ ] **Step 3: Build to verify**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds (or errors in cmd_scan.rs)

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-core/src/s3/config.rs crates/s3-gallery-cli/src/cmd_init.rs
git commit -m "feat: rewrite init command to create and upload host.config.json"
```

---

### Task 4: Update scan command — read host config from OSS

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`

**Context:** The `scan` command no longer takes a `host` argument. Instead, it downloads `host.config.json` from the OSS bucket to get the `host_id` and `host_name`. The `--prefix` option replaces the old `host` argument for directory scope.

- [ ] **Step 1: Rewrite cmd_scan.rs**

```rust
use crate::cli::Cli;
use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::HostIdentifier;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::scan::scanner::run_scan as core_run_scan;
use s3_gallery_core::scan::scanner::ScanConfig;
use s3_gallery_core::types::BucketName;
use std::sync::Arc;

pub async fn run_scan(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig(
            "--bucket is required for scan. Use: s3-gallery scan --bucket <name>".to_string(),
        )
    })?;
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;

    // Create S3 client
    let config = OssConfig::validate(
        bucket.clone(),
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let s3 = Arc::new(RealS3Client::from_config(&config)) as Arc<dyn S3Client>;

    // Download host.config.json from OSS to get host identity
    let config_key = ObjectKey::new(".s3-gallery/host.config.json".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

    let config_data = s3.get_object(&bucket, &config_key).await
        .map_err(|_| S3GalleryError::NotFound(
            "No host.config.json found. Run 's3-gallery init --bucket <name> --name <display-name>' first.".to_string(),
        ))?;

    let host: HostIdentifier = serde_json::from_slice(&config_data)
        .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

    // Create local DB directory and pool
    let db_path = &cli.db_path;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;

    // Clone pool before moving into ScanConfig
    let db_pool = pool.clone();

    // Resolve scan prefix: CLI --prefix > config prefix > bucket root
    let scan_prefix = match &prefix {
        Some(p) => s3_gallery_core::types::ObjectKey::new(p.clone())
            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?,
        None => host.prefix.clone(),
    };

    // Run scan
    let scan_config = ScanConfig {
        s3: s3.clone(),
        db: pool,
        bucket: bucket.clone(),
        prefix: scan_prefix,
        concurrency: opts.concurrency,
        extract_metadata: opts.extract_metadata,
        generate_thumbnails: opts.with_thumbnails,
        client_id: format!("cli-{}", host.host_id),
        host_id: host.host_id.clone(),
    };

    let result = core_run_scan(scan_config).await?;

    // Auto push DB to remote after scan
    let db_key = host.db_path();
    let db_data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
    s3.put_object(&bucket, db_key, &db_data).await?;

    println!("Scan complete:");
    println!("  Host: {} ({})", host.host_name, host.host_id);
    println!("  Total files: {}", result.total_files);
    println!("  New files: {}", result.new_files);
    println!("  Changed files: {}", result.changed_files);
    println!("  Deleted files: {}", result.deleted_files);
    println!("  Duration: {:.1}s", result.duration_secs);
    println!("  DB pushed to remote.");

    // Save host config for serve to auto-detect
    HostConfigEntry::upsert_host_config(
        &db_pool,
        &host.host_id,
        bucket_str,
        &cli.endpoint,
        &cli.region,
    ).await?;

    Ok(())
}

#[derive(Debug, clap::Parser)]
pub struct ScanOptions {
    pub incremental: bool,
    pub schedule: bool,
    pub force: bool,
    pub extract_metadata: bool,
    #[clap(long = "with-thumbnails")]
    pub with_thumbnails: bool,
    pub concurrency: usize,
}
```

Note: Need to import `ObjectKey` from `s3_gallery_core::types`. Add to the imports:
```rust
use s3_gallery_core::types::{BucketName, ObjectKey};
```

- [ ] **Step 2: Build to verify**

Run: `cargo build 2>&1 | tail -10`
Expected: Build succeeds

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "feat: scan reads host config from OSS instead of CLI host arg"
```

---

### Task 5: Final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -5`
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