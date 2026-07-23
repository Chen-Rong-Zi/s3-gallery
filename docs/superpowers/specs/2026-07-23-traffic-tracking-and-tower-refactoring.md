# S3 Traffic Tracking & Tower Layer Refactoring

> **For agentic workers:** Implementation plan to be created via `superpowers:writing-plans` after this spec is approved.

**Goal:** Track S3 traffic (download/upload bytes) for cost analysis, with per-business-layer granularity and a dashboard. Refactor six subsystems — S3 service, scan pipeline, web handlers, thumbnail service, view layer, and traffic tracking — to use Tower's composable patterns.

**Architecture:** Six subsystems refactored with Tower, each at the appropriate level of abstraction — `tower::Service` for S3, Axum middleware for web, `LayerFn`/function composition for scan and thumbnails, and common pattern extraction for views.

**Tech Stack:** Rust, `tower` (Service, Layer, LayerFn), `tokio`, `sqlx`/SQLite, Axum, `minijinja` templates

---

## 1. Motivation

S3 object storage charges for downstream traffic. The project downloads files for EXIF extraction, thumbnail generation, and user downloads. Without traffic tracking, users have no visibility into which operations, files, or hosts consume the most bandwidth.

Beyond traffic tracking, the codebase has several areas with repetitive boilerplate and tightly coupled cross-cutting concerns that can be extracted and composed using Tower's patterns:

- **S3 stack:** `LoggedS3Client` has 340 lines of manual decorator boilerplate
- **Web handlers:** 11 handlers each have 15 lines of identical `render_template()` + error handling
- **Scan pipeline:** Lock acquisition, concurrency control, and logging are mixed with business logic
- **Thumbnail service:** Cache-aside pattern is manually implemented
- **View layer:** `Option<host_id>` branching is duplicated across every view function

---

## 2. Tower Layer Architecture

### 2.1 Approach Overview

Each subsystem uses a different Tower mechanism:

| Subsystem | Mechanism | Why |
|-----------|-----------|-----|
| S3Service | `tower::Service` | Need to reuse Tower's built-in `TimeoutLayer`, `ConcurrencyLimitLayer`, `BufferLayer` |
| Web handlers | Axum middleware | Already Tower-based, use `axum::middleware::from_fn` |
| Scan pipeline | `LayerFn` + function composition | Extract cross-cutting concerns without changing core logic |
| Thumbnail | `LayerFn` / Builder | Cache/generation/watermark decoupling |
| View layer | Common pattern extraction | Eliminate `Option<host_id>` duplication |

### 2.2 S3Service — `tower::Service` Implementation

```rust
use tower::Service;

/// Unified S3 request type
pub enum S3Request {
    GetObject(BucketName, ObjectKey),
    GetObjectRange(BucketName, ObjectKey, u64, u64),
    ListObjects(BucketName, ObjectKey),
    HeadObject(BucketName, ObjectKey),
    PutObject(BucketName, ObjectKey, Vec<u8>),
    PutObjectIfNoneMatch(BucketName, ObjectKey, Vec<u8>),
    DeleteObject(BucketName, ObjectKey),
    ObjectExists(BucketName, ObjectKey),
}

/// Unified S3 response type
pub enum S3Response {
    GetObject(Vec<u8>),
    GetObjectRange(Vec<u8>),
    ListObjects(Vec<ObjectSummary>),
    HeadObject(ObjectMetadata),
    PutObject(()),
    PutObjectIfNoneMatch(bool),
    DeleteObject(()),
    ObjectExists(bool),
}

/// S3Service wraps Arc<dyn S3Client> and implements tower::Service
#[derive(Clone)]
pub struct S3Service {
    inner: Arc<dyn S3Client>,
}

impl S3Service {
    pub fn new(inner: Arc<dyn S3Client>) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> Arc<dyn S3Client> {
        self.inner
    }
}

impl Service<S3Request> for S3Service {
    type Response = S3Response;
    type Error = S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: S3Request) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let result = match req {
                S3Request::GetObject(b, k) => inner.get_object(&b, &k).await.map(S3Response::GetObject),
                S3Request::GetObjectRange(b, k, s, e) => inner.get_object_range(&b, &k, s, e).await.map(S3Response::GetObjectRange),
                S3Request::ListObjects(b, p) => inner.list_objects(&b, &p).await.map(S3Response::ListObjects),
                S3Request::HeadObject(b, k) => inner.head_object(&b, &k).await.map(S3Response::HeadObject),
                S3Request::PutObject(b, k, body) => inner.put_object(&b, &k, &body).await.map(S3Response::PutObject),
                S3Request::PutObjectIfNoneMatch(b, k, body) => inner.put_object_if_none_match(&b, &k, &body).await.map(S3Response::PutObjectIfNoneMatch),
                S3Request::DeleteObject(b, k) => inner.delete_object(&b, &k).await.map(S3Response::DeleteObject),
                S3Request::ObjectExists(b, k) => inner.object_exists(&b, &k).await.map(S3Response::ObjectExists),
            };
            result.map_err(|e| e)  // S3Client returns S3GalleryError, same as Service::Error
        })
    }
}
```

### 2.3 Layer Implementations

Each layer implements `tower::Layer<S3Service, Service = S3Service>`:

```rust
use tower::layer::Layer;

/// LogLayer wraps S3Service with logging. Must be placed inside the Tower
/// stack because it needs access to the inner Arc<dyn S3Client>:
///   BufferLayer(TimeoutLayer(LogLayer(S3Service)))
pub struct LogLayer;
impl Layer<S3Service> for LogLayer {
    type Service = S3Service;
    fn layer(&self, inner: S3Service) -> Self::Service {
        S3Service::new(Arc::new(LoggingS3Client::new(inner.into_inner())))
    }
}

/// TrafficLayer also wraps S3Service directly — it needs the inner
/// S3Client to record per-operation traffic via BusinessS3Client.
/// Place it at the same level as LogLayer:
///   BufferLayer(TimeoutLayer(TrafficLayer(LogLayer(S3Service))))
pub struct TrafficLayer {
    recorder: Arc<TrafficRecorder>,
    host_id: String,
    business: String,
}
impl Layer<S3Service> for TrafficLayer {
    type Service = S3Service;
    fn layer(&self, inner: S3Service) -> Self::Service {
        S3Service::new(Arc::new(BusinessS3Client::new(inner.into_inner(), &self.host_id, &self.business, self.recorder.clone())))
    }
}

impl TrafficLayer {
    pub fn new(recorder: Arc<TrafficRecorder>, host_id: &str, business: &str) -> Self {
        Self { recorder, host_id: host_id.to_string(), business: business.to_string() }
    }
}
```

### 2.4 Composition

```rust
use tower::ServiceBuilder;
use tower::buffer::BufferLayer;
use tower::timeout::TimeoutLayer;
use tower::limit::ConcurrencyLimitLayer;

// Note: LogLayer and TrafficLayer wrap S3Service directly (they need access
// to the inner Arc<dyn S3Client>). Tower built-in layers (BufferLayer,
// TimeoutLayer, ConcurrencyLimitLayer) wrap any Service<S3Request> and
// are placed outside.

// Step 1: Build the inner stack with LogLayer + TrafficLayer (both Layer<S3Service>)
let core = S3Service::new(Arc::new(RealS3Client::from_config(&config)));
let inner = ServiceBuilder::new()
    .layer(TrafficLayer::new(recorder, host_id, "s3_api"))  // 协议层统计
    .layer(LogLayer)                                          // 日志
    .service(core);

// Step 2: Wrap with Tower built-in layers (they operate on any Service)
let stack = ServiceBuilder::new()
    .layer(BufferLayer::new(1024))                    // Tower — 反压
    .layer(TimeoutLayer::new(Duration::from_secs(30))) // Tower — 超时
    .layer(ConcurrencyLimitLayer::new(10))             // Tower — 并发控制
    .service(inner);

// Per-business stacks: need a separate S3Service + TrafficLayer chain
// because TrafficLayer carries a business label.
// These are placed BEFORE the Tower stack so traffic is recorded at the
// S3Client level, then the request passes through the Tower layers.
let exif_s3 = ServiceBuilder::new()
    .layer(BufferLayer::new(1024))
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(ConcurrencyLimitLayer::new(10))
    .layer(LogLayer)
    .service(
        ServiceBuilder::new()
            .layer(TrafficLayer::new(recorder, host_id, "exif_extraction"))
            .service(S3Service::new(Arc::new(RealS3Client::from_config(&config))))
    );

let dl_s3 = ServiceBuilder::new()
    .layer(BufferLayer::new(1024))
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(ConcurrencyLimitLayer::new(10))
    .layer(LogLayer)
    .service(
        ServiceBuilder::new()
            .layer(TrafficLayer::new(recorder, host_id, "web_download"))
            .service(S3Service::new(Arc::new(RealS3Client::from_config(&config))))
    );
```

### 2.5 S3Client Trait Compatibility

The `S3Client` trait is kept as a convenience interface. `S3Service` provides helper methods:

```rust
impl S3Service {
    pub async fn get_object(&mut self, bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>> {
        self.call(S3Request::GetObject(bucket.clone(), key.clone()))
            .await?
            .try_into_get_object()
    }
    // ... same for all 8 operations
}
```

This means existing code using `client.get_object(&b, &k).await` can keep working — only the construction site changes.

---

## 3. Traffic Tracking

### 3.1 TrafficRecord & TrafficCounters

```rust
#[derive(Debug, Clone)]
pub struct TrafficRecord {
    pub host_id: String,
    /// Empty for list/head/delete operations that don't target a single file.
    pub file_key: String,
    pub business: String,
    pub operation: S3Operation,
    pub direction: String,
    pub bytes: u64,
    pub count: u64,
}

/// S3Client operations — used as index into `per_operation` array.
/// Must match the order of S3Request variants.
#[repr(usize)]
pub enum S3Operation {
    GetObject,            // 0
    GetObjectRange,       // 1
    PutObject,            // 2
    PutObjectIfNoneMatch, // 3
    ListObjects,          // 4
    HeadObject,           // 5
    DeleteObject,         // 6
    ObjectExists,         // 7
}

impl S3Operation {
    pub fn from_request(req: &S3Request) -> Self {
        match req {
            S3Request::GetObject(..) => Self::GetObject,
            S3Request::GetObjectRange(..) => Self::GetObjectRange,
            S3Request::PutObject(..) => Self::PutObject,
            S3Request::PutObjectIfNoneMatch(..) => Self::PutObjectIfNoneMatch,
            S3Request::ListObjects(..) => Self::ListObjects,
            S3Request::HeadObject(..) => Self::HeadObject,
            S3Request::DeleteObject(..) => Self::DeleteObject,
            S3Request::ObjectExists(..) => Self::ObjectExists,
        }
    }
}

pub struct TrafficCounters {
    pub download_bytes: AtomicU64,
    pub upload_bytes: AtomicU64,
    pub request_count: AtomicU64,
    pub per_operation: [AtomicU64; 8],  // one per S3Operation variant
}

impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            download_bytes: AtomicU64::new(0),
            upload_bytes: AtomicU64::new(0),
            request_count: AtomicU64::new(0),
            per_operation: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}
```

### 3.2 TrafficRecorder

```rust
pub struct TrafficRecorder {
    counters: Arc<TrafficCounters>,
    tx: mpsc::Sender<TrafficRecord>,
}

impl TrafficRecorder {
    pub fn new(pool: SqlitePool) -> Self {
        let counters = Arc::new(TrafficCounters::new());
        let (tx, rx) = mpsc::channel(10_000);
        let bg = counters.clone();
        tokio::spawn(Self::background_aggregator(rx, pool, bg));
        Self { counters, tx }
    }

    pub fn record(&self, record: TrafficRecord) {
        self.counters.record(&record);
        let _ = self.tx.try_send(record);
    }
}
```

### 3.3 BusinessS3Client

```rust
pub struct BusinessS3Client {
    inner: Arc<dyn S3Client>,
    recorder: Arc<TrafficRecorder>,
    host_id: String,
    business: String,
}

impl BusinessS3Client {
    pub fn new(
        inner: Arc<dyn S3Client>,
        host_id: &str,
        business: &str,
        recorder: Arc<TrafficRecorder>,
    ) -> Self { ... }
}

impl S3Client for BusinessS3Client {
    // All 8 methods: delegate to inner, on success call recorder.record(...)
}
```

### 3.4 Background Aggregator

A tokio task reads from the `mpsc` channel every 60 seconds, batches records, and writes to `traffic_log`, `traffic_file_log`, and `traffic_stats` tables.

### 3.5 Traffic Config

```rust
pub struct TrafficConfig {
    pub enabled: bool,
    pub aggregation_interval_secs: u64,  // default 60
}
```

Default to `enabled: true`. When disabled, `TrafficRecorder` is not created and `traffic_recorder` is `None` everywhere. The `enabled` flag is checked once at app startup (in `main()` / `run_serve()`), not in `TrafficLayer::layer()` — if disabled, the `TrafficLayer` is simply not added to the stack. Each integration point checks `if let Some(ref rec)` before wrapping with `TrafficLayer`.

---

## 4. Database Schema

### 4.1 New Tables

```sql
CREATE TABLE IF NOT EXISTS traffic_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    business TEXT NOT NULL,
    direction TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    count INTEGER NOT NULL,
    recorded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_traffic_log_host_time ON traffic_log(host_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_traffic_log_business ON traffic_log(business, recorded_at);

CREATE TABLE IF NOT EXISTS traffic_file_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id TEXT NOT NULL,
    file_key TEXT NOT NULL,
    business TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    count INTEGER NOT NULL,
    recorded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_traffic_file_host_key ON traffic_file_log(host_id, file_key);
CREATE INDEX IF NOT EXISTS idx_traffic_file_time ON traffic_file_log(recorded_at);

CREATE TABLE IF NOT EXISTS traffic_stats (
    host_id TEXT NOT NULL,
    period TEXT NOT NULL,
    operation TEXT NOT NULL,
    business TEXT NOT NULL,
    direction TEXT NOT NULL,
    total_bytes INTEGER NOT NULL,
    total_count INTEGER NOT NULL,
    PRIMARY KEY (host_id, period, operation, business, direction)
);
```

### 4.2 Retention Policy

Traffic tables grow over time. The background aggregator periodically cleans up old data:
- `traffic_log`: retain 90 days, delete older rows
- `traffic_file_log`: retain 90 days, delete older rows
- `traffic_stats`: retain 12 months (daily rows), then roll up to monthly and delete daily

Cleanup runs once per hour as part of the background aggregator loop.

### 4.3 Migration Strategy

New tables use `CREATE TABLE IF NOT EXISTS` in `db/schema.rs`. Idempotent, safe on existing databases. Schema version remains at 1 (backward-compatible addition).

---

## 5. Scan Pipeline Refactoring

### 5.1 Current Problem

`run_scan()` in `scanner.rs` mixes cross-cutting concerns with business logic:

```rust
pub async fn run_scan(config: ScanConfig) -> Result<ScanResult> {
    // Step 1: Acquire lock ← cross-cutting
    let guard = acquire_lock(...).await?;
    // Step 2: List objects ←  business
    let all_objects = config.s3.list_objects(...).await?;
    // Step 3-8: ... business ...
    // Step 9: Release lock ← cross-cutting
    guard.release().await?;
}
```

### 5.2 Refactoring: Function Composition

Extract cross-cutting concerns into composable wrappers using `tower::layer::LayerFn`:

```rust
use tower::layer::LayerFn;

// Core scan logic (pure business, no lock/concurrency/logging)
async fn scan_core(config: ScanConfig) -> Result<ScanResult> {
    let all_objects = config.s3.list_objects(&config.bucket, &config.prefix).await?;
    // ... steps 2-8 ...
    Ok(ScanResult { ... })
}

// Lock layer
fn with_lock<F, Fut>(f: F) -> impl FnOnce(ScanConfig) -> Fut
where
    F: Fn(ScanConfig) -> Fut,
    Fut: Future<Output = Result<ScanResult>>,
{
    |config: ScanConfig| async move {
        let guard = acquire_lock(config.s3.clone(), ...).await?;
        let result = f(config).await;
        guard.release().await?;
        result
    }
}

// Concurrency layer
fn with_concurrency<F, Fut>(max: usize, f: F) -> impl FnOnce(ScanConfig) -> Fut
where
    F: Fn(ScanConfig) -> Fut,
    Fut: Future<Output = Result<ScanResult>>,
{
    |config: ScanConfig| async move {
        let limiter = ConcurrencyLimiter::new(max);
        f(config).await
    }
}

// Compose
let scan = with_lock(with_concurrency(10, scan_core));
let result = scan(config).await?;
```

### 5.3 Files Changed

- `scan/scanner.rs`: Extract `scan_core()`, add `with_lock()`, `with_concurrency()` wrappers
- `s3/lock.rs`: No change needed (already a function)
- `util/concurrency.rs`: Keep `ConcurrencyLimiter` as is, or optionally remove if `ConcurrencyLimitLayer` on S3Service replaces it

---

## 6. Web Handlers Refactoring

### 6.1 Current Problem

11 handlers each have identical `render_template()` boilerplate (~15 lines each = 165 lines total):

```rust
// Repeated in every handler
fn render_template(state: &AppState, template_name: &str, context: &Value) -> Result<Html<String>, Box<Response>> {
    let template = state.templates.get_template(template_name).map_err(|e| {
        Box::new((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({...}))).into_response())
    })?;
    let html = template.render(context).map_err(|e| {
        Box::new((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({...}))).into_response())
    })?;
    Ok(Html(html))
}
```

### 6.2 Refactoring: Axum Middleware

Replace per-handler boilerplate with a shared middleware:

```rust
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use http::Request;

// Unified template renderer as middleware
async fn template_middleware<B>(
    request: Request<B>,
    next: Next<B>,
) -> Response {
    let response = next.run(request).await;
    // If the handler returned a TemplateResult, render it
    response
}

// Handler return type
pub enum HandlerResult {
    Html(String),
    Json(serde_json::Value),
    Redirect(String),
    Error(StatusCode, serde_json::Value),
}

impl IntoResponse for HandlerResult {
    fn into_response(self) -> Response {
        match self {
            HandlerResult::Html(html) => Html(html).into_response(),
            HandlerResult::Json(json) => Json(json).into_response(),
            HandlerResult::Redirect(url) => Redirect::to(&url).into_response(),
            HandlerResult::Error(status, json) => (status, Json(json)).into_response(),
        }
    }
}

// Shared template renderer
pub fn render_template(state: &AppState, template_name: &str, context: &Value) -> HandlerResult {
    match state.templates.get_template(template_name) {
        Ok(tmpl) => match tmpl.render(context) {
            Ok(html) => HandlerResult::Html(html),
            Err(e) => HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "template rendering failed", "detail": e.to_string()})
            ),
        },
        Err(e) => HandlerResult::Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "template not found", "detail": e.to_string()})
        ),
    }
}
```

### 6.3 Handler Simplification

**Before (gallery.rs):**
```rust
pub async fn gallery(State(state): State<AppState>, ...) -> impl IntoResponse {
    // ... logic ...
    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}
```

**After:**
```rust
pub async fn gallery(State(state): State<AppState>, ...) -> HandlerResult {
    // ... logic ...
    render_template(&state, template_name, &context)
}
```

### 6.4 Files Changed

- `web/handlers/mod.rs`: Add `render_template` shared function, `HandlerResult` enum
- `web/handlers/browse.rs`: Use `HandlerResult` instead of `impl IntoResponse`
- `web/handlers/gallery.rs`: Same
- `web/handlers/search.rs`: Same
- `web/handlers/tags.rs`: Same
- `web/handlers/duplicates.rs`: Same
- `web/handlers/stats.rs`: Same
- `web/handlers/file_detail.rs`: Same
- `web/handlers/settings.rs`: Same
- `web/handlers/dashboard.rs`: Same
- `web/handlers/download.rs`: Same (already has non-template responses)
- `web/handlers/thumbnail.rs`: Same

---

## 7. View Layer Refactoring

### 7.1 Current Problem

Every view function has `Option<host_id>` branching, duplicating the entire SQL query:

```rust
// Repeated in stat.rs, duplicates.rs, files.rs, etc.
let result = if let Some(hid) = host_id {
    sqlx::query_as("SELECT ... WHERE host_id = ? AND ...")
        .bind(hid).fetch_all(db).await
} else {
    sqlx::query_as("SELECT ... WHERE ...")
        .fetch_all(db).await
};
```

### 7.2 Refactoring: Extract Common Query Patterns

```rust
/// Helper: append WHERE host_id = ? conditionally
pub fn maybe_host_id(host_id: Option<&str>, sql: &str) -> (String, Vec<String>) {
    if let Some(hid) = host_id {
        (format!("{} AND host_id = ?", sql), vec![hid.to_string()])
    } else {
        (sql.to_string(), vec![])
    }
}

/// Helper: execute a query with optional host_id binding
pub async fn fetch_all_opt<T>(
    db: &SqlitePool,
    sql: &str,
    host_id: Option<&str>,
) -> Result<Vec<T>>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
{
    if let Some(hid) = host_id {
        sqlx::query_as::<_, T>(sql)
            .bind(hid)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    } else {
        sqlx::query_as::<_, T>(sql)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    }
}
```

### 7.3 Affected View Files

| File | Current Pattern | Refactoring |
|------|----------------|-------------|
| `view/stat.rs` | 6 `if let Some(hid)` branches | Use `fetch_all_opt` |
| `view/duplicates.rs` | 2 `if let Some(hid)` branches | Use `fetch_all_opt` |
| `view/search.rs` | Simple, no branching | Minor cleanup |
| `view/ls.rs` | No branching | No change |
| `view/timeline_gallery.rs` | Has `host_id` branching | Use `fetch_all_opt` |
| `view/export.rs` | Simple | No change |
| `view/tags.rs` | No branching | No change |

---

## 8. Thumbnail Service Refactoring

### 8.1 Current Problem

`ThumbnailCache::get_or_generate()` mixes cache lookup, generation, and cache storage in one method.

### 8.2 Refactoring: LayerFn Composition

```rust
use tower::layer::LayerFn;

// Core generator
async fn generate_thumbnail(data: &[u8]) -> Result<Vec<u8>> { ... }

// Cache layer
fn with_cache<F, Fut>(db: SqlitePool, max_bytes: u64, f: F) -> impl Fn(ThumbnailRequest) -> Fut
where
    F: Fn(ThumbnailRequest) -> Fut,
    Fut: Future<Output = Result<ThumbnailResponse>>,
{
    |req: ThumbnailRequest| async move {
        // Check cache first
        if let Some(cached) = ThumbnailEntry::get(&db, &req.key).await.ok() {
            return Ok(ThumbnailResponse::Cached(cached.data));
        }
        // Generate
        let result = f(req).await?;
        // Cache the result
        // ...
        result
    }
}

// Compose
let thumbnail_service = with_cache(db, MAX_CACHE_BYTES, generate_thumbnail);
let result = thumbnail_service(ThumbnailRequest { key, data }).await?;
```

### 8.3 Files Changed

- `thumbnail/generator.rs`: Extract `with_cache()`, reorganize `ThumbnailCache`

---

## 9. S3Service Integration Points

### 9.1 Scan — EXIF Extraction

```rust
// After:
let exif_s3: S3Service = if let Some(ref rec) = config.traffic_recorder {
    ServiceBuilder::new()
        .layer(TrafficLayer::new(rec.clone(), &config.host_id, "exif_extraction"))
        .service(config.s3_stack.clone())
} else {
    config.s3_stack.clone()
};
let data = exif_s3.get_object_range(&config.bucket, &obj.key, 0, 65536).await?;
```

### 9.2 Web — File Download

```rust
let recorder = &state.traffic_recorder;
let dl_s3: S3Service = if let Some(ref rec) = recorder {
    ServiceBuilder::new()
        .layer(TrafficLayer::new(rec.clone(), host_id, "web_download"))
        .service(state.s3_stack.clone())
} else {
    state.s3_stack.clone()
};
let data = dl_s3.get_object(&bucket_name, &object_key).await?;
```

### 9.3 Web — Thumbnail Generation

Same pattern with `"web_thumbnail"` business tag.

### 9.4 TrafficRecorder Wiring

```rust
// In cmd_serve.rs / cmd_scan.rs:
let recorder = Arc::new(TrafficRecorder::new(pool.clone()));

// ScanConfig
pub struct ScanConfig {
    // ... existing fields
    pub s3_stack: S3Service,
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
}

// AppState
pub struct AppState {
    // ... existing fields
    pub s3_stack: S3Service,
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
}
```

---

## 10. CLI Commands

### 10.1 `s3-gallery traffic summary`

```
s3-gallery traffic summary [--host <host_id>] [--period day|month] [--since <date>] [--until <date>]
```

Output:
```
Host: my-photos
Period: 2026-07-01 ~ 2026-07-23

Business              Download (MB)    Upload (MB)    Requests
──────────────────────────────────────────────────────────────
exif_extraction          12.3 MB         0.0 MB          567
web_download          1,200.0 MB         0.0 MB        1,200
web_thumbnail            34.5 MB         0.0 MB          345
──────────────────────────────────────────────────────────────
Total                 1,246.8 MB         0.0 MB        2,112

Top Files by Traffic:
  photos/vacation/IMG_2023.mp4      234.5 MB
  photos/vacation/IMG_2022.jpg       45.2 MB

Estimated Cost: $0.037 (at $0.03/GB download)
```

### 10.2 `s3-gallery traffic live`

```
s3-gallery traffic live [--interval <seconds>]

Live Traffic (refreshing every 2s)
Download: 1.2 MB/s    Upload: 0.0 KB/s    Requests: 12/s
Top Files: photos/vacation/img_001.jpg   45.2 MB
```

---

## 11. Web Dashboard

### 11.1 New Routes

- `GET /traffic` — renders `traffic.html` template
- `GET /api/traffic/live` — JSON for real-time counters (HTMX poll every 5s)
- `GET /api/traffic/history?period=day&host_id=xxx` — JSON for historical data

### 11.2 Dashboard Layout

```
┌──────────────┐  ┌──────────────┐
│ ↓ Download   │  │ ↑ Upload     │
│   1,234 MB   │  │     0.3 MB   │  ← 实时计数器
└──────────────┘  └──────────────┘
┌─ Per Business (today) ──────────────────────────┐
│  exif_extraction   12.3 MB    567 req           │
│  web_download    1,200.0 MB  1,200 req          │
└─────────────────────────────────────────────────┘
┌─ Top Files (today) ─────────────────────────────┐
│  photos/vacation/IMG_2023.mp4    234.5 MB       │
└─────────────────────────────────────────────────┘
┌─ Last 7 Days ───────────────────────────────────┐
│  ██▌███▌████████▌███████████████▌               │
│  07/17  07/19  07/21  07/23                     │
└─────────────────────────────────────────────────┘
```

### 11.3 Navigation

Add "Traffic" link to the navigation bar in `layout.html`.

---

## 12. Files to Create & Modify

### 12.1 Dependencies

**`s3-gallery-core/Cargo.toml`:**
```toml
tower = { version = "0.5", features = ["layer", "buffer", "timeout", "limit"] }
```

**`s3-gallery-cli/Cargo.toml`:** Already has `tower-http`. No addition needed.

### 12.2 Files to Create

| File | Purpose |
|------|---------|
| `s3/s3_service.rs` | `S3Request`, `S3Response`, `S3Service` (tower::Service) |
| `s3/layers.rs` | `LogLayer`, `TrafficLayer`, `TimeoutLayer`, `ConcurrencyLimitLayer` (custom) |
| `s3/traffic_recorder.rs` | `TrafficRecorder`, `TrafficCounters`, `TrafficRecord`, `BusinessS3Client` |
| `s3/traffic_persist.rs` | Background aggregator, DB writer, stats rollup |
| `view/traffic.rs` | Traffic query logic (summary, live, history) |
| `templates/traffic.html` | Dashboard page template |
| `util/db_helpers.rs` | `fetch_all_opt`, `maybe_host_id` query helpers |

### 12.3 Files to Modify

| File | Change |
|------|--------|
| `s3/mod.rs` | Add `pub mod s3_service; pub mod layers; pub mod traffic_recorder; pub mod traffic_persist;` |
| `s3/logged.rs` | Refactor into `LoggingS3Client` (may keep as is, `LogLayer` wraps it) |
| `scan/scanner.rs` | Extract `scan_core()`, add `with_lock()`/`with_concurrency()` wrappers, use `S3Service` |
| `scan/mod.rs` | No change |
| `web/handlers/mod.rs` | Add `HandlerResult` enum, shared `render_template()` |
| `web/handlers/browse.rs` | Use `HandlerResult` |
| `web/handlers/gallery.rs` | Use `HandlerResult` |
| `web/handlers/search.rs` | Use `HandlerResult` |
| `web/handlers/tags.rs` | Use `HandlerResult` |
| `web/handlers/duplicates.rs` | Use `HandlerResult` |
| `web/handlers/stats.rs` | Use `HandlerResult` |
| `web/handlers/file_detail.rs` | Use `HandlerResult` |
| `web/handlers/settings.rs` | Use `HandlerResult` |
| `web/handlers/dashboard.rs` | Use `HandlerResult` |
| `web/handlers/download.rs` | Use `HandlerResult`, integrate `S3Service` + `TrafficLayer` |
| `web/handlers/thumbnail.rs` | Use `HandlerResult`, integrate `S3Service` + `TrafficLayer` |
| `web/router.rs` | Add `/traffic` and `/api/traffic/*` routes |
| `web/state.rs` | Add `S3Service` and `Arc<TrafficRecorder>` |
| `db/schema.rs` | Add `traffic_log`, `traffic_file_log`, `traffic_stats` tables |
| `db/models.rs` | Add `TrafficLogEntry`, `TrafficFileLogEntry`, `TrafficStatsEntry` |
| `templates/layout.html` | Add "Traffic" nav link |
| `s3-gallery-web/src/lib.rs` | Add `traffic.html` to template list |
| `cmd_scan.rs` | Build `S3Service` stack, pass to `ScanConfig` |
| `cmd_serve.rs` | Build `S3Service` stack, pass to `AppState` |
| `view/stat.rs` | Use `fetch_all_opt` to eliminate `Option<host_id>` branching |
| `view/duplicates.rs` | Use `fetch_all_opt` |
| `view/timeline_gallery.rs` | Use `fetch_all_opt` |
| `thumbnail/generator.rs` | Extract `with_cache()`, reorganize |

---

## 13. Error Handling

- `TrafficRecorder::record()` uses `try_send` — never blocks S3 operations
- Channel full (10,000 capacity) → records silently dropped, logged at `tracing::warn!`
- DB write failures in background aggregator logged via `tracing::warn!`, never propagated
- `BusinessS3Client` only records on success; errors are not counted as traffic
- Real-time counters use `AtomicU64` with relaxed ordering
- `HandlerResult` provides a single error path for all web handlers

---

## 14. Testing

### 14.1 Unit Tests

- `S3Service` delegates all 8 operations correctly
- `BusinessS3Client` records traffic on success only
- `TrafficCounters` atomics increment correctly
- `with_lock()` / `with_concurrency()` wrappers compose correctly
- `fetch_all_opt` returns correct results with and without `host_id`
- Thumbnail `with_cache()` returns cached result on hit, generates on miss

### 14.2 Integration Tests

- Multi-layer S3 stack: `BufferLayer(TimeoutLayer(TrafficLayer(core)))`
- `traffic summary` CLI output format
- `traffic live` CLI real-time display
- Web handler with `HandlerResult` returns correct responses

### 14.3 Mocking

- `MockS3Client` (already exists) — verify `S3Service` delegation
- `MockTrafficRecorder` — `Arc<Mutex<Vec<TrafficRecord>>>` for deterministic testing
- Background aggregator tested with `mpsc::channel` + `tokio::test` + timeout

---

## 15. Self-Review Checklist

- [ ] No placeholders (TBD, TODO, etc.)
- [ ] All sections internally consistent
- [ ] Architecture matches feature descriptions
- [ ] S3Service + S3Client trait coexist without breaking existing code
- [ ] Scan pipeline wrappers don't change business logic
- [ ] Web handler refactoring is backward-compatible
- [ ] View layer helpers don't change query semantics
- [ ] DB schema has proper indexes for query patterns
- [ ] Error handling covers all failure modes
- [ ] Tests cover delegation, recording, aggregation, and CLI output
- [ ] No scope creep beyond the 6 subsystems described
- [ ] TrafficLayer is `Layer<S3Service>` (not generic) — composes correctly with S3Service
- [ ] S3Service derives Clone and implements `tower::Service` correctly
- [ ] S3Operation covers all 8 S3Request variants
- [ ] Traffic tables have retention policy (90 days)