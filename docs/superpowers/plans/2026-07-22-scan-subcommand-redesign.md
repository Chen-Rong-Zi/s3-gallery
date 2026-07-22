# Scan Subcommand Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the ambiguous `scan` command into three semantic subcommands: `init` (fresh scan), `update` (incremental), `sync` (remote pull + update).

**Architecture:** CLI layer only — the core scanner (`run_scan`, `ScanConfig`, `ScanResult`) is unchanged. Refactor `cmd_scan.rs` into three entry points, each with clear precondition checks. The `Db` command's `pull`/`push` logic is reused by `sync` internally.

**Tech Stack:** Rust, clap (nested subcommands), s3-gallery-core

**Estimated total time:** 60 min

---

### Task 1: Update CLI struct with nested ScanCommand

**Files:**
- Modify: `crates/s3-gallery-cli/src/cli.rs:61-129`

**Objective:** Replace flat `Commands::Scan` struct with `Commands::Scan { command: ScanCommand }` where `ScanCommand` is an enum of `Init`, `Update`, `Sync`.

- [ ] **Step 1: Replace `Commands::Scan` with nested subcommand enum**

In `crates/s3-gallery-cli/src/cli.rs`, replace the `Scan` variant from a flat struct to a nested subcommand:

```rust
    /// Scan/update the database
    #[command(subcommand)]
    Scan {
        #[command(subcommand)]
        command: ScanCommand,
    },
```

Then add a new `ScanCommand` enum after `Commands`:

```rust
#[derive(Subcommand)]
pub enum ScanCommand {
    /// Fresh scan: create a new local database from scratch
    Init {
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
        /// Confirm overwrite of existing local database
        #[arg(long)]
        confirm: bool,
    },
    /// Incremental update: scan new/changed files into existing local database
    Update {
        /// File prefix to scan (e.g. "photos/")
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
    /// Sync: pull remote database first, then update locally
    Sync {
        /// File prefix to scan (e.g. "photos/")
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
}
```

- [ ] **Step 2: Build and verify**

```bash
cargo build -p s3-gallery-cli 2>&1
```

Expected: Compilation succeeds. `--help` shows the new subcommand structure.

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/cli.rs
git commit -m "feat: add ScanCommand enum with init/update/sync subcommands"
```

---

### Task 2: Refactor cmd_scan.rs into three entry points

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`

**Objective:** Keep the existing `run_scan` function body as the core scanning logic. Add three public entry points `run_init`, `run_update`, `run_sync` with appropriate precondition checks. Extract common setup into helper functions.

- [ ] **Step 1: Extract common helpers**

Add at the top of `cmd_scan.rs` (after imports):

```rust
/// Parse bucket from CLI, create S3 client, and ensure DB directory exists.
async fn setup_scan_common(cli: &Cli) -> Result<(BucketName, Arc<dyn S3Client>, SqlitePool)> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig(
            "--bucket is required for scan. Use: s3-gallery scan <init|update|sync> --bucket <name>".to_string(),
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

    // Ensure DB directory exists
    let db_path = &cli.db_path;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok((bucket, s3, bucket_str.to_string()))
}

/// Create/connect to local DB, run migrations, return pool.
async fn setup_db_pool(cli: &Cli) -> Result<SqlitePool> {
    let pool = create_pool(&cli.db_path).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}
```

- [ ] **Step 2: Add `run_init` function**

Insert before `run_scan`:

```rust
pub async fn run_init(cli: &Cli, prefix: Option<String>, opts: ScanOptions, confirm: bool) -> Result<()> {
    // Check if local DB already exists
    if cli.db_path.exists() && !confirm {
        return Err(S3GalleryError::Internal(
            format!("Local database exists at '{}'. Use --confirm to overwrite.", cli.db_path.display())
        ));
    }

    let (bucket, s3, bucket_str) = setup_scan_common(cli).await?;
    let pool = setup_db_pool(cli).await?;

    // If --confirm, drop all tables and re-create schema
    if cli.db_path.exists() && confirm {
        tracing::info!("--confirm set, removing existing database and re-scanning from scratch");
        // Remove existing DB file so we start fresh
        std::fs::remove_file(&cli.db_path)?;
        // Re-create pool with fresh DB
        let new_pool = setup_db_pool(cli).await?;
        run_scan_core(&s3, &new_pool, &bucket, &bucket_str, prefix, &opts, cli).await
    } else {
        run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await
    }
}
```

- [ ] **Step 3: Add `run_update` function**

```rust
pub async fn run_update(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    // Check that local DB exists
    if !cli.db_path.exists() {
        return Err(S3GalleryError::NotFound(
            format!("No local database found at '{}'. Run 's3-gallery scan init' first.", cli.db_path.display())
        ));
    }

    let (bucket, s3, bucket_str) = setup_scan_common(cli).await?;
    let pool = setup_db_pool(cli).await?;

    run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await
}
```

- [ ] **Step 4: Add `run_sync` function**

```rust
pub async fn run_sync(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    let (bucket, s3, bucket_str) = setup_scan_common(cli).await?;

    // Pull remote DB from OSS
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;

    match s3.get_object(&bucket, &db_key).await {
        Ok(data) => {
            tracing::info!("pulled remote DB ({} bytes)", data.len());
            if let Some(parent) = cli.db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&cli.db_path, &data).map_err(S3GalleryError::IoError)?;
        }
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::info!("no remote DB found, falling back to fresh scan");
            // Fall back to init behavior
            let pool = setup_db_pool(cli).await?;
            return run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await;
        }
        Err(e) => return Err(e),
    }

    let pool = setup_db_pool(cli).await?;
    run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await
}
```

- [ ] **Step 5: Extract the core scan logic into `run_scan_core`**

Rename the existing `run_scan` body to `run_scan_core` (keeping the same scan logic). The old `run_scan` function signature becomes the shared helper:

```rust
/// Core scan logic: list OSS objects, discover hosts, update DB, push DB to remote.
async fn run_scan_core(
    s3: &Arc<dyn S3Client>,
    pool: &SqlitePool,
    bucket: &BucketName,
    bucket_str: &str,
    prefix: Option<String>,
    opts: &ScanOptions,
    cli: &Cli,
) -> Result<()> {
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

    let root_config = s3.get_object(bucket, &config_key).await.ok();

    if let Some(data) = root_config {
        // CASE 1: Root has config → scan entire scope as a single host
        let host: HostIdentifier = serde_json::from_slice(&data)
            .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

        tracing::info!(host_id = %host.host_id, name = %host.host_name, "found host config at scope root");

        let scan_prefix = ObjectKey::new(scope_prefix_str.clone())
            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

        let result = scan_host(
            s3, pool, bucket, &host, &scan_prefix, opts,
        )
        .await?;

        // Save host config
        HostConfigEntry::upsert_host_config(
            pool, &host.host_id, bucket_str, &cli.endpoint, &cli.region,
        )
        .await?;

        println!("Scan complete:");
        println!("  Host: {} ({})", host.host_name, host.host_id);
        println!("  Total files: {}", result.total_files);
        println!("  New files: {}", result.new_files);
        println!("  Changed files: {}", result.changed_files);
        println!("  Deleted files: {}", result.deleted_files);
        println!("  Duration: {:.1}s", result.duration_secs);
    } else {
        // CASE 2: No root config → discover hosts in first-level subdirectories
        let list_prefix = ObjectKey::new(scope_prefix_str.clone())
            .map_err(|e| S3GalleryError::Internal(format!("Invalid prefix: {e}")))?;

        let all_objects = s3.list_objects(bucket, &list_prefix).await?;

        // Extract unique first-level directory names
        let mut subdirs: BTreeSet<String> = BTreeSet::new();
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

        let mut total_files: u64 = 0;
        let mut total_new: u64 = 0;
        let mut total_changed: u64 = 0;
        let mut total_deleted: u64 = 0;
        let mut host_count: u64 = 0;
        let mut duration_secs: f64 = 0.0;

        for dir in &subdirs {
            let dir_prefix_str = format!("{}{}/", scope_prefix_str, dir);
            let dir_config_key_str = format!("{}{}/.s3-gallery/host.config.json", scope_prefix_str, dir);
            let dir_config_key = ObjectKey::new(dir_config_key_str)
                .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

            let dir_config = s3.get_object(bucket, &dir_config_key).await.ok();

            if let Some(data) = dir_config {
                // This subdirectory is a host
                let host: HostIdentifier = serde_json::from_slice(&data)
                    .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                tracing::info!(host_id = %host.host_id, name = %host.host_name, dir = %dir, "found host config in subdirectory");

                let scan_prefix = ObjectKey::new(dir_prefix_str.clone())
                    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                let result = scan_host(
                    s3, pool, bucket, &host, &scan_prefix, opts,
                )
                .await?;

                total_files += result.total_files;
                total_new += result.new_files;
                total_changed += result.changed_files;
                total_deleted += result.deleted_files;
                duration_secs += result.duration_secs;
                host_count += 1;

                // Save host config
                HostConfigEntry::upsert_host_config(
                    pool, &host.host_id, bucket_str, &cli.endpoint, &cli.region,
                )
                .await?;

                println!("  Host: {} ({}) — {} files", host.host_name, host.host_id, result.total_files);
            } else {
                // No host config → scan as regular directory, use dir name as host_id
                tracing::info!(dir = %dir, "no host config, scanning as regular directory");

                // Build a temporary host with dir name as host_id
                let temp_host = HostIdentifier::build(
                    dir.clone(),
                    dir,
                    "auto",
                    "",
                    bucket.clone(),
                    ObjectKey::new(dir_prefix_str.clone())
                        .map_err(|e| S3GalleryError::Internal(format!("invalid prefix: {e}")))?,
                    chrono::Utc::now().to_rfc3339(),
                    1,
                )?;

                let scan_prefix = ObjectKey::new(dir_prefix_str.clone())
                    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                let result = scan_host(
                    s3, pool, bucket, &temp_host, &scan_prefix, opts,
                )
                .await?;

                total_files += result.total_files;
                total_new += result.new_files;
                total_changed += result.changed_files;
                total_deleted += result.deleted_files;
                duration_secs += result.duration_secs;

                // Save minimal host config for serve to discover
                HostConfigEntry::upsert_host_config(
                    pool, dir, bucket_str, &cli.endpoint, &cli.region,
                )
                .await?;

                println!("  Directory: {} — {} files (no host config)", dir, result.total_files);
            }
        }

        println!("Scan complete:");
        if host_count > 0 {
            println!("  Hosts: {}", host_count);
        }
        println!("  Total files: {}", total_files);
        println!("  New files: {}", total_new);
        println!("  Changed files: {}", total_changed);
        println!("  Deleted files: {}", total_deleted);
        println!("  Duration: {:.1}s", duration_secs);
        if host_count == 0 {
            println!("# init: s3-gallery init --bucket {} --prefix <name> --name <display-name>", bucket_str);
        }
    }

    // Auto push DB to remote
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
    let db_data = std::fs::read(&cli.db_path).map_err(S3GalleryError::IoError)?;
    s3.put_object(bucket, &db_key, &db_data).await?;
    println!("  DB pushed to remote.");

    Ok(())
}
```

- [ ] **Step 6: Build and fix any compilation errors**

```bash
cargo build -p s3-gallery-cli 2>&1
```

Expected: Compilation succeeds. The `run_scan` function is removed and replaced by `run_init`, `run_update`, `run_sync`.

- [ ] **Step 7: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "feat: refactor scan into init/update/sync with precondition checks"
```

---

### Task 3: Update main.rs dispatch

**Files:**
- Modify: `crates/s3-gallery-cli/src/main.rs:30-51`

**Objective:** Route `Commands::Scan` to the appropriate `run_*` function based on the nested subcommand.

- [ ] **Step 1: Replace the Scan match arm**

In `crates/s3-gallery-cli/src/main.rs`, replace:

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

With:

```rust
        Commands::Scan { command } => {
            match command {
                cli::ScanCommand::Init {
                    prefix,
                    incremental,
                    schedule,
                    force,
                    no_metadata,
                    with_thumbnails,
                    concurrency,
                    confirm,
                } => {
                    let opts = cmd_scan::ScanOptions {
                        incremental: *incremental,
                        schedule: *schedule,
                        force: *force,
                        extract_metadata: !no_metadata,
                        with_thumbnails: *with_thumbnails,
                        concurrency: *concurrency,
                    };
                    if let Err(e) = cmd_scan::run_init(&cli, prefix.clone(), opts, *confirm).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                cli::ScanCommand::Update {
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
                    if let Err(e) = cmd_scan::run_update(&cli, prefix.clone(), opts).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                cli::ScanCommand::Sync {
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
                    if let Err(e) = cmd_scan::run_sync(&cli, prefix.clone(), opts).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
```

- [ ] **Step 2: Add `use crate::cli::ScanCommand` if needed**

Check that `ScanCommand` is accessible. Since `Commands::Scan` now holds `command: ScanCommand`, the match pattern `cli::ScanCommand::Init` already works via the `cli` module path.

- [ ] **Step 3: Build and verify**

```bash
cargo build -p s3-gallery-cli 2>&1
```

Expected: Compilation succeeds.

- [ ] **Step 4: Test help output**

```bash
cargo run -p s3-gallery-cli -- scan --help
cargo run -p s3-gallery-cli -- scan init --help
cargo run -p s3-gallery-cli -- scan update --help
cargo run -p s3-gallery-cli -- scan sync --help
```

Expected: Each shows the correct subcommand structure and flags.

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/main.rs
git commit -m "feat: wire scan init/update/sync subcommands in main dispatch"
```

---

### Task 4: Verify full build and integration

**Files:**
- None (build only)

- [ ] **Step 1: Full build**

```bash
cargo build 2>&1
```

Expected: All crates compile without errors or warnings.

- [ ] **Step 2: Run existing tests**

```bash
cargo test 2>&1
```

Expected: All tests pass. Any test that used the old `scan` CLI syntax may need updating.

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "chore: fix tests and finalize scan subcommand split"
```