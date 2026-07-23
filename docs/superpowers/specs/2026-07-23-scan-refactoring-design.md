# Scan Refactoring: Tower Service Pipeline Design

> **Goal:** Refactor scan logic into a pipeline of composable `tower::Service` layers, each with independent responsibility, sharing `ScanRequest`/`ScanResponse` types, and providing detailed traffic/file/status statistics.

**Architecture:** Four `tower::Layer` wrappers (`DiscoverLayer` → `DiffLayer` → `ProcessLayer` → `AggregateLayer`) composed via `ServiceBuilder`. Each layer delegates to an inner `Service<ScanRequest, Response=ScanResponse>` and enriches the response after the inner call completes. S3 traffic flows through `S3Service` with `TrafficLayer` business labels per stage.

**Tech Stack:** Rust, tower (Service/Layer traits), sqlx (SqlitePool), S3Service (existing tower::Service wrapper)

---

## Architecture

```
AggregateLayer
  wraps ProcessLayer
    wraps DiffLayer
      wraps DiscoverLayer
        wraps S3Service (with TrafficLayer "scan_discover")
```

### Call flow (top-down, then bottom-up)

```
call(AggregateLayer)
  → call(ProcessLayer)
    → call(DiffLayer)
      → call(DiscoverLayer)
        → S3Service.list_objects()     ← S3 调用，记录为 "scan_discover"
        → S3Service.get_object()       ← 读 host.config.json，记入 "scan_discover"
        ← 写入 scan_objects 表
      ← 读 DB scan_objects，对比 files 表
      ← 更新 files 表 (new/changed/deleted)
    ← 读 DB metadata_state='pending' 的文件
    ← S3Service.get_object_range()     ← 下载 64KB EXIF，记入 "scan_exif"
    ← 写入 metadata 表
  ← 读 DB 汇总统计
  ← 读 TrafficCounters 获取流量
  ← 返回 AggregateReport
```

### Key design rules

1. **Each layer is `Service<ScanRequest, Response=ScanResponse>`**, generic over inner service `I: Service<ScanRequest, Response=ScanResponse, Error=S3GalleryError> + Clone`
2. **Each layer calls inner.call() first**, then does its own work from DB, then returns enriched response
3. **S3 calls ONLY happen in DiscoverLayer (list/read config) and ProcessLayer (EXIF download)** — both use `S3Service` with stage-specific `TrafficLayer` labels
4. **DiffLayer is pure DB** — no S3 calls
5. **AggregateLayer is pure DB + TrafficCounters** — no S3 calls

---

## Shared Types

```rust
/// Request — same for all layers
#[derive(Clone)]
pub struct ScanRequest {
    pub bucket: BucketName,
    pub scope_prefix: ObjectKey,
    pub concurrency: usize,
    pub extract_metadata: bool,
    pub generate_thumbnails: bool,
    pub client_id: String,
}

/// Response — each layer fills its section
#[derive(Default)]
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

pub struct HostInfo {
    pub host_id: String,
    pub host_name: String,
    pub prefix: ObjectKey,
    pub config: Option<HostIdentifier>,
}

pub struct HostDiffResult {
    pub host_id: String,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub unchanged_count: u64,
    pub file_type_counts: HashMap<String, u64>,
}

pub struct HostProcessResult {
    pub host_id: String,
    pub processed_count: u64,
    pub failed_count: u64,
}

pub struct AggregateReport {
    // A: 流量统计
    pub traffic_by_stage: HashMap<String, TrafficByOperation>,
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub total_requests: u64,
    pub estimated_cost: f64,

    // B: 文件类型统计
    pub file_type_breakdown: HashMap<String, u64>,
    pub size_ranges: SizeRanges,

    // C: 扫描状态统计
    pub total_files: u64,
    pub total_size: u64,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub host_count: u64,
    pub duration_secs: f64,
}

pub struct TrafficByOperation {
    pub count: u64,
    pub bytes: u64,
}

pub struct SizeRanges {
    pub tiny: u64,     // 0-1KB
    pub small: u64,    // 1KB-100KB
    pub medium: u64,   // 100KB-1MB
    pub large: u64,    // 1MB-10MB
    pub huge: u64,     // 10MB+
}
```

---

## Layer Definitions

### DiscoverLayer (innermost)

Wraps `S3Service` (with `TrafficLayer("scan_discover")`). Discovers hosts from S3, lists objects, writes `scan_objects` table.

```rust
pub struct DiscoverLayer { db: SqlitePool }

impl Layer<S3Service> for DiscoverLayer {
    type Service = DiscoverService<S3Service>;
    fn layer(&self, inner: S3Service) -> Self::Service {
        DiscoverService { inner, db: self.db.clone() }
    }
}

pub struct DiscoverService<I> { inner: I, db: SqlitePool }

impl Service<ScanRequest> for DiscoverService<S3Service> {
    type Response = ScanResponse;
    type Error = S3GalleryError;

    fn call(&mut self, req: ScanRequest) -> ... {
        // 1. Generate scan_id
        // 2. Read host.config.json at scope root
        // 3. If found → single host; else → discover hosts in subdirectories
        // 4. For each host: list_objects → batch insert into scan_objects table
        // 5. Upsert host_config
        // 6. Return ScanResponse { scan_id, hosts, ..default() }
    }
}
```

### DiffLayer (2nd layer)

Generic over inner service. Pure DB — reads `scan_objects`, diffs against `files` table, updates `files` table.

```rust
pub struct DiffLayer { db: SqlitePool }

impl<I> Layer<I> for DiffLayer
where I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone
{
    type Service = DiffService<I>;
    fn layer(&self, inner: I) -> Self::Service {
        DiffService { inner, db: self.db.clone() }
    }
}

pub struct DiffService<I> { inner: I, db: SqlitePool }

impl<I> Service<ScanRequest> for DiffService<I>
where I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone
{
    type Response = ScanResponse;
    // call: inner.call() → for each host: read scan_objects → diff_objects() → apply_diff() → fill diff_results
}
```

### ProcessLayer (3rd layer)

Generic over inner service. Reads `files` table for `metadata_state='pending'` entries, downloads EXIF data via `S3Service` (with `TrafficLayer("scan_exif")`), writes metadata.

```rust
pub struct ProcessLayer { db: SqlitePool, exif_s3: S3Service }

impl<I> Layer<I> for ProcessLayer
where I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone
{
    type Service = ProcessService<I>;
    fn layer(&self, inner: I) -> Self::Service {
        ProcessService { inner, db: self.db.clone(), exif_s3: self.exif_s3.clone() }
    }
}
```

### AggregateLayer (outermost)

Generic over inner service. Pure DB + TrafficCounters. Reads aggregate stats from DB, snapshots atomic counters, returns final `AggregateReport`.

```rust
pub struct AggregateLayer { db: SqlitePool, counters: Arc<TrafficCounters> }

impl<I> Layer<I> for AggregateLayer
where I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone
{
    type Service = AggregateService<I>;
    fn layer(&self, inner: I) -> Self::Service {
        AggregateService { inner, db: self.db.clone(), counters: self.counters.clone() }
    }
}
```

---

## DB Schema: scan_objects table

```sql
CREATE TABLE IF NOT EXISTS scan_objects (
    scan_id TEXT NOT NULL,
    host_id TEXT NOT NULL,
    key TEXT NOT NULL,
    etag TEXT NOT NULL,
    size INTEGER NOT NULL,
    last_modified TEXT NOT NULL,
    PRIMARY KEY (scan_id, key)
);
```

This table holds the S3 snapshot for one scan. It is created/sourced in DiscoverLayer, consumed in DiffLayer, and cleaned up by AggregateLayer after the report is generated (DELETE FROM scan_objects WHERE scan_id = ?).

---

## Composition

```rust
let discover_s3 = ServiceBuilder::new()
    .layer(LogLayer)
    .layer(TrafficLayer::new(recorder.clone(), "__scan", "scan_discover"))
    .service(S3Service::new(raw_s3));

let exif_s3 = ServiceBuilder::new()
    .layer(LogLayer)
    .layer(TrafficLayer::new(recorder.clone(), "__scan", "scan_exif"))
    .service(S3Service::new(raw_s3));

let mut pipeline = ServiceBuilder::new()
    .layer(AggregateLayer::new(db.clone(), recorder.counters.clone()))
    .layer(ProcessLayer::new(db.clone(), exif_s3))
    .layer(DiffLayer::new(db.clone()))
    .layer(DiscoverLayer::new(db.clone()))
    .service(discover_s3);

let resp = pipeline.call(ScanRequest {
    bucket, scope_prefix, concurrency: 10,
    extract_metadata: true, generate_thumbnails: false,
    client_id: "cli-scan".into(),
}).await?;

let report = resp.report.unwrap();
println!("Total files: {}", report.total_files);
println!("Traffic: {} MB down, {} reqs",
    report.total_download_bytes / 1_000_000,
    report.total_requests);
```

---

## Traffic Recording

| Stage | Business Label | S3 Operations |
|-------|---------------|---------------|
| Host discovery | `scan_discover` | `list_objects`, `get_object` (config) |
| EXIF extraction | `scan_exif` | `get_object_range` (64KB) |

Traffic counters are read at the end of AggregateLayer via `flush_counters()` and included in `AggregateReport.traffic_by_stage`.

---

## Files to Touch

| File | Change |
|------|--------|
| `crates/s3-gallery-core/src/scan/mod.rs` | Add `pipeline` module |
| `crates/s3-gallery-core/src/scan/pipeline.rs` | New: `ScanRequest`, `ScanResponse`, `HostInfo`, `AggregateReport`, shared types |
| `crates/s3-gallery-core/src/scan/discover.rs` | New: `DiscoverLayer`, `DiscoverService` |
| `crates/s3-gallery-core/src/scan/diff.rs` | Keep existing `diff_objects()`, add `HostDiffResult` types |
| `crates/s3-gallery-core/src/scan/process.rs` | New: `ProcessLayer`, `ProcessService` |
| `crates/s3-gallery-core/src/scan/aggregate.rs` | New: `AggregateLayer`, `AggregateService` |
| `crates/s3-gallery-core/src/scan/scanner.rs` | Keep `run_scan()` as backward-compat wrapper, or deprecate |
| `crates/s3-gallery-core/src/scan/scan_objects.rs` | New: `scan_objects` table operations |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Replace orchestration with `pipeline.call()` |
| `crates/s3-gallery-core/src/db/schema.rs` | Add `scan_objects` table migration |
| `crates/s3-gallery-core/src/db/models.rs` | Add `ScanObjectEntry` model |