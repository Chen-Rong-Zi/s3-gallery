# Tower 迁移清理实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复上次 Tower 迁移的三个遗留问题——scan 层过度设计、view 层迁移未完成、业务代码直接使用 `Arc<dyn S3Client>`。

**Architecture:** (A) 4 个 scan 层去掉手写 `Service` trait，改用 `BoxService::new(service_fn(...))`；(B) view 层用 `fetch_scalar_opt`/`fetch_all_opt` 消除 `Option<host_id>` 分支；(C) 所有业务代码改用 `S3Service`，禁止直接使用 `Arc<dyn S3Client>`。

**Tech Stack:** Rust, `tower` 0.5 (Service, Layer, service_fn, boxed)

---

## File Structure

### Files to Modify

| 文件 | 变更 |
|------|------|
| `crates/s3-gallery-core/Cargo.toml` | 添加 tower "boxed" feature |
| `crates/s3-gallery-core/src/scan/discover.rs` | 去掉 `DiscoverService` 结构体，`Layer::layer()` 返回 `BoxService` |
| `crates/s3-gallery-core/src/scan/diff_layer.rs` | 去掉 `DiffService` 结构体，`Layer::layer()` 返回 `BoxService` |
| `crates/s3-gallery-core/src/scan/process.rs` | 去掉 `ProcessService` 结构体，`Layer::layer()` 返回 `BoxService` |
| `crates/s3-gallery-core/src/scan/aggregate.rs` | 去掉 `AggregateService` 结构体，`Layer::layer()` 返回 `BoxService` |
| `crates/s3-gallery-core/src/util/db_helpers.rs` | 添加 `fetch_scalar_opt` 辅助函数 |
| `crates/s3-gallery-core/src/view/stat.rs` | 5 处 scalar 查询改用 `fetch_scalar_opt` |
| `crates/s3-gallery-core/src/view/timeline_gallery.rs` | 1 处 `FileEntry` 查询改用 `fetch_all_opt` |
| `crates/s3-gallery-core/src/view/mod.rs` | 移除 `pub use remote::RemoteView` |
| `crates/s3-gallery-cli/src/cmd_serve.rs` | `s3_stack` 只加 `LogLayer`，不加 `TrafficLayer` |
| `crates/s3-gallery-cli/src/web/state.rs` | 移除 `s3_clients` 字段，添加 `s3_with_traffic()` 方法 |
| `crates/s3-gallery-cli/src/web/handlers/download.rs` | 使用 `state.s3_with_traffic()` |
| `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs` | 使用 `state.s3_with_traffic()` |
| `crates/s3-gallery-cli/src/cmd_init.rs` | 改用 `S3Service` 替代 `Arc<dyn S3Client>` |
| `crates/s3-gallery-cli/src/cmd_db.rs` | 改用 `S3Service` 替代 `Arc<dyn S3Client>` |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | DB 拉取/推送改用 `S3Service` |

### Files to Delete

| 文件 | 原因 |
|------|------|
| `crates/s3-gallery-core/src/view/remote.rs` | 死代码，未被使用 |

---

### Task 1: Add tower "boxed" feature

**Files:**
- Modify: `crates/s3-gallery-core/Cargo.toml`

- [ ] **Step 1: Add "boxed" to tower features**

Edit `crates/s3-gallery-core/Cargo.toml`, change the tower line from:
```toml
tower = { version = "0.5", features = ["buffer", "timeout", "limit", "make"] }
```
to:
```toml
tower = { version = "0.5", features = ["buffer", "timeout", "limit", "make", "boxed"] }
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p s3-gallery-core`
Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-core/Cargo.toml
git commit -m "chore: add tower boxed feature for BoxService"
```

---

### Task 2: Simplify 4 scan layers with BoxService + service_fn

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/discover.rs`
- Modify: `crates/s3-gallery-core/src/scan/diff_layer.rs`
- Modify: `crates/s3-gallery-core/src/scan/process.rs`
- Modify: `crates/s3-gallery-core/src/scan/aggregate.rs`

**Pattern for all 4 layers:**

Each layer follows the same pattern: remove the `*Service` struct, change `Layer::layer()` to return `BoxService<ScanRequest, ScanResponse, S3GalleryError>`, and use `tower::service_fn` inside.

- [ ] **Step 1: Simplify discover.rs**

**Current** (discover.rs has `DiscoverService` struct with full `Service<ScanRequest>` impl):
```rust
use tower::{Layer, Service};
// ... imports ...

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
        let db = self.db.clone();
        let bucket = req.bucket.clone();
        let scope_prefix = req.scope_prefix.clone();
        let mut s3 = self.s3.clone();
        Box::pin(async move {
            // ... host discovery logic (lines 73-220) ...
        })
    }
}
```

**After** — remove `DiscoverService`, change imports and `Layer::layer()`:
```rust
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::collections::BTreeSet;

use tower::boxed::BoxService;
use tower::service_fn;
use tower::{Layer, Service};
use uuid::Uuid;

// ... DiscoverLayer struct stays the same ...

impl Layer<S3Service> for DiscoverLayer {
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: S3Service) -> Self::Service {
        let db = self.db.clone();
        BoxService::new(service_fn(move |req: ScanRequest| {
            let mut s3 = inner.clone();
            let db = db.clone();
            async move {
                // ... same host discovery logic (exactly as before) ...
                // Note: remove `req` destructuring at top since it's already in the closure param
                let scan_id = Uuid::new_v4().to_string();
                let scope_prefix_str = if req.scope_prefix.as_str().is_empty() {
                    String::new()
                } else {
                    format!("{}/", req.scope_prefix.as_str().trim_end_matches('/'))
                };
                // ... rest of the logic unchanged ...
            }
        }))
    }
}

// Remove the entire DiscoverService struct and its impl Service block
```

**关键变动：**
1. 添加 imports: `use tower::boxed::BoxService; use tower::service_fn;`
2. 将 `Layer::layer()` 的 `type Service` 改为 `BoxService<ScanRequest, ScanResponse, S3GalleryError>`
3. `layer()` 方法体改为 `BoxService::new(service_fn(move |req: ScanRequest| { ... }))`
4. 删除整个 `DiscoverService` 结构体和 `impl Service<ScanRequest> for DiscoverService` 块
5. `call()` 方法中的逻辑移到 `service_fn` 闭包中，`req` 从闭包参数获取

**注意：** `DiscoverService` 的测试代码（`test_discover_empty_prefix`）直接构造了 `DiscoverService`，需要改为构造 `DiscoverService` 通过 `DiscoverLayer` 来创建 service：

```rust
// 修改测试：通过 Layer 构建，而不是直接构造 DiscoverService
let mut discover = DiscoverLayer::new(pool.clone()).layer(s3);
```

- [ ] **Step 2: Simplify diff_layer.rs**

Same pattern — remove `DiffService<I>`, change `Layer::layer()`:

```rust
// 删除 imports:
// use std::future::Future;
// use std::pin::Pin;
// use std::task::{Context, Poll};

// 添加:
use tower::boxed::BoxService;
use tower::service_fn;

// DiffLayer struct stays the same

impl<I> Layer<I> for DiffLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError>
        + Clone + Send + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        BoxService::new(service_fn(move |req: ScanRequest| {
            let mut inner = inner.clone();
            let db = db.clone();
            async move {
                // 1. Call inner (DiscoverLayer)
                let mut resp = inner.call(req).await?;

                // 2. For each host, diff scan_objects against files table
                let mut diff_results = Vec::new();
                for host in &resp.hosts {
                    // ... same logic as current call() method ...
                }

                resp.diff_results = diff_results;
                Ok(resp)
            }
        }))
    }
}

// 删除整个 DiffService<I> 结构体和 impl<I> Service<ScanRequest> for DiffService<I> 块
```

- [ ] **Step 3: Simplify process.rs**

Same pattern as diff_layer.rs — remove `ProcessService<I>`, change `Layer::layer()`:

```rust
use tower::boxed::BoxService;
use tower::service_fn;

impl<I> Layer<I> for ProcessLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError>
        + Clone + Send + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let mut exif_s3 = self.exif_s3.clone();
        let bucket = req.bucket.clone();  // NOPE — req is not available here
        // Actually, bucket and extract_metadata need to come from req
        BoxService::new(service_fn(move |req: ScanRequest| {
            let mut inner = inner.clone();
            let db = db.clone();
            let mut exif_s3 = exif_s3.clone();
            let extract_metadata = req.extract_metadata;
            let bucket = req.bucket.clone();
            async move {
                // 1. Call inner (DiffLayer -> DiscoverLayer)
                let mut resp = inner.call(req).await?;

                // 2. Extract metadata for pending files if enabled
                let mut process_results = Vec::new();
                for host in &resp.hosts {
                    // ... same logic as current call() method ...
                }

                resp.process_results = process_results;
                Ok(resp)
            }
        }))
    }
}

// 删除 ProcessService<I> 结构体和其 impl Service 块
```

- [ ] **Step 4: Simplify aggregate.rs**

Same pattern — remove `AggregateService<I>`, change `Layer::layer()`:

```rust
use tower::boxed::BoxService;
use tower::service_fn;

impl<I> Layer<I> for AggregateLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError>
        + Clone + Send + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let counters = self.counters.clone();
        BoxService::new(service_fn(move |req: ScanRequest| {
            let mut inner = inner.clone();
            let db = db.clone();
            let counters = counters.clone();
            let start = std::time::Instant::now();

            async move {
                // 1. Call inner chain
                let mut resp = inner.call(req).await?;

                // 2. Read traffic counters BEFORE flushing
                // ... same logic as current call() method ...
                let total_download = counters.download_bytes.load(Ordering::Relaxed);
                // ... etc ...

                // 3. Flush traffic counters to DB
                flush_counters(&counters, &db).await;

                // ... compute file type breakdown, size ranges, etc ...
                // ... same logic as current call() method ...

                resp.report = Some(AggregateReport { ... });
                Ok(resp)
            }
        }))
    }
}

// 删除 AggregateService<I> 结构体和其 impl Service 块
```

- [ ] **Step 5: Build and test**

Run: `cargo test -p s3-gallery-core -- scan 2>&1 | head -50`
Expected: All scan tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/scan/
git commit -m "refactor: replace hand-written Service impls with BoxService + service_fn in scan layers"
```

---

### Task 3: Complete view layer migration

**Files:**
- Modify: `crates/s3-gallery-core/src/util/db_helpers.rs`
- Modify: `crates/s3-gallery-core/src/view/stat.rs`
- Modify: `crates/s3-gallery-core/src/view/timeline_gallery.rs`

- [ ] **Step 1: Add fetch_scalar_opt to db_helpers.rs**

Add after the `maybe_host_id` function:

```rust
/// Execute a scalar query with an optional host_id binding.
///
/// If `host_id` is `Some`, the query is executed with `host_id` bound to the
/// first `?` parameter. If `None`, the query is executed as-is.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the query fails.
pub async fn fetch_scalar_opt<T>(
    db: &SqlitePool,
    sql: &str,
    host_id: Option<&str>,
) -> Result<T>
where
    T: sqlx::Decode<'_, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    if let Some(hid) = host_id {
        sqlx::query_scalar(sql)
            .bind(hid)
            .fetch_one(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    } else {
        sqlx::query_scalar(sql)
            .fetch_one(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    }
}
```

- [ ] **Step 2: Run test to verify it compiles**

Run: `cargo check -p s3-gallery-core`
Expected: Build succeeds

- [ ] **Step 3: Update stat.rs — replace 5 scalar branches**

In `crates/s3-gallery-core/src/view/stat.rs`, replace the 5 `if let Some(hid) = host_id` scalar query branches with `fetch_scalar_opt`:

**Replace total_files (lines 41-52):**
```rust
// Before:
let total_files: i64 = if let Some(hid) = host_id {
    sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0")
        .bind(hid)
        .fetch_one(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
} else {
    sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE is_deleted = 0")
        .fetch_one(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
};

// After:
let total_files: i64 = fetch_scalar_opt(
    db,
    "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0",
    host_id,
).await?;
```

**Replace total_size (lines 54-65):**
```rust
// Before:
let total_size: Option<i64> = if let Some(hid) = host_id {
    sqlx::query_scalar("SELECT SUM(size) FROM files WHERE host_id = ? AND is_deleted = 0")
        .bind(hid)
        .fetch_one(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
} else {
    sqlx::query_scalar("SELECT SUM(size) FROM files WHERE is_deleted = 0")
        .fetch_one(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
};

// After:
let total_size: Option<i64> = fetch_scalar_opt(
    db,
    "SELECT SUM(size) FROM files WHERE host_id = ? AND is_deleted = 0",
    host_id,
).await?;
```

**Replace deleted_files (lines 67-78):**
```rust
let deleted_files: i64 = fetch_scalar_opt(
    db,
    "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 1",
    host_id,
).await?;
```

**Replace metadata_extracted (lines 80-95):**
```rust
let metadata_extracted: i64 = fetch_scalar_opt(
    db,
    "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0 AND metadata_state = 'extracted'",
    host_id,
).await?;
```

**Replace metadata_pending (lines 97-112):**
```rust
let metadata_pending: i64 = fetch_scalar_opt(
    db,
    "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0 AND metadata_state = 'pending'",
    host_id,
).await?;
```

- [ ] **Step 4: Update timeline_gallery.rs — replace 1 FileEntry query**

In `crates/s3-gallery-core/src/view/timeline_gallery.rs`, replace the non-tag FileEntry query (lines 64-82):

```rust
// Before:
} else {
    if let Some(hid) = host_id {
        sqlx::query_as(
            "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
             ORDER BY effective_date DESC, last_modified DESC",
        )
        .bind(hid)
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        sqlx::query_as(
            "SELECT * FROM files WHERE is_deleted = 0 \
             ORDER BY effective_date DESC, last_modified DESC",
        )
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    }
};

// After:
} else {
    fetch_all_opt::<FileEntry>(
        db,
        "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
         ORDER BY effective_date DESC, last_modified DESC",
        host_id,
    ).await?
};
```

Add the import at the top of timeline_gallery.rs if not already present:
```rust
use crate::util::db_helpers::fetch_all_opt;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p s3-gallery-core -- stat duplicates timeline_gallery --nocapture`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/util/db_helpers.rs crates/s3-gallery-core/src/view/stat.rs crates/s3-gallery-core/src/view/timeline_gallery.rs
git commit -m "refactor: add fetch_scalar_opt and complete Option<host_id> branch elimination in views"
```

---

### Task 4: Refactor AppState — remove s3_clients, add s3_with_traffic()

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs`
- Modify: `crates/s3-gallery-cli/src/web/state.rs`

- [ ] **Step 1: Update cmd_serve.rs — build s3_stack without TrafficLayer**

In `crates/s3-gallery-cli/src/cmd_serve.rs`, change the s3_stack construction:

```rust
// Before (lines 128-130):
// Apply layers: LogLayer wraps TrafficLayer, both output S3Service
let s3_stack = LogLayer.layer(
    TrafficLayer::new(recorder.clone(), "serve", "s3_api").layer(core_s3),
);

// After:
// Apply LogLayer only — TrafficLayer is added per-handler via s3_with_traffic()
let s3_stack = LogLayer.layer(core_s3);
```

Also remove the `use s3_gallery_core::s3::layers::TrafficLayer;` import if it's no longer needed (check if TrafficLayer is used elsewhere in the file).

- [ ] **Step 2: Update web/state.rs — remove s3_clients, add s3_with_traffic()**

Replace the `AppState` struct and its methods:

```rust
// Before:
use std::collections::HashMap;
use std::sync::Arc;

use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub db: SqlitePool,
    pub hosts: Vec<HostConfigEntry>,
    /// S3 clients keyed by endpoint URL (endpoint "" = CLI default).
    pub s3_clients: HashMap<String, Arc<dyn S3Client>>,
    /// Tower-composed S3 service stack.
    pub s3_stack: S3Service,
    /// Optional traffic recorder.
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
    // ... other fields ...
}

impl AppState {
    pub fn get_s3_client(&self, host: &HostConfigEntry) -> Option<&Arc<dyn S3Client>> {
        let endpoint = self.effective_endpoint(host);
        self.s3_clients.get(endpoint)
    }
    // ... other methods ...
}

// After:
use std::sync::Arc;

use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::s3::layers::TrafficLayer;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use sqlx::SqlitePool;
use tower::ServiceBuilder;

#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub db: SqlitePool,
    pub hosts: Vec<HostConfigEntry>,
    /// Tower-composed S3 service stack (LogLayer only).
    pub s3_stack: S3Service,
    /// Optional traffic recorder.
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
    // ... other fields (<s3_clients> removed) ...
}

impl AppState {
    /// Get an S3Service with per-business traffic recording.
    pub fn s3_with_traffic(&self, host_id: &str, business: &str) -> S3Service {
        if let Some(ref recorder) = self.traffic_recorder {
            ServiceBuilder::new()
                .layer(TrafficLayer::new(recorder.clone(), host_id, business))
                .service(self.s3_stack.clone())
        } else {
            self.s3_stack.clone()
        }
    }
    // ... other methods (remove get_s3_client) ...
}
```

- [ ] **Step 3: Update cmd_serve.rs — remove s3_clients from AppState construction**

In `crates/s3-gallery-cli/src/cmd_serve.rs`, remove the `s3_clients` HashMap from the AppState builder:

```rust
// Before (around line 148):
AppState {
    templates: Arc::new(env),
    db: pool.clone(),
    hosts,
    s3_clients,
    s3_stack,
    traffic_recorder: Some(recorder),
    prefix: cli.prefix.clone(),
    cli_endpoint: cli.endpoint.clone(),
    cli_region: cli.region.clone(),
    access_key: cli.access_key.clone(),
    secret_key: cli.secret_key.clone(),
}

// After:
AppState {
    templates: Arc::new(env),
    db: pool.clone(),
    hosts,
    s3_stack,
    traffic_recorder: Some(recorder),
    prefix: cli.prefix.clone(),
    cli_endpoint: cli.endpoint.clone(),
    cli_region: cli.region.clone(),
    access_key: cli.access_key.clone(),
    secret_key: cli.secret_key.clone(),
}
```

Also remove the `s3_clients` variable and its construction code (lines 86-116) if it's no longer needed. The `s3_clients` HashMap was used to build `core_s3` — since we still need `core_s3`, extract the first client creation directly:

```rust
// Before (lines 86-120):
let mut s3_clients: HashMap<String, Arc<dyn S3Client>> = HashMap::new();
for host in &hosts {
    let endpoint = if host.endpoint.is_empty() { &cli.endpoint } else { &host.endpoint };
    let region = if host.region.is_empty() { &cli.region } else { &host.region };
    if !s3_clients.contains_key(endpoint) {
        let config = OssConfig::validate(...)?;
        let client = RealS3Client::from_config(&config);
        s3_clients.insert(endpoint.clone(), Arc::new(client) as Arc<dyn S3Client>);
    }
}

let core_s3 = S3Service::new(
    s3_clients.values().next().cloned()
        .ok_or_else(|| S3GalleryError::Internal("no S3 clients available".to_string()))?,
);

// After:
let first_host = hosts.first().ok_or_else(|| {
    S3GalleryError::Internal("no hosts available".to_string())
})?;
let endpoint = if first_host.endpoint.is_empty() { &cli.endpoint } else { &first_host.endpoint };
let region = if first_host.region.is_empty() { &cli.region } else { &first_host.region };
let config = OssConfig::validate(
    BucketName::new("placeholder")
        .map_err(|_| S3GalleryError::Internal("invalid placeholder".to_string()))?,
    endpoint,
    region,
    &cli.access_key,
    &cli.secret_key,
    10,
)?;
let core_s3 = S3Service::new(Arc::new(RealS3Client::from_config(&config)));
```

- [ ] **Step 4: Build and test**

Run: `cargo check -p s3-gallery-cli`
Expected: Build succeeds

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_serve.rs crates/s3-gallery-cli/src/web/state.rs
git commit -m "refactor: remove s3_clients from AppState, add s3_with_traffic() method"
```

---

### Task 5: Update web handlers to use s3_with_traffic()

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/download.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs`

- [ ] **Step 1: Update download.rs**

Replace the S3Service construction block (lines 137-159):

```rust
// Before:
let raw_client = match state.get_s3_client(host) {
    Some(c) => c.clone(),
    None => {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "application/json")],
            format!("{{\"error\":\"no S3 client\",\"detail\":\"No S3 client for host: {host_id}\"}}").into_bytes(),
        ).into_response();
    }
};

let mut s3 = if let Some(ref recorder) = state.traffic_recorder {
    let core = S3Service::new(raw_client);
    ServiceBuilder::new()
        .layer(TrafficLayer::new(recorder.clone(), host_id, "web_download"))
        .service(core)
} else {
    S3Service::new(raw_client)
};

// After:
let mut s3 = state.s3_with_traffic(host_id, "web_download");
```

Remove the now-unused imports:
- `use s3_gallery_core::s3::layers::TrafficLayer;`
- `use s3_gallery_core::s3::s3_service::S3Service;`
- `use tower::ServiceBuilder;`

- [ ] **Step 2: Update thumbnail.rs**

Same change — replace the S3Service construction block (lines 125-144):

```rust
// Before:
let raw_client = match state.get_s3_client(host) {
    Some(c) => c.clone(),
    None => {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "application/json")],
            format!("{{\"error\":\"no S3 client\",\"detail\":\"{host_id}\"}}").into_bytes(),
        ).into_response();
    }
};

let mut s3 = if let Some(ref recorder) = state.traffic_recorder {
    let core = S3Service::new(raw_client);
    ServiceBuilder::new()
        .layer(TrafficLayer::new(recorder.clone(), host_id, "web_thumbnail"))
        .service(core)
} else {
    S3Service::new(raw_client)
};

// After:
let mut s3 = state.s3_with_traffic(host_id, "web_thumbnail");
```

Also remove the unused imports.

- [ ] **Step 3: Build and test**

Run: `cargo check -p s3-gallery-cli`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/download.rs crates/s3-gallery-cli/src/web/handlers/thumbnail.rs
git commit -m "refactor: web handlers use state.s3_with_traffic() instead of raw S3Client"
```

---

### Task 6: Update cmd_init.rs to use S3Service

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_init.rs`

- [ ] **Step 1: Replace Arc<dyn S3Client> with S3Service**

```rust
// Before:
use std::sync::Arc;
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::real::RealS3Client;

let s3 = Arc::new(RealS3Client::from_config(&config)) as Arc<dyn S3Client>;
s3.put_object(&bucket, config_key, &json_bytes).await?;

// After:
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;

let mut s3 = S3Service::new(Arc::new(RealS3Client::from_config(&config)));
s3.put_object(&bucket, &config_key, &json_bytes).await?;
```

Note: `S3Service::put_object` takes `&ObjectKey` (not `&ObjectKey` directly, but the convenience method takes `&ObjectKey`). `config_key` is already a `ObjectKey` (from `host.config_path()`), so it works.

Also remove the unused `use std::sync::Arc;` and `use s3_gallery_core::s3::client::S3Client;` imports.

- [ ] **Step 2: Build and test**

Run: `cargo check -p s3-gallery-cli`
Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_init.rs
git commit -m "refactor: cmd_init uses S3Service instead of Arc<dyn S3Client>"
```

---

### Task 7: Update cmd_db.rs to use S3Service

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_db.rs`

- [ ] **Step 1: Replace Arc<dyn S3Client> with S3Service**

```rust
// Before:
use std::sync::Arc;
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::lock::check_lock;
use s3_gallery_core::s3::real::RealS3Client;

async fn create_s3_client(cli: &Cli) -> Result<Arc<dyn S3Client>> {
    // ...
    let client = RealS3Client::from_config(&config);
    Ok(Arc::new(client))
}

// Usage:
let s3 = create_s3_client(cli).await?;
match s3.get_object(&bucket, db_key).await { ... }
s3.put_object(&bucket, db_key, &data).await?;
s3.delete_object(&bucket, host_id.lock_path()).await?;
let locked = check_lock(s3.as_ref(), &bucket, lock_key).await?;

// After:
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;

async fn create_s3_client(cli: &Cli) -> Result<S3Service> {
    // ...
    let client = RealS3Client::from_config(&config);
    Ok(S3Service::new(Arc::new(client)))
}

// Usage:
let mut s3 = create_s3_client(cli).await?;
match s3.get_object(&bucket, &db_key).await { ... }
s3.put_object(&bucket, &db_key, &data).await?;
s3.delete_object(&bucket, &host_id.lock_path()).await?;
// Replace check_lock with direct object_exists call:
let locked = s3.object_exists(&bucket, &lock_key).await?;
```

Note: `S3Service::get_object`, `put_object`, `delete_object`, `object_exists` take `&mut self` and `&ObjectKey` (not `&ObjectKey` by reference — the convenience methods take `&ObjectKey`). The `db_key` and `lock_key` are already `ObjectKey` types.

Also remove unused imports: `use std::sync::Arc;`, `use s3_gallery_core::s3::client::S3Client;`, `use s3_gallery_core::s3::lock::check_lock;`.

- [ ] **Step 2: Build and test**

Run: `cargo check -p s3-gallery-cli`
Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_db.rs
git commit -m "refactor: cmd_db uses S3Service instead of Arc<dyn S3Client>"
```

---

### Task 8: Update cmd_scan.rs to use S3Service for DB operations

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`

- [ ] **Step 1: Replace Arc<dyn S3Client> with S3Service for DB push/pull**

The pipeline already uses S3Service (via `discover_s3`). The DB push/pull at the end uses `s3: Arc<dyn S3Client>` directly. Change the function signature and usage:

```rust
// Before:
async fn run_scan_core(
    s3: Arc<dyn S3Client>,
    pool: &SqlitePool,
    bucket: &BucketName,
    scope_prefix: &str,
    opts: &ScanOptions,
    cli: &Cli,
) -> Result<()> {
    // ... pipeline ...
    s3.put_object(bucket, &db_key, &db_data).await?;
}

// After:
async fn run_scan_core(
    s3: Arc<dyn S3Client>,  // Keep for pipeline, but change DB push to use S3Service
    pool: &SqlitePool,
    bucket: &BucketName,
    scope_prefix: &str,
    opts: &ScanOptions,
    cli: &Cli,
) -> Result<()> {
    // ... pipeline (uses s3 for discover_s3) ...

    // DB push/pull — use S3Service instead of Arc<dyn S3Client>
    // Build a minimal S3Service for the DB push
    let mut s3_service = S3Service::new(s3.clone());
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
    let db_data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
    s3_service.put_object(bucket, &db_key, &db_data).await?;
}
```

Wait, actually, the `s3` parameter is used both for the pipeline (passed to `S3Service::new(s3.clone())` for discover_s3) and for DB push/pull. So we can't fully remove the `Arc<dyn S3Client>` parameter — the pipeline still needs it to construct S3Service instances.

Actually, looking at the code more carefully, `setup_scan_common` returns `Arc<dyn S3Client>`, and this is used to:
1. Build `discover_s3` and `exif_s3` (both use `S3Service::new(s3.clone())`)
2. DB pull: `s3.get_object(&bucket, &db_key).await`
3. DB push: `s3.put_object(bucket, &db_key, &db_data).await?`

The minimum change is to wrap the DB operations with S3Service:

```rust
// Before (line 217):
s3.put_object(bucket, &db_key, &db_data).await?;

// After:
let mut s3_svc = S3Service::new(s3.clone());
s3_svc.put_object(bucket, &db_key, &db_data).await?;
```

And for the DB pull (line 109):
```rust
// Before:
match s3.get_object(&bucket, &db_key).await {
// After:
let mut s3_svc = S3Service::new(s3.clone());
match s3_svc.get_object(&bucket, &db_key).await {
```

But this is a bit ugly — creating a new S3Service wrapper just for one call. A cleaner approach is to change `setup_scan_common` to return `S3Service` instead of `Arc<dyn S3Client>`, since the `Arc<dyn S3Client>` is always wrapped in `S3Service` anyway.

Let me think about this more carefully...

Actually, `setup_scan_common` returns `Arc<dyn S3Client>` and it's used to create both `discover_s3` and `exif_s3` via `S3Service::new(s3.clone())`. If we change it to return `S3Service`, we'd need to call `into_inner()` to get the `Arc<dyn S3Client>` back, which is ugly.

The simplest approach that satisfies the "no direct S3Client" rule: wrap the raw client in S3Service for the DB operations. Since the raw client is already inside the pipeline's S3Service instances, it's fine:

```rust
// At the end of run_scan_core, after the pipeline:
let mut db_s3 = S3Service::new(s3.clone());
let db_key = ObjectKey::new("s3-gallery.db".to_string())
    .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
let db_data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
db_s3.put_object(bucket, &db_key, &db_data).await?;
```

- [ ] **Step 2: Build and test**

Run: `cargo check -p s3-gallery-cli`
Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "refactor: cmd_scan DB operations use S3Service instead of raw S3Client"
```

---

### Task 9: Delete dead code — view/remote.rs

**Files:**
- Delete: `crates/s3-gallery-core/src/view/remote.rs`
- Modify: `crates/s3-gallery-core/src/view/mod.rs`

- [ ] **Step 1: Remove remote reference from mod.rs**

In `crates/s3-gallery-core/src/view/mod.rs`, remove the line:
```rust
pub use remote::RemoteView;
```

- [ ] **Step 2: Delete remote.rs**

```bash
rm crates/s3-gallery-core/src/view/remote.rs
```

- [ ] **Step 3: Build and test**

Run: `cargo check -p s3-gallery-core`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-core/src/view/remote.rs crates/s3-gallery-core/src/view/mod.rs
git commit -m "refactor: remove dead code RemoteView (unused)"
```

---

### Task 10: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p s3-gallery-core`
Expected: All tests pass

Run: `cargo test -p s3-gallery-cli`
Expected: All tests pass

- [ ] **Step 2: Full build check**

Run: `cargo check`
Expected: Build succeeds for all crates