# Scan Refactoring: Tower Service Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor scan logic into a pipeline of 4 composable `tower::Layer` wrappers (Discover → Diff → Process → Aggregate), each sharing `ScanRequest`/`ScanResponse` types, producing detailed traffic/file/status statistics.

**Architecture:** `ServiceBuilder::new().layer(AggregateLayer).layer(ProcessLayer).layer(DiffLayer).layer(DiscoverLayer).service(S3Service)` — each layer generic over `I: Service<ScanRequest, Response=ScanResponse>`, calls inner first, then enriches response from DB.

**Tech Stack:** Rust, tower (Service/Layer), sqlx (SqlitePool), S3Service (existing tower::Service wrapper)

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/s3-gallery-core/src/scan/pipeline.rs` | Create | Shared types: `ScanRequest`, `ScanResponse`, `HostInfo`, `HostDiffResult`, `HostProcessResult`, `AggregateReport`, `TrafficByOperation`, `SizeRanges` |
| `crates/s3-gallery-core/src/scan/discover.rs` | Create | `DiscoverLayer` + `DiscoverService` — discovers hosts, lists S3 objects, writes `scan_objects` table |
| `crates/s3-gallery-core/src/scan/diff.rs` | Modify | Add `HostDiffResult` + `apply_diff()` function (keep existing `diff_objects()` intact) |
| `crates/s3-gallery-core/src/scan/process.rs` | Create | `ProcessLayer` + `ProcessService` — EXIF extraction for pending files |
| `crates/s3-gallery-core/src/scan/aggregate.rs` | Create | `AggregateLayer` + `AggregateService` — reads DB + TrafficCounters, produces `AggregateReport` |
| `crates/s3-gallery-core/src/scan/mod.rs` | Modify | Add `pub mod pipeline`, `pub mod discover`, `pub mod process`, `pub mod aggregate` |
| `crates/s3-gallery-core/src/scan/scan_objects.rs` | Create | `ScanObjectEntry` model — CRUD for `scan_objects` table |
| `crates/s3-gallery-core/src/db/schema.rs` | Modify | Add `CREATE TABLE IF NOT EXISTS scan_objects (...)` migration |
| `crates/s3-gallery-core/src/scan/scanner.rs` | Modify | Keep `run_scan()` as backward-compat wrapper that builds pipeline internally |
| `crates/s3-gallery-core/src/lib.rs` | No change | `pub mod scan` already exists |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Modify | Replace orchestration with `pipeline.call()` |

---

### Task 1: Add `scan_objects` table migration + ScanObjectEntry model

**Files:**
- Create: `crates/s3-gallery-core/src/scan/scan_objects.rs`
- Modify: `crates/s3-gallery-core/src/db/schema.rs`

- [ ] **Step 1: Add `scan_objects` table to schema.rs**

Add this DDL in `run_migrations()` after the `dir_sizes` table (around line 193):

```rust
// scan_objects — snapshot of S3 listing for one scan
execute_query(
    pool,
    "CREATE TABLE IF NOT EXISTS scan_objects (
        scan_id TEXT NOT NULL,
        host_id TEXT NOT NULL,
        key TEXT NOT NULL,
        etag TEXT NOT NULL,
        size INTEGER NOT NULL,
        last_modified TEXT NOT NULL,
        is_deleted INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (scan_id, key)
    );",
)
.await?;

execute_query(
    pool,
    "CREATE INDEX IF NOT EXISTS idx_scan_objects_scan_id ON scan_objects(scan_id);",
)
.await?;

execute_query(
    pool,
    "CREATE INDEX IF NOT EXISTS idx_scan_objects_host_id ON scan_objects(host_id);",
)
.await?;
```

- [ ] **Step 2: Create `scan_objects.rs` with ScanObjectEntry model**

```rust
//! ScanObjectEntry — snapshot of S3 objects for one scan.

use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};
use crate::s3::client::ObjectSummary;

/// A row in the `scan_objects` table — snapshot of S3 listing for one scan.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScanObjectEntry {
    pub scan_id: String,
    pub host_id: String,
    pub key: String,
    pub etag: String,
    pub size: i64,
    pub last_modified: String,
    pub is_deleted: bool,
}

impl ScanObjectEntry {
    /// Batch insert scan objects from an S3 listing.
    pub async fn batch_insert(
        pool: &SqlitePool,
        scan_id: &str,
        host_id: &str,
        objects: &[ObjectSummary],
    ) -> Result<()> {
        // Use a transaction for batch insert
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to begin transaction: {e}")))?;

        for obj in objects {
            sqlx::query(
                "INSERT OR IGNORE INTO scan_objects (scan_id, host_id, key, etag, size, last_modified, is_deleted) \
                 VALUES (?, ?, ?, ?, ?, ?, 0)",
            )
            .bind(scan_id)
            .bind(host_id)
            .bind(obj.key.as_str())
            .bind(obj.etag.as_str())
            .bind(obj.size.as_u64() as i64)
            .bind(&obj.last_modified)
            .execute(&mut *tx)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to insert scan object: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// List all scan objects for a given scan_id and host_id.
    pub async fn list_by_scan(
        pool: &SqlitePool,
        scan_id: &str,
        host_id: &str,
    ) -> Result<Vec<ScanObjectEntry>> {
        sqlx::query_as::<_, ScanObjectEntry>(
            "SELECT * FROM scan_objects WHERE scan_id = ? AND host_id = ? AND is_deleted = 0 ORDER BY key",
        )
        .bind(scan_id)
        .bind(host_id)
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list scan objects: {e}")))
    }

    /// Delete all scan objects for a given scan_id.
    pub async fn delete_by_scan(pool: &SqlitePool, scan_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM scan_objects WHERE scan_id = ?")
            .bind(scan_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete scan objects: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::client::{Etag, FileSize, ObjectKey, ObjectSummary};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_batch_insert_and_list() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let objects = vec![
            ObjectSummary {
                key: ObjectKey::new("photos/a.jpg")?,
                etag: Etag::new("e1")?,
                size: FileSize::new(100),
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            },
            ObjectSummary {
                key: ObjectKey::new("photos/b.jpg")?,
                etag: Etag::new("e2")?,
                size: FileSize::new(200),
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            },
        ];

        ScanObjectEntry::batch_insert(&pool, "scan-1", "host-1", &objects).await?;

        let entries = ScanObjectEntry::list_by_scan(&pool, "scan-1", "host-1").await?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "photos/a.jpg");
        assert_eq!(entries[1].key, "photos/b.jpg");

        // Delete by scan
        ScanObjectEntry::delete_by_scan(&pool, "scan-1").await?;
        let entries = ScanObjectEntry::list_by_scan(&pool, "scan-1", "host-1").await?;
        assert!(entries.is_empty());

        Ok(())
    }
}
```

- [ ] **Step 3: Run tests to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core scan_objects -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: add scan_objects table and ScanObjectEntry model"
```

---

### Task 2: Create shared pipeline types (ScanRequest, ScanResponse, HostInfo, AggregateReport)

**Files:**
- Create: `crates/s3-gallery-core/src/scan/pipeline.rs`

- [ ] **Step 1: Write the pipeline.rs file with all shared types**

```rust
//! Shared types for the scan pipeline — ScanRequest, ScanResponse, and
//! AggregateReport with traffic/file/status breakdowns.

use std::collections::HashMap;

use crate::s3::config::HostIdentifier;
use crate::types::ObjectKey;

/// Request — same for all pipeline layers.
#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub bucket: crate::types::BucketName,
    pub scope_prefix: ObjectKey,
    pub concurrency: usize,
    pub extract_metadata: bool,
    pub generate_thumbnails: bool,
    pub client_id: String,
}

/// Response — each layer fills its section.
#[derive(Debug, Default)]
pub struct ScanResponse {
    // DiscoverLayer fills:
    pub scan_id: String,
    pub hosts: Vec<HostInfo>,

    // DiffLayer fills:
    pub diff_results: Vec<HostDiffResult>,

    // ProcessLayer fills:
    pub process_results: Vec<HostProcessResult>,

    // AggregateLayer fills:
    pub report: Option<AggregateReport>,
}

/// A discovered host.
#[derive(Debug, Clone)]
pub struct HostInfo {
    pub host_id: String,
    pub host_name: String,
    pub prefix: ObjectKey,
    pub config: Option<HostIdentifier>,
}

/// Result of diffing scan_objects against files table for one host.
#[derive(Debug, Clone)]
pub struct HostDiffResult {
    pub host_id: String,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub unchanged_count: u64,
    pub file_type_counts: HashMap<String, u64>,
}

/// Result of processing (EXIF extraction) for one host.
#[derive(Debug, Clone)]
pub struct HostProcessResult {
    pub host_id: String,
    pub processed_count: u64,
    pub failed_count: u64,
}

/// Traffic breakdown by operation type for a single stage.
#[derive(Debug, Clone, Default)]
pub struct TrafficByOperation {
    pub count: u64,
    pub bytes: u64,
}

/// Size distribution ranges.
#[derive(Debug, Clone, Default)]
pub struct SizeRanges {
    pub tiny: u64,   // 0-1KB
    pub small: u64,  // 1KB-100KB
    pub medium: u64, // 100KB-1MB
    pub large: u64,  // 1MB-10MB
    pub huge: u64,   // 10MB+
}

/// Final aggregate report with all statistics.
#[derive(Debug, Clone)]
pub struct AggregateReport {
    // A: Traffic statistics
    pub traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>>,
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub total_requests: u64,
    pub estimated_cost: f64,

    // B: File type statistics
    pub file_type_breakdown: HashMap<String, u64>,
    pub size_ranges: SizeRanges,

    // C: Scan status statistics
    pub total_files: u64,
    pub total_size: u64,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub host_count: u64,
    pub duration_secs: f64,
}

impl Default for AggregateReport {
    fn default() -> Self {
        Self {
            traffic_by_stage: HashMap::new(),
            total_download_bytes: 0,
            total_upload_bytes: 0,
            total_requests: 0,
            estimated_cost: 0.0,
            file_type_breakdown: HashMap::new(),
            size_ranges: SizeRanges::default(),
            total_files: 0,
            total_size: 0,
            new_files: 0,
            changed_files: 0,
            deleted_files: 0,
            host_count: 0,
            duration_secs: 0.0,
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-core 2>&1 | head -20`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: add shared pipeline types (ScanRequest, ScanResponse, AggregateReport)"
```

---

### Task 3: Implement DiscoverLayer + DiscoverService

**Files:**
- Create: `crates/s3-gallery-core/src/scan/discover.rs`

- [ ] **Step 1: Create discover.rs with DiscoverLayer and DiscoverService**

```rust
//! DiscoverLayer — discovers hosts from S3, lists objects, writes scan_objects table.
//!
//! This is the innermost pipeline layer, wrapping S3Service directly.
//! It is NOT generic — it wraps S3Service because S3Service is
//! Service<S3Request, Response=S3Response>, not Service<ScanRequest, ...>.
//! The outer layers (DiffLayer, ProcessLayer, AggregateLayer) are generic.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};
use uuid::Uuid;

use crate::db::models::HostConfigEntry;
use crate::error::{Result, S3GalleryError};
use crate::s3::config::HostIdentifier;
use crate::s3::s3_service::S3Service;
use crate::scan::pipeline::{HostInfo, ScanRequest, ScanResponse};
use crate::scan::scan_objects::ScanObjectEntry;
use crate::types::ObjectKey;
use sqlx::SqlitePool;

```rust
//! DiscoverLayer — discovers hosts from S3, lists objects, writes scan_objects table.
//!
//! This is the innermost pipeline layer, wrapping S3Service directly.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};
use uuid::Uuid;

use crate::db::models::HostConfigEntry;
use crate::error::{Result, S3GalleryError};
use crate::s3::config::HostIdentifier;
use crate::s3::s3_service::S3Service;
use crate::scan::pipeline::{HostInfo, ScanRequest, ScanResponse};
use crate::scan::scan_objects::ScanObjectEntry;
use crate::types::ObjectKey;
use sqlx::SqlitePool;

/// DiscoverLayer wraps S3Service with host discovery logic.
pub struct DiscoverLayer {
    db: SqlitePool,
}

impl DiscoverLayer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl Layer<S3Service> for DiscoverLayer {
    type Service = DiscoverService;

    fn layer(&self, inner: S3Service) -> Self::Service {
        DiscoverService {
            s3: inner,
            db: self.db.clone(),
        }
    }
}

/// DiscoverService — discovers hosts, lists objects, writes scan_objects table.
///
/// This is the innermost pipeline layer. It directly uses S3Service convenience
/// methods for S3 operations (list_objects, get_object).
pub struct DiscoverService {
    s3: S3Service,
    db: SqlitePool,
}

impl Service<ScanRequest> for DiscoverService {
    type Response = ScanResponse;
    type Error = S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ScanRequest) -> Self::Future {
        // We need to clone self because we return a Future
        // But S3Service has convenience methods that take &mut self
        // We need to handle this carefully
        let db = self.db.clone();
        let bucket = req.bucket.clone();
        let scope_prefix = req.scope_prefix.clone();
        let client_id = req.client_id.clone();

        // For the S3 operations, we need to use the s3 field
        // Since we can't clone DiscoverService easily (S3Service is Clone),
        // we extract what we need
        let mut s3 = self.s3.clone();

        Box::pin(async move {
            let scan_id = Uuid::new_v4().to_string();

            // Determine scope prefix string
            let scope_prefix_str = if scope_prefix.as_str().is_empty() {
                String::new()
            } else {
                format!("{}/", scope_prefix.as_str().trim_end_matches('/'))
            };

            // Try to read host.config.json at scope root
            let config_key_str = format!("{}.s3-gallery/host.config.json", scope_prefix_str);
            let config_key = ObjectKey::new(config_key_str.clone())
                .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

            let root_config = s3.get_object(&bucket, &config_key).await.ok();

            let hosts = if let Some(data) = root_config {
                // CASE 1: Single host at root
                let host: HostIdentifier = serde_json::from_slice(&data)
                    .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                // Save host config
                HostConfigEntry::upsert_host_config(
                    &db,
                    &host.host_id,
                    bucket.as_str(),
                    "", // endpoint — caller should fill
                    "", // region — caller should fill
                )
                .await?;

                let prefix = ObjectKey::new(scope_prefix_str.clone())
                    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                vec![HostInfo {
                    host_id: host.host_id.clone(),
                    host_name: host.host_name.clone(),
                    prefix,
                    config: Some(host),
                }]
            } else {
                // CASE 2: No root config — discover hosts in subdirectories
                let list_prefix = ObjectKey::new(scope_prefix_str.clone())
                    .map_err(|e| S3GalleryError::Internal(format!("Invalid prefix: {e}")))?;

                let all_objects = s3.list_objects(&bucket, &list_prefix).await?;

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

                let mut discovered = Vec::new();
                for dir in &subdirs {
                    let dir_prefix_str = format!("{}{}/", scope_prefix_str, dir);
                    let dir_config_key_str =
                        format!("{}{}/.s3-gallery/host.config.json", scope_prefix_str, dir);
                    let dir_config_key = ObjectKey::new(dir_config_key_str)
                        .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

                    let dir_config = s3.get_object(&bucket, &dir_config_key).await.ok();

                    if let Some(data) = dir_config {
                        // This subdirectory is a configured host
                        let host: HostIdentifier = serde_json::from_slice(&data)
                            .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                        HostConfigEntry::upsert_host_config(
                            &db,
                            &host.host_id,
                            bucket.as_str(),
                            "",
                            "",
                        )
                        .await?;

                        let prefix = ObjectKey::new(dir_prefix_str.clone())
                            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                        discovered.push(HostInfo {
                            host_id: host.host_id.clone(),
                            host_name: host.host_name.clone(),
                            prefix,
                            config: Some(host),
                        });
                    } else {
                        // No host config — use dir name as host_id
                        HostConfigEntry::upsert_host_config(
                            &db,
                            dir,
                            bucket.as_str(),
                            "",
                            "",
                        )
                        .await?;

                        let prefix = ObjectKey::new(dir_prefix_str.clone())
                            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                        discovered.push(HostInfo {
                            host_id: dir.clone(),
                            host_name: dir.clone(),
                            prefix,
                            config: None,
                        });
                    }
                }
                discovered
            };

            // List objects for each host and write to scan_objects table
            for host in &hosts {
                let objects = s3.list_objects(&bucket, &host.prefix).await?;

                // Filter out .s3-gallery directory
                let filtered: Vec<_> = objects
                    .into_iter()
                    .filter(|obj| {
                        let key = obj.key.as_str();
                        !key.contains("/.s3-gallery/") && !key.starts_with(".s3-gallery/")
                    })
                    .collect();

                ScanObjectEntry::batch_insert(&db, &scan_id, &host.host_id, &filtered).await?;
            }

            tracing::info!(
                target: "s3_gallery::scan",
                scan_id = %scan_id,
                hosts = hosts.len(),
                "Discover phase completed"
            );

            Ok(ScanResponse {
                scan_id,
                hosts,
                ..Default::default()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::mock::MockS3Client;
    use crate::types::BucketName;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_discover_empty_prefix() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let mock = Arc::new(MockS3Client::new());
        let s3 = S3Service::new(mock);
        let mut discover = DiscoverService {
            s3,
            db: pool.clone(),
        };

        let req = ScanRequest {
            bucket: BucketName::new("test-bucket")?,
            scope_prefix: ObjectKey::new("")?,
            concurrency: 10,
            extract_metadata: false,
            generate_thumbnails: false,
            client_id: "test".to_string(),
        };

        let resp = discover.call(req).await?;
        assert_eq!(resp.hosts.len(), 0);
        assert!(!resp.scan_id.is_empty());

        Ok(())
    }
}
```

- [ ] **Step 2: Run tests to verify**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core discover::tests -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: implement DiscoverLayer and DiscoverService"
```

---

### Task 4: Implement DiffLayer + DiffService

**Files:**
- Create: `crates/s3-gallery-core/src/scan/diff_layer.rs` (or modify `diff.rs`)

Actually, let me keep `diff.rs` as the pure diff function, and add the layer in a separate file or in the same file.

Better: add `apply_diff()` to `diff.rs` and create the layer in `diff_layer.rs`.

- [ ] **Step 1: Add `apply_diff()` function to `diff.rs`**

```rust
/// Apply a diff result to the database: upsert new/changed, mark deleted.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if any database operation fails.
pub async fn apply_diff(
    pool: &sqlx::SqlitePool,
    host_id: &str,
    diff: &DiffResult,
    classify_file: impl Fn(&ObjectKey) -> crate::types::FileType,
    get_content_type: impl Fn(&ObjectKey) -> Option<String>,
) -> Result<()> {
    use crate::db::models::FileEntry;

    for obj in &diff.new_objects {
        let file_type = classify_file(&obj.key);
        let content_type = get_content_type(&obj.key);

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: host_id.to_string(),
                key: obj.key.as_str().to_string(),
                etag: obj.etag.as_str().to_string(),
                size: obj.size.as_u64() as i64,
                last_modified: obj.last_modified.clone(),
                content_type,
                file_type: file_type.to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;
    }

    for obj in &diff.changed_objects {
        let file_type = classify_file(&obj.key);
        let content_type = get_content_type(&obj.key);

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: host_id.to_string(),
                key: obj.key.as_str().to_string(),
                etag: obj.etag.as_str().to_string(),
                size: obj.size.as_u64() as i64,
                last_modified: obj.last_modified.clone(),
                content_type,
                file_type: file_type.to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;
    }

    for key in &diff.deleted_keys {
        FileEntry::mark_deleted(pool, host_id, key).await?;
    }

    Ok(())
}
```

- [ ] **Step 2: Create `diff_layer.rs` with DiffLayer and DiffService**

```rust
//! DiffLayer — compares scan_objects against files table, updates files table.
//!
//! This is the second layer, generic over inner service.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};
use sqlx::SqlitePool;

use crate::classify::classifier::{classify_extension, content_type_from_extension, parse_extension};
use crate::error::Result;
use crate::s3::client::ObjectKey;
use crate::scan::diff::{apply_diff, diff_objects, DiffResult};
use crate::scan::pipeline::{HostDiffResult, ScanRequest, ScanResponse};
use crate::scan::scan_objects::ScanObjectEntry;
use crate::types::FileType;

/// DiffLayer wraps an inner service with DB diff logic.
pub struct DiffLayer {
    db: SqlitePool,
}

impl DiffLayer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl<I> Layer<I> for DiffLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError> + Clone,
{
    type Service = DiffService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        DiffService {
            inner,
            db: self.db.clone(),
        }
    }
}

/// DiffService — calls inner, then diffs scan_objects against files table.
pub struct DiffService<I> {
    inner: I,
    db: SqlitePool,
}

impl<I> Service<ScanRequest> for DiffService<I>
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError> + Clone,
{
    type Response = ScanResponse;
    type Error = crate::error::S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ScanRequest) -> Self::Future {
        // Clone what we need
        let mut inner = self.inner.clone();
        let db = self.db.clone();

        Box::pin(async move {
            // 1. Call inner (DiscoverLayer)
            let mut resp = inner.call(req).await?;

            // 2. For each host, diff scan_objects against files table
            let mut diff_results = Vec::new();
            for host in &resp.hosts {
                let scan_objects = ScanObjectEntry::list_by_scan(
                    &db, &resp.scan_id, &host.host_id,
                ).await?;

                // Convert to ObjectSummary for diff
                let s3_objects: Vec<_> = scan_objects
                    .iter()
                    .map(|entry| {
                        use crate::s3::client::{Etag, FileSize, ObjectSummary};
                        ObjectSummary {
                            key: ObjectKey::new(entry.key.clone())
                                .expect("valid key from DB"),
                            etag: Etag::new(entry.etag.clone())
                                .expect("valid etag from DB"),
                            size: FileSize::new(entry.size as u64),
                            last_modified: entry.last_modified.clone(),
                        }
                    })
                    .collect();

                // Get existing DB entries
                let db_entries = crate::db::models::FileEntry::list_by_prefix(
                    &db, &host.host_id, host.prefix.as_str(),
                ).await?;

                // Diff
                let diff = diff_objects(&s3_objects, &db_entries);

                // Apply diff to files table
                apply_diff(
                    &db,
                    &host.host_id,
                    &diff,
                    |key| {
                        if let Some(name) = key.file_name() {
                            if let Some(ext) = parse_extension(name) {
                                return classify_extension(&ext);
                            }
                        }
                        FileType::Unknown
                    },
                    |key| {
                        if let Some(name) = key.file_name() {
                            if let Some(ext) = parse_extension(name) {
                                return content_type_from_extension(&ext);
                            }
                        }
                        None
                    },
                ).await?;

                // Compute file type counts from new+changed objects
                let mut file_type_counts = std::collections::HashMap::new();
                for obj in diff.new_objects.iter().chain(diff.changed_objects.iter()) {
                    let ft = if let Some(name) = obj.key.file_name() {
                        if let Some(ext) = parse_extension(name) {
                            classify_extension(&ext).to_string()
                        } else {
                            "unknown".to_string()
                        }
                    } else {
                        "unknown".to_string()
                    };
                    *file_type_counts.entry(ft).or_insert(0) += 1;
                }

                diff_results.push(HostDiffResult {
                    host_id: host.host_id.clone(),
                    new_files: diff.new_objects.len() as u64,
                    changed_files: diff.changed_objects.len() as u64,
                    deleted_files: diff.deleted_keys.len() as u64,
                    unchanged_count: diff.unchanged_count,
                    file_type_counts,
                });
            }

            resp.diff_results = diff_results;
            Ok(resp)
        })
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-core 2>&1 | head -30`
Expected: Compilation succeeds

- [ ] **Step 4: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: implement DiffLayer and DiffService, add apply_diff helper"
```

---

### Task 5: Implement ProcessLayer + ProcessService

**Files:**
- Create: `crates/s3-gallery-core/src/scan/process.rs`

- [ ] **Step 1: Create process.rs with ProcessLayer and ProcessService**

The ProcessService extracts metadata for files with `metadata_state='pending'`. It extracts EXIF extraction logic from the existing `scanner.rs::process_file_metadata()`.

```rust
//! ProcessLayer — extracts metadata for pending files.
//!
//! This is the third pipeline layer, generic over inner service.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chrono::Utc;
use tower::{Layer, Service};
use sqlx::SqlitePool;

use crate::db::models::{FileEntry, MetadataEntry, TagEntry, FileTagEntry};
use crate::error::{Result, S3GalleryError};
use crate::extractor::exif::ExifExtractor;
use crate::extractor::registry::ExtractorRegistry;
use crate::extractor::tag_rules::{evaluate_all, parse_dms, TagRule};
use crate::s3::s3_service::S3Service;
use crate::scan::pipeline::{HostProcessResult, ScanRequest, ScanResponse};
use crate::classify::classifier::parse_extension;

/// ProcessLayer wraps an inner service with metadata extraction.
pub struct ProcessLayer {
    db: SqlitePool,
    exif_s3: S3Service,
}

impl ProcessLayer {
    pub fn new(db: SqlitePool, exif_s3: S3Service) -> Self {
        Self { db, exif_s3 }
    }
}

impl<I> Layer<I> for ProcessLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError> + Clone,
{
    type Service = ProcessService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        ProcessService {
            inner,
            db: self.db.clone(),
            exif_s3: self.exif_s3.clone(),
        }
    }
}

/// ProcessService — calls inner, then extracts metadata for pending files.
pub struct ProcessService<I> {
    inner: I,
    db: SqlitePool,
    exif_s3: S3Service,
}

impl<I> Service<ScanRequest> for ProcessService<I>
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError> + Clone,
{
    type Response = ScanResponse;
    type Error = crate::error::S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ScanRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        let db = self.db.clone();
        let mut exif_s3 = self.exif_s3.clone();
        let extract_metadata = req.extract_metadata;
        let bucket = req.bucket.clone(); // Clone before req is consumed

        Box::pin(async move {
            // 1. Call inner (DiffLayer → DiscoverLayer)
            let mut resp = inner.call(req).await?;

            // 2. Extract metadata for pending files if enabled
            let mut process_results = Vec::new();
            for host in &resp.hosts {
                if !extract_metadata {
                    process_results.push(HostProcessResult {
                        host_id: host.host_id.clone(),
                        processed_count: 0,
                        failed_count: 0,
                    });
                    continue;
                }

                // Find pending files for this host
                let pending = sqlx::query_as::<_, FileEntry>(
                    "SELECT * FROM files WHERE host_id = ? AND metadata_state = 'pending' AND is_deleted = 0",
                )
                .bind(&host.host_id)
                .fetch_all(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(format!("Failed to query pending files: {e}")))?;

                let mut registry = ExtractorRegistry::new();
                registry.register(Box::new(ExifExtractor::new()));
                let tag_rules = TagRule::default_rules();

                let mut processed = 0u64;
                let mut failed = 0u64;

                for entry in &pending {
                    let key_str = entry.key.as_str();
                    let file_name = key_str.rsplit('/').next().unwrap_or(key_str);
                    let ext = match parse_extension(file_name) {
                        Some(e) => e,
                        None => continue,
                    };

                    // Check if any extractor supports this file type
                    let file_type = crate::classify::classifier::classify_extension(&ext).to_string();
                    if registry.find(&file_type, ext.as_str()).is_empty() {
                        // Mark as extracted so we don't retry
                        let _ = sqlx::query(
                            "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                        )
                        .bind(&host.host_id)
                        .bind(key_str)
                        .execute(&db)
                        .await;
                        continue;
                    }

                    // Download 64KB for EXIF
                    let obj_key = match crate::types::ObjectKey::new(key_str.to_string()) {
                        Ok(k) => k,
                        Err(_) => continue,
                    };

                    let data = match exif_s3
                        .get_object_range(&bucket, &obj_key, 0, 65536)
                        .await
                    {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(key = %key_str, error = %e, "failed to download range for metadata");
                            let _ = sqlx::query(
                                "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
                            )
                            .bind(&host.host_id)
                            .bind(key_str)
                            .execute(&db)
                            .await;
                            failed += 1;
                            continue;
                        }
                    };

                    // Extract metadata
                    let items = match registry.extract_all(&data, &file_type, ext.as_str()).await {
                        Ok(items) => items,
                        Err(e) => {
                            tracing::warn!(key = %key_str, error = %e, "metadata extraction failed");
                            let _ = sqlx::query(
                                "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
                            )
                            .bind(&host.host_id)
                            .bind(key_str)
                            .execute(&db)
                            .await;
                            failed += 1;
                            continue;
                        }
                    };

                    if items.is_empty() {
                        let _ = sqlx::query(
                            "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                        )
                        .bind(&host.host_id)
                        .bind(key_str)
                        .execute(&db)
                        .await;
                        continue;
                    }

                    // Store metadata
                    let now = Utc::now().to_rfc3339();
                    for item in &items {
                        MetadataEntry::insert(&db, &MetadataEntry {
                            file_key: key_str.to_string(),
                            namespace: item.namespace.to_string(),
                            key: item.key.clone(),
                            value: item.value.clone(),
                            extracted_at: now.clone(),
                            partial: false,
                        }).await?;
                    }

                    // Generate and store tags
                    let tags = evaluate_all(&tag_rules, &items, &file_type);
                    for tag in &tags {
                        let tag_id = TagEntry::ensure_exists(&db, &tag.tag_name, &tag.tag_type).await?;
                        FileTagEntry::insert(&db, &FileTagEntry {
                            file_key: key_str.to_string(),
                            tag_id,
                        }).await?;
                    }

                    // Add exif:yes tag
                    let exif_tag_id = TagEntry::ensure_exists(&db, "exif:yes", "auto").await?;
                    FileTagEntry::insert(&db, &FileTagEntry {
                        file_key: key_str.to_string(),
                        tag_id: exif_tag_id,
                    }).await?;

                    // Update effective_date
                    let exif_date = items
                        .iter()
                        .find(|m| m.key == "DateTimeOriginal" || m.key == "DateTimeDigitized")
                        .map(|m| m.value.as_str())
                        .and_then(|v| v.get(..10))
                        .map(|d| d.replace(":", "-"));
                    let effective_date = exif_date
                        .as_deref()
                        .unwrap_or_else(|| &entry.last_modified[..10.min(entry.last_modified.len())]);

                    sqlx::query(
                        "UPDATE files SET effective_date = ?, metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                    )
                    .bind(effective_date)
                    .bind(&host.host_id)
                    .bind(key_str)
                    .execute(&db)
                    .await
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

                    processed += 1;
                }

                process_results.push(HostProcessResult {
                    host_id: host.host_id.clone(),
                    processed_count: processed,
                    failed_count: failed,
                });
            }

            resp.process_results = process_results;
            Ok(resp)
        })
    }
}
```

Wait, there's a problem — I need the bucket from `ScanRequest` inside the ProcessService's call, but I'm using `bucket` which is in `req` that gets consumed by the inner call. Let me fix this — I need to clone the bucket before calling inner.

Actually, looking at the code more carefully, `req` is passed to `inner.call(req)` which consumes it. But `ScanRequest` derives `Clone`. So I can clone it first.

Let me fix the ProcessService:

```rust
fn call(&mut self, req: ScanRequest) -> Self::Future {
    let mut inner = self.inner.clone();
    let db = self.db.clone();
    let mut exif_s3 = self.exif_s3.clone();
    let extract_metadata = req.extract_metadata;
    let bucket = req.bucket.clone(); // Clone bucket before consuming req

    Box::pin(async move {
        // 1. Call inner
        let mut resp = inner.call(req).await?;
        // ... use bucket ...
    })
}
```

This is getting complex. Let me simplify the plan by noting that the extractor logic will be adapted from the existing `process_file_metadata()` in scanner.rs.

- [ ] **Step 2: Run tests**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-core 2>&1 | head -30`
Expected: Compilation succeeds

- [ ] **Step 3: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: implement ProcessLayer and ProcessService with metadata extraction"
```

---

### Task 6: Implement AggregateLayer + AggregateService

**Files:**
- Create: `crates/s3-gallery-core/src/scan/aggregate.rs`

- [ ] **Step 1: Create aggregate.rs with AggregateLayer and AggregateService**

```rust
//! AggregateLayer — reads DB + TrafficCounters, produces AggregateReport.
//!
//! This is the outermost pipeline layer, generic over inner service.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use std::collections::HashMap;

use tower::{Layer, Service};
use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};
use crate::s3::traffic_persist::flush_counters;
use crate::s3::traffic_recorder::TrafficCounters;
use crate::scan::pipeline::{AggregateReport, ScanRequest, ScanResponse, SizeRanges, TrafficByOperation};
use crate::scan::scan_objects::ScanObjectEntry;

/// AggregateLayer wraps an inner service with report generation.
pub struct AggregateLayer {
    db: SqlitePool,
    counters: Arc<TrafficCounters>,
}

impl AggregateLayer {
    pub fn new(db: SqlitePool, counters: Arc<TrafficCounters>) -> Self {
        Self { db, counters }
    }
}

impl<I> Layer<I> for AggregateLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError> + Clone,
{
    type Service = AggregateService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        AggregateService {
            inner,
            db: self.db.clone(),
            counters: self.counters.clone(),
        }
    }
}

/// AggregateService — calls inner, then generates AggregateReport from DB + counters.
pub struct AggregateService<I> {
    inner: I,
    db: SqlitePool,
    counters: Arc<TrafficCounters>,
}

impl<I> Service<ScanRequest> for AggregateService<I>
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError> + Clone,
{
    type Response = ScanResponse;
    type Error = crate::error::S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ScanRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        let db = self.db.clone();
        let counters = self.counters.clone();
        let start = Instant::now();

        Box::pin(async move {
            // 1. Call inner chain
            let mut resp = inner.call(req).await?;

            // 2. Flush traffic counters to DB
            flush_counters(&counters, &db).await;

            // 3. Read traffic from DB for this scan's hosts
            let mut traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>> = HashMap::new();

            // Read traffic_log entries for scan_discover and scan_exif
            let traffic_rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
                "SELECT business, operation, SUM(bytes), SUM(count) \
                 FROM traffic_log \
                 WHERE business IN ('scan_discover', 'scan_exif') \
                 AND recorded_at >= datetime('now', '-1 hour') \
                 GROUP BY business, operation"
            )
            .fetch_all(&db)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to read traffic: {e}")))?;

            let mut total_download = 0u64;
            let mut total_upload = 0u64;
            let mut total_requests = 0u64;

            for (business, operation, bytes, count) in &traffic_rows {
                let stage = traffic_by_stage
                    .entry(business.clone())
                    .or_default();
                stage.insert(
                    operation.clone(),
                    TrafficByOperation {
                        count: *count as u64,
                        bytes: *bytes as u64,
                    },
                );

                // Determine direction from operation name
                match operation.as_str() {
                    "GetObject" | "GetObjectRange" | "HeadObject" | "ListObjects" => {
                        total_download += *bytes as u64;
                    }
                    "PutObject" | "PutObjectIfNoneMatch" => {
                        total_upload += *bytes as u64;
                    }
                    _ => {
                        total_download += *bytes as u64;
                    }
                }
                total_requests += *count as u64;
            }

            // 4. Compute file type breakdown from diff_results
            let mut file_type_breakdown: HashMap<String, u64> = HashMap::new();
            let mut size_ranges = SizeRanges::default();

            for diff_result in &resp.diff_results {
                for (ft, count) in &diff_result.file_type_counts {
                    *file_type_breakdown.entry(ft.clone()).or_insert(0) += count;
                }
            }

            // 5. Compute size ranges from scan_objects
            for host in &resp.hosts {
                let objects = ScanObjectEntry::list_by_scan(
                    &db, &resp.scan_id, &host.host_id,
                ).await?;
                for obj in &objects {
                    match obj.size {
                        0..=1024 => size_ranges.tiny += 1,
                        1025..=102400 => size_ranges.small += 1,
                        102401..=1048576 => size_ranges.medium += 1,
                        1048577..=10485760 => size_ranges.large += 1,
                        _ => size_ranges.huge += 1,
                    }
                }
            }

            // 6. Compute scan status totals from diff results
            let mut total_files = 0u64;
            let mut total_size = 0u64;
            let mut new_files = 0u64;
            let mut changed_files = 0u64;
            let mut deleted_files = 0u64;

            for diff_result in &resp.diff_results {
                new_files += diff_result.new_files;
                changed_files += diff_result.changed_files;
                deleted_files += diff_result.deleted_files;
                total_files += diff_result.new_files + diff_result.changed_files + diff_result.unchanged_count;
            }

            // Total size from files table
            let total_size_val: Option<i64> = sqlx::query_scalar(
                "SELECT SUM(size) FROM files WHERE host_id IN (SELECT host_id FROM scan_objects WHERE scan_id = ?) AND is_deleted = 0",
            )
            .bind(&resp.scan_id)
            .fetch_optional(&db)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to sum sizes: {e}")))?;
            total_size = total_size_val.unwrap_or(0) as u64;

            // 7. Calculate estimated cost ($0.09/GB download = $0.00009/MB)
            let estimated_cost = total_download as f64 * 0.00000009; // per byte

            // 8. Clean up scan_objects
            ScanObjectEntry::delete_by_scan(&db, &resp.scan_id).await?;

            resp.report = Some(AggregateReport {
                traffic_by_stage,
                total_download_bytes: total_download,
                total_upload_bytes: total_upload,
                total_requests,
                estimated_cost,
                file_type_breakdown,
                size_ranges,
                total_files,
                total_size,
                new_files,
                changed_files,
                deleted_files,
                host_count: resp.hosts.len() as u64,
                duration_secs: start.elapsed().as_secs_f64(),
            });

            Ok(resp)
        })
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-core 2>&1 | head -30`
Expected: Compilation succeeds

- [ ] **Step 3: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: implement AggregateLayer and AggregateService with report generation"
```

---

### Task 7: Update scan/mod.rs to export new modules

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/mod.rs`

- [ ] **Step 1: Update mod.rs**

```rust
pub mod aggregate;
pub mod diff;
pub mod diff_layer;
pub mod discover;
pub mod pipeline;
pub mod process;
pub mod scan_objects;
pub mod scanner;
```

- [ ] **Step 2: Verify compilation**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-core 2>&1 | head -30`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: update scan module exports"
```

---

### Task 8: Update cmd_scan.rs to use the pipeline

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`

- [ ] **Step 1: Refactor `run_scan_core` to build and use the pipeline**

Replace the core orchestration in `cmd_scan.rs` with a pipeline call:

```rust
use s3_gallery_core::scan::pipeline::ScanRequest;
use s3_gallery_core::scan::pipeline::ScanResponse;
use s3_gallery_core::scan::discover::DiscoverLayer;
use s3_gallery_core::scan::diff_layer::DiffLayer;
use s3_gallery_core::scan::process::ProcessLayer;
use s3_gallery_core::scan::aggregate::AggregateLayer;
use s3_gallery_core::s3::layers::{LogLayer, TrafficLayer};
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use s3_gallery_core::s3::traffic_persist::spawn_aggregator;
use s3_gallery_core::types::{BucketName, ObjectKey};
use tower::ServiceBuilder;

/// Core scan logic: build pipeline, run it, push DB to remote.
async fn run_scan_core(
    s3: Arc<dyn S3Client>,
    pool: &SqlitePool,
    bucket: &BucketName,
    scope_prefix: &str,
    opts: &ScanOptions,
    cli: &Cli,
) -> Result<()> {
    // Set up traffic tracking
    let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
    let _agg_handle = spawn_aggregator(recorder.clone(), pool.clone(), 60);

    // Build S3 service stacks with traffic layers
    let discover_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), "__scan", "scan_discover"))
        .service(S3Service::new(s3.clone()));

    let exif_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), "__scan", "scan_exif"))
        .service(S3Service::new(s3.clone()));

    // Build pipeline
    let mut pipeline = ServiceBuilder::new()
        .layer(AggregateLayer::new(pool.clone(), recorder.counters.clone()))
        .layer(ProcessLayer::new(pool.clone(), exif_s3))
        .layer(DiffLayer::new(pool.clone()))
        .layer(DiscoverLayer::new(pool.clone()))
        .service(discover_s3);

    let scope_prefix_key = ObjectKey::new(scope_prefix.to_string())
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

    let resp = pipeline
        .call(ScanRequest {
            bucket: bucket.clone(),
            scope_prefix: scope_prefix_key,
            concurrency: opts.concurrency,
            extract_metadata: opts.extract_metadata,
            generate_thumbnails: opts.with_thumbnails,
            client_id: format!("cli-{}", bucket.as_str()),
        })
        .await?;

    // Print report
    if let Some(report) = &resp.report {
        println!("Scan complete:");
        println!("  Hosts: {}", report.host_count);
        println!("  Total files: {}", report.total_files);
        println!("  New files: {}", report.new_files);
        println!("  Changed files: {}", report.changed_files);
        println!("  Deleted files: {}", report.deleted_files);
        println!("  Duration: {:.1}s", report.duration_secs);

        if !report.file_type_breakdown.is_empty() {
            println!("  File types:");
            for (ft, count) in &report.file_type_breakdown {
                println!("    {}: {}", ft, count);
            }
        }

        println!("  Traffic:");
        println!("    Download: {:.1} MB", report.total_download_bytes as f64 / 1_000_000.0);
        println!("    Upload: {:.1} MB", report.total_upload_bytes as f64 / 1_000_000.0);
        println!("    Requests: {}", report.total_requests);
        println!("    Est. Cost: ${:.4}", report.estimated_cost);
    }

    // Flush traffic counters to DB before pushing
    s3_gallery_core::s3::traffic_persist::flush_counters(&recorder.counters, &pool).await;

    // Auto push DB to remote
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
    let db_data = std::fs::read(&cli.db_path).map_err(S3GalleryError::IoError)?;
    s3.put_object(bucket, &db_key, &db_data).await?;
    println!("  DB pushed to remote.");

    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1 | head -50`
Expected: Compilation succeeds

- [ ] **Step 3: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: update cmd_scan.rs to use pipeline"
```

---

### Task 9: Update scanner.rs to use pipeline internally (backward-compat)

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`

- [ ] **Step 1: Refactor scanner.rs to use the pipeline internally**

Keep `run_scan()` as a backward-compatible wrapper that builds the pipeline and calls it.

```rust
/// Run a full scan using the pipeline internally.
///
/// This is a backward-compatible wrapper around the pipeline.
pub async fn run_scan(config: ScanConfig) -> Result<ScanResult> {
    use tower::ServiceBuilder;
    use std::sync::Arc;
    use crate::s3::traffic_recorder::TrafficRecorder;
    use crate::s3::traffic_persist::spawn_aggregator;
    use crate::scan::pipeline::ScanRequest;
    use crate::scan::discover::DiscoverLayer;
    use crate::scan::diff_layer::DiffLayer;
    use crate::scan::process::ProcessLayer;
    use crate::scan::aggregate::AggregateLayer;
    use crate::s3::layers::{LogLayer, TrafficLayer};

    let recorder = Arc::new(TrafficRecorder::new(config.db.clone()));
    let _agg_handle = spawn_aggregator(recorder.clone(), config.db.clone(), 60);

    let discover_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), &config.host_id, "scan_discover"))
        .service(S3Service::new(config.s3.into_inner()));

    let exif_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), &config.host_id, "scan_exif"))
        .service(S3Service::new(config.s3.into_inner()));

    let mut pipeline = ServiceBuilder::new()
        .layer(AggregateLayer::new(config.db.clone(), recorder.counters.clone()))
        .layer(ProcessLayer::new(config.db.clone(), exif_s3))
        .layer(DiffLayer::new(config.db.clone()))
        .layer(DiscoverLayer::new(config.db.clone()))
        .service(discover_s3);

    let resp = pipeline
        .call(ScanRequest {
            bucket: config.bucket.clone(),
            scope_prefix: config.prefix.clone(),
            concurrency: config.concurrency,
            extract_metadata: config.extract_metadata,
            generate_thumbnails: config.generate_thumbnails,
            client_id: config.client_id.clone(),
        })
        .await?;

    // Convert AggregateReport to ScanResult
    if let Some(report) = &resp.report {
        Ok(ScanResult {
            total_files: report.total_files,
            total_size: report.total_size,
            new_files: report.new_files,
            changed_files: report.changed_files,
            deleted_files: report.deleted_files,
            metadata_extracted: 0,
            duration_secs: report.duration_secs,
        })
    } else {
        Ok(ScanResult {
            total_files: 0,
            total_size: 0,
            new_files: 0,
            changed_files: 0,
            deleted_files: 0,
            metadata_extracted: 0,
            duration_secs: 0.0,
        })
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1 | head -50`
Expected: Compilation succeeds

- [ ] **Step 3: Run existing tests to make sure nothing broke**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core -- --nocapture`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
cd /Users/macbook/Project/s3-gallery && git add -A && git commit -m "feat: update scanner.rs to use pipeline internally"
```