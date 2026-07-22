# Host Auto-Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `scan` auto-discovers hosts by checking for `.s3-gallery/host.config.json` in the scan scope and its first-level subdirectories.

**Architecture:** `--prefix` specifies the scan scope. The scanner checks if the scope root has `.s3-gallery/host.config.json` → it's a single host. If not, it checks each first-level subdirectory → each with config is a host. Directories without config are scanned normally.

**Tech Stack:** Rust, OSS/S3

---

## File Map

| File | Responsibility | Change |
|------|---------------|--------|
| `crates/s3-gallery-cli/src/cli.rs` | CLI definitions | Update Init to use --prefix, update Scan |
| `crates/s3-gallery-cli/src/main.rs` | Entry point | Update match arms |
| `crates/s3-gallery-cli/src/cmd_init.rs` | Init command | Use --prefix for config location |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Scan command | Rewrite with host auto-discovery |
| `crates/s3-gallery-core/src/scan/scanner.rs` | Core scan logic | May need to support multiple scan configs per run |

---

### Task 1: Update init command — use --prefix for config location

**Files:**
- Modify: `crates/s3-gallery-cli/src/cli.rs:63-69`
- Modify: `crates/s3-gallery-cli/src/main.rs:19-25`
- Modify: `crates/s3-gallery-cli/src/cmd_init.rs`

**Context:** The `init` command currently creates `host.config.json` at the bucket root. It needs to accept `--prefix` to specify the directory path where the config should be created.

- [ ] **Step 1: Update CLI Init definition**

```rust
/// Initialize a host directory and create host.config.json
Init {
    /// OSS bucket name
    #[arg(short = 'b', long)]
    bucket: String,
    /// Directory prefix for the host (e.g. "photos"), defaults to bucket root
    #[arg(long)]
    prefix: Option<String>,
    /// Human-readable display name for this host
    #[arg(short = 'n', long)]
    name: String,
    /// Optional description
    #[arg(short = 'd', long)]
    description: Option<String>,
},
```

- [ ] **Step 2: Update main.rs Init match arm**

```rust
Commands::Init {
    bucket,
    prefix,
    name,
    description,
} => {
    if let Err(e) = cmd_init::run_init(&cli, bucket, prefix.as_deref(), name, description.as_deref()).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: Update cmd_init.rs to use prefix**

```rust
use crate::cli::Cli;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::{HostIdentifier, OssConfig};
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::types::{BucketName, ObjectKey};
use std::sync::Arc;

pub async fn run_init(
    cli: &Cli,
    bucket_str: &str,
    prefix: Option<&str>,
    name: &str,
    description: Option<&str>,
) -> Result<()> {
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {e}")))?;

    // Generate UUID as host_id
    let host_id = uuid::Uuid::new_v4().to_string();

    // Build HostIdentifier with the specified prefix
    let host_prefix = ObjectKey::new(prefix.unwrap_or("").to_string())
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

    let host = HostIdentifier::build(
        host_id,
        name,
        "unknown",
        description.unwrap_or(""),
        bucket.clone(),
        host_prefix,
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
    let s3 = Arc::new(RealS3Client::from_config(&config)) as Arc<dyn S3Client>;

    // Upload to OSS
    let config_key = host.config_path();
    let json_bytes = json.into_bytes();
    s3.put_object(&bucket, config_key, &json_bytes).await?;

    println!("Host initialized:");
    println!("  host_id: {}", host.host_id);
    println!("  name: {}", host.host_name);
    println!("  bucket: {}", bucket_str);
    println!("  config: {}/.s3-gallery/host.config.json", prefix.unwrap_or(""));

    Ok(())
}
```

- [ ] **Step 4: Build to verify**

Run: `cargo build 2>&1 | tail -5`
Expected: Build succeeds

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/cli.rs crates/s3-gallery-cli/src/main.rs crates/s3-gallery-cli/src/cmd_init.rs
git commit -m "feat: init accepts --prefix for config location"
```

---

### Task 2: Rewrite scan command with host auto-discovery

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`

**Context:** The scan command needs to support two modes:
1. **Single host mode**: if the scan scope root has `.s3-gallery/host.config.json`, scan everything as one host
2. **Auto-discover mode**: if no root config, check each first-level subdirectory

For each discovered host, we run a separate `ScanConfig` and save results. For directories without config, we scan normally but without host classification.

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
use s3_gallery_core::types::{BucketName, ObjectKey};
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

    // Create local DB directory and pool
    let db_path = &cli.db_path;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;

    // Determine scan scope prefix
    let scope_prefix = prefix.unwrap_or_default();
    let scope_prefix_str = if scope_prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", scope_prefix.trim_end_matches('/'))
    };

    // Try to read host.config.json at the scope root
    let config_key_str = format!("{}.s3-gallery/host.config.json", scope_prefix_str);
    let config_key = ObjectKey::new(config_key_str.clone())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

    let root_config = s3.get_object(&bucket, &config_key).await.ok();

    if let Some(data) = root_config {
        // CASE 1: Root has config → scan entire scope as a single host
        let host: HostIdentifier = serde_json::from_slice(&data)
            .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

        tracing::info!(host_id = %host.host_id, name = %host.host_name, "found host config at scope root");

        let scan_prefix = ObjectKey::new(scope_prefix_str.clone())
            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

        scan_host(
            &s3, &pool, &bucket, &host, &scan_prefix, &opts, &cli,
        ).await?;

        // Save host config
        HostConfigEntry::upsert_host_config(
            &pool, &host.host_id, bucket_str, &cli.endpoint, &cli.region,
        ).await?;

        println!("Scan complete for host: {} ({})", host.host_name, host.host_id);
    } else {
        // CASE 2: No root config → discover hosts in first-level subdirectories
        // List all objects under the scope prefix to find first-level directories
        let list_prefix = ObjectKey::new(scope_prefix_str.clone())
            .map_err(|e| S3GalleryError::Internal(format!("Invalid prefix: {e}")))?;

        let all_objects = s3.list_objects(&bucket, &list_prefix).await?;

        // Extract unique first-level directory names
        let mut subdirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for obj in &all_objects {
            let key = obj.key.as_str();
            if let Some(rest) = key.strip_prefix(&scope_prefix_str) {
                if let Some(slash) = rest.find('/') {
                    let dir = &rest[..slash];
                    if !dir.is_empty() {
                        subdirs.insert(dir.to_string());
                    }
                }
            }
        }

        let mut total_files = 0u64;
        let mut host_count = 0u64;

        for dir in &subdirs {
            let dir_prefix = format!("{}{}/", scope_prefix_str, dir);
            let dir_config_key_str = format!("{}{}/.s3-gallery/host.config.json", scope_prefix_str, dir);
            let dir_config_key = ObjectKey::new(dir_config_key_str)
                .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

            let dir_config = s3.get_object(&bucket, &dir_config_key).await.ok();

            if let Some(data) = dir_config {
                // This subdirectory is a host
                let host: HostIdentifier = serde_json::from_slice(&data)
                    .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                tracing::info!(host_id = %host.host_id, name = %host.host_name, dir = %dir, "found host config in subdirectory");

                let scan_prefix = ObjectKey::new(dir_prefix.clone())
                    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                let result = scan_host(
                    &s3, &pool, &bucket, &host, &scan_prefix, &opts, &cli,
                ).await?;

                total_files += result.total_files;
                host_count += 1;

                // Save host config
                HostConfigEntry::upsert_host_config(
                    &pool, &host.host_id, bucket_str, &cli.endpoint, &cli.region,
                ).await?;

                println!("  Host: {} ({}) — {} files", host.host_name, host.host_id, result.total_files);
            } else {
                // Not a host — scan normally without host classification
                tracing::info!(dir = %dir, "no host config, scanning as regular directory");
                // For now, skip non-host directories
                println!("  Skipping {} (no host config)", dir);
            }
        }

        if host_count == 0 {
            println!("No hosts found in scope '{}'.", scope_prefix_str);
            println!("Use 's3-gallery init --bucket {} --prefix <name> --name <display-name>' to create a host.", bucket_str);
        } else {
            println!("Scan complete: {} hosts, {} total files", host_count, total_files);
        }
    }

    // Auto push DB to remote
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
    let db_data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
    s3.put_object(&bucket, db_key, &db_data).await?;
    println!("  DB pushed to remote.");

    Ok(())
}

/// Scan a single host and return the scan result.
async fn scan_host(
    s3: &Arc<dyn S3Client>,
    pool: &sqlx::SqlitePool,
    bucket: &BucketName,
    host: &HostIdentifier,
    scan_prefix: &ObjectKey,
    opts: &ScanOptions,
    _cli: &Cli,
) -> Result<s3_gallery_core::scan::scanner::ScanResult> {
    let scan_config = ScanConfig {
        s3: s3.clone(),
        db: pool.clone(),
        bucket: bucket.clone(),
        prefix: scan_prefix.clone(),
        concurrency: opts.concurrency,
        extract_metadata: opts.extract_metadata,
        generate_thumbnails: opts.with_thumbnails,
        client_id: format!("cli-{}", host.host_id),
        host_id: host.host_id.clone(),
    };

    core_run_scan(scan_config).await
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

- [ ] **Step 2: Build to verify**

Run: `cargo build 2>&1 | tail -10`
Expected: Build succeeds

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "feat: scan auto-discovers hosts by checking for .s3-gallery config"
```

---

### Task 3: Final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -5`
Expected: No warnings

- [ ] **Step 2: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: All tests pass

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "chore: final verification before merge"
```