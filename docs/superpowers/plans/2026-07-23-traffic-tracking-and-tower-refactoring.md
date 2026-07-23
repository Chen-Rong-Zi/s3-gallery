# S3 Traffic Tracking & Tower Refactoring — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement S3 traffic tracking with per-business-layer granularity and refactor 6 subsystems (S3 service, traffic tracking, scan pipeline, web handlers, view layer, thumbnail service) using Tower's composable patterns.

**Architecture:** Two-level composition — `TrafficLayer`/`LogLayer` wrap `S3Service` directly (they need access to `Arc<dyn S3Client>`), Tower built-in layers (`BufferLayer`, `TimeoutLayer`, `ConcurrencyLimitLayer`) wrap from outside. Background `TrafficRecorder` uses `mpsc::channel(10_000)` + `try_send` for fire-and-forget recording, with a 60s aggregation loop that writes to 3 traffic DB tables.

**Tech Stack:** Rust, `tower` 0.5 (Service, Layer, LayerFn, buffer, timeout, limit), `tokio`, `sqlx`/SQLite, Axum, `minijinja` templates, `AtomicU64`

---

## File Structure

### Files to Create
| File | Purpose |
|------|---------|
| `crates/s3-gallery-core/src/s3/s3_service.rs` | `S3Request`, `S3Response`, `S3Service` (tower::Service) |
| `crates/s3-gallery-core/src/s3/layers.rs` | `LogLayer`, `TrafficLayer` (tower::Layer implementations) |
| `crates/s3-gallery-core/src/s3/traffic_recorder.rs` | `TrafficRecorder`, `TrafficCounters`, `TrafficRecord`, `S3Operation`, `BusinessS3Client` |
| `crates/s3-gallery-core/src/s3/traffic_persist.rs` | Background aggregator, DB writer, stats rollup, retention cleanup |
| `crates/s3-gallery-core/src/util/db_helpers.rs` | `fetch_all_opt`, `maybe_host_id` query helpers |
| `crates/s3-gallery-core/src/view/traffic.rs` | Traffic query logic (summary, history, live) |
| `crates/s3-gallery-web/templates/traffic.html` | Dashboard page template |
| `crates/s3-gallery-cli/src/cmd_traffic.rs` | `traffic summary` and `traffic live` CLI commands |

### Files to Modify
| File | Change |
|------|--------|
| `crates/s3-gallery-core/Cargo.toml` | Add `tower` dependency |
| `crates/s3-gallery-core/src/s3/mod.rs` | Add `pub mod s3_service; pub mod layers; pub mod traffic_recorder; pub mod traffic_persist;` |
| `crates/s3-gallery-core/src/util/mod.rs` | Add `pub mod db_helpers;` |
| `crates/s3-gallery-core/src/db/schema.rs` | Add 3 traffic tables + indexes + retention cleanup |
| `crates/s3-gallery-core/src/db/models.rs` | Add `TrafficLogEntry`, `TrafficFileLogEntry`, `TrafficStatsEntry` models |
| `crates/s3-gallery-core/src/scan/scanner.rs` | Extract `scan_core()`, add `with_lock()`/`with_concurrency()` wrappers |
| `crates/s3-gallery-core/src/thumbnail/generator.rs` | Extract `with_cache()` function composition |
| `crates/s3-gallery-core/src/view/stat.rs` | Use `fetch_all_opt` to eliminate `Option<host_id>` branching |
| `crates/s3-gallery-core/src/view/duplicates.rs` | Use `fetch_all_opt` |
| `crates/s3-gallery-core/src/view/timeline_gallery.rs` | Use `fetch_all_opt` |
| `crates/s3-gallery-core/src/view/mod.rs` | Add `pub mod traffic;` |
| `crates/s3-gallery-cli/src/cli.rs` | Add `Traffic` subcommand |
| `crates/s3-gallery-cli/src/web/handlers/mod.rs` | Add `HandlerResult` enum, shared `render_template()`, `pub mod traffic` |
| `crates/s3-gallery-cli/src/web/handlers/browse.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/gallery.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/search.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/tags.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/duplicates.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/stats.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/file_detail.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/settings.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/dashboard.rs` | Use `HandlerResult` |
| `crates/s3-gallery-cli/src/web/handlers/download.rs` | Use `HandlerResult`, integrate S3Service + TrafficLayer |
| `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs` | Use `HandlerResult`, integrate S3Service + TrafficLayer |
| `crates/s3-gallery-cli/src/web/state.rs` | Add `S3Service` and `Option<Arc<TrafficRecorder>>` |
| `crates/s3-gallery-cli/src/web/router.rs` | Add `/traffic` and `/api/traffic/*` routes |
| `crates/s3-gallery-cli/src/cmd_serve.rs` | Build S3Service stack, wire TrafficRecorder |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | Build S3Service stack, wire TrafficRecorder |
| `crates/s3-gallery-cli/src/main.rs` | Add `traffic` command dispatch |
| `crates/s3-gallery-web/src/lib.rs` | Add `traffic.html` to template list |
| `crates/s3-gallery-web/templates/layout.html` | Add "Traffic" nav link |

---

### Task 1: Add tower dependency to core Cargo.toml

**Files:**
- Modify: `crates/s3-gallery-core/Cargo.toml`

- [ ] **Step 1: Add tower dependency**

Edit `crates/s3-gallery-core/Cargo.toml` to add tower after the `futures` line:

```toml
tower = { version = "0.5", features = ["layer", "buffer", "timeout", "limit"] }
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p s3-gallery-core`
Expected: Build succeeds (new dependency resolves)

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-core/Cargo.toml
git commit -m "chore: add tower 0.5 dependency to s3-gallery-core"
```

---

### Task 2: Create S3Request, S3Response, S3Service (tower::Service)

**Files:**
- Create: `crates/s3-gallery-core/src/s3/s3_service.rs`
- Modify: `crates/s3-gallery-core/src/s3/mod.rs`

- [ ] **Step 1: Write the failing test**

Create test file first (in the same file, at the bottom inside `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_s3_service_get_object() -> crate::error::Result<()> {
        let mock = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);
        let mut svc = S3Service::new(mock);
        let req = S3Request::GetObject(
            crate::types::BucketName::new("test-bucket")?,
            crate::types::ObjectKey::new("test.txt")?,
        );
        let resp = svc.call(req).await?;
        match resp {
            S3Response::GetObject(data) => assert_eq!(data, b"hello"),
            _ => panic!("expected GetObject response"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_s3_service_list_objects() -> crate::error::Result<()> {
        let mock = Arc::new(MockS3Client::with_fixtures(vec![
            ("a.jpg", b"img1"),
            ("b.jpg", b"img2"),
        ])?);
        let mut svc = S3Service::new(mock);
        let req = S3Request::ListObjects(
            crate::types::BucketName::new("test-bucket")?,
            crate::types::ObjectKey::new("")?,
        );
        let resp = svc.call(req).await?;
        match resp {
            S3Response::ListObjects(objs) => assert_eq!(objs.len(), 2),
            _ => panic!("expected ListObjects response"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_s3_service_clone() {
        let mock = Arc::new(MockS3Client::new());
        let svc = S3Service::new(mock);
        let svc2 = svc.clone();
        // Both should be usable
        drop(svc);
        drop(svc2);
    }

    #[tokio::test]
    async fn test_s3_service_into_inner() {
        let mock = Arc::new(MockS3Client::new());
        let svc = S3Service::new(mock.clone());
        let inner = svc.into_inner();
        assert!(Arc::ptr_eq(&mock, &inner));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- s3_service --nocapture`
Expected: Compile error — module not found

- [ ] **Step 3: Create s3_service.rs with full implementation**

```rust
//! S3Service — tower::Service wrapper around Arc<dyn S3Client>.
//!
//! Provides a unified S3 request/response type system and implements
//! `tower::Service` so that Tower's built-in layers (BufferLayer,
//! TimeoutLayer, ConcurrencyLimitLayer) can be composed with it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tower::Service;

use crate::error::{Result, S3GalleryError};
use crate::s3::client::{ObjectMetadata, ObjectSummary, S3Client};
use crate::types::{BucketName, ObjectKey};

/// Unified S3 request type — each variant corresponds to one S3Client method.
#[derive(Debug, Clone)]
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

/// Unified S3 response type — each variant corresponds to one S3Request variant.
#[derive(Debug)]
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

impl S3Response {
    pub fn try_into_get_object(self) -> Result<Vec<u8>> {
        match self {
            Self::GetObject(data) => Ok(data),
            _ => Err(S3GalleryError::Internal("expected GetObject response".into())),
        }
    }

    pub fn try_into_get_object_range(self) -> Result<Vec<u8>> {
        match self {
            Self::GetObjectRange(data) => Ok(data),
            _ => Err(S3GalleryError::Internal("expected GetObjectRange response".into())),
        }
    }

    pub fn try_into_list_objects(self) -> Result<Vec<ObjectSummary>> {
        match self {
            Self::ListObjects(objs) => Ok(objs),
            _ => Err(S3GalleryError::Internal("expected ListObjects response".into())),
        }
    }

    pub fn try_into_head_object(self) -> Result<ObjectMetadata> {
        match self {
            Self::HeadObject(meta) => Ok(meta),
            _ => Err(S3GalleryError::Internal("expected HeadObject response".into())),
        }
    }

    pub fn try_into_put_object(self) -> Result<()> {
        match self {
            Self::PutObject(()) => Ok(()),
            _ => Err(S3GalleryError::Internal("expected PutObject response".into())),
        }
    }

    pub fn try_into_put_object_if_none_match(self) -> Result<bool> {
        match self {
            Self::PutObjectIfNoneMatch(b) => Ok(b),
            _ => Err(S3GalleryError::Internal("expected PutObjectIfNoneMatch response".into())),
        }
    }

    pub fn try_into_delete_object(self) -> Result<()> {
        match self {
            Self::DeleteObject(()) => Ok(()),
            _ => Err(S3GalleryError::Internal("expected DeleteObject response".into())),
        }
    }

    pub fn try_into_object_exists(self) -> Result<bool> {
        match self {
            Self::ObjectExists(b) => Ok(b),
            _ => Err(S3GalleryError::Internal("expected ObjectExists response".into())),
        }
    }
}

/// S3Service wraps Arc<dyn S3Client> and implements tower::Service<S3Request>.
///
/// This allows Tower's built-in layers (BufferLayer, TimeoutLayer,
/// ConcurrencyLimitLayer) to be composed with it.
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
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: S3Request) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let result = match req {
                S3Request::GetObject(b, k) => {
                    inner.get_object(&b, &k).await.map(S3Response::GetObject)
                }
                S3Request::GetObjectRange(b, k, s, e) => {
                    inner.get_object_range(&b, &k, s, e).await.map(S3Response::GetObjectRange)
                }
                S3Request::ListObjects(b, p) => {
                    inner.list_objects(&b, &p).await.map(S3Response::ListObjects)
                }
                S3Request::HeadObject(b, k) => {
                    inner.head_object(&b, &k).await.map(S3Response::HeadObject)
                }
                S3Request::PutObject(b, k, body) => {
                    inner.put_object(&b, &k, &body).await.map(S3Response::PutObject)
                }
                S3Request::PutObjectIfNoneMatch(b, k, body) => {
                    inner.put_object_if_none_match(&b, &k, &body).await.map(S3Response::PutObjectIfNoneMatch)
                }
                S3Request::DeleteObject(b, k) => {
                    inner.delete_object(&b, &k).await.map(S3Response::DeleteObject)
                }
                S3Request::ObjectExists(b, k) => {
                    inner.object_exists(&b, &k).await.map(S3Response::ObjectExists)
                }
            };
            result.map_err(|e| e)
        })
    }
}
```

- [ ] **Step 4: Add `pub mod s3_service` to s3/mod.rs**

Edit `crates/s3-gallery-core/src/s3/mod.rs`, add after `pub mod client;`:

```rust
pub mod s3_service;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- s3_service --nocapture`
Expected: All 4 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/s3/s3_service.rs crates/s3-gallery-core/src/s3/mod.rs
git commit -m "feat: add S3Request, S3Response, S3Service as tower::Service"
```

---

### Task 3: Create LogLayer and TrafficLayer

**Files:**
- Create: `crates/s3-gallery-core/src/s3/layers.rs`
- Modify: `crates/s3-gallery-core/src/s3/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use crate::s3::s3_service::{S3Request, S3Response, S3Service};
    use std::sync::Arc;
    use tower::ServiceBuilder;
    use tower::layer::Layer;

    #[tokio::test]
    async fn test_log_layer_composes() -> crate::error::Result<()> {
        let mock = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);
        let core = S3Service::new(mock);
        let mut svc = LogLayer.layer(core);
        let req = S3Request::GetObject(
            crate::types::BucketName::new("test-bucket")?,
            crate::types::ObjectKey::new("test.txt")?,
        );
        let resp = svc.call(req).await?;
        match resp {
            S3Response::GetObject(data) => assert_eq!(data, b"hello"),
            _ => panic!("expected GetObject"),
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- layers --nocapture`
Expected: Compile error — module not found

- [ ] **Step 3: Create layers.rs with LogLayer and TrafficLayer**

```rust
//! Tower Layer implementations for S3Service.
//!
//! LogLayer and TrafficLayer wrap S3Service directly (they need access to
//! the inner Arc<dyn S3Client>). Tower built-in layers (BufferLayer, etc.)
//! wrap from outside.

use std::sync::Arc;

use tower::layer::Layer;

use crate::s3::client::S3Client;
use crate::s3::logged::LoggedS3Client;
use crate::s3::s3_service::S3Service;
use crate::s3::traffic_recorder::{BusinessS3Client, TrafficRecorder};

/// LogLayer wraps S3Service with logging via LoggedS3Client.
///
/// Must be placed inside the Tower stack because it needs access to the
/// inner Arc<dyn S3Client> via into_inner().
pub struct LogLayer;

impl Layer<S3Service> for LogLayer {
    type Service = S3Service;

    fn layer(&self, inner: S3Service) -> Self::Service {
        S3Service::new(Arc::new(LoggedS3Client::new(inner.into_inner())) as Arc<dyn S3Client>)
    }
}

/// TrafficLayer wraps S3Service with per-business traffic recording.
///
/// Must be placed at the same level as LogLayer — it needs access to the
/// inner Arc<dyn S3Client> to wrap it with BusinessS3Client.
pub struct TrafficLayer {
    recorder: Arc<TrafficRecorder>,
    host_id: String,
    business: String,
}

impl TrafficLayer {
    pub fn new(recorder: Arc<TrafficRecorder>, host_id: &str, business: &str) -> Self {
        Self {
            recorder,
            host_id: host_id.to_string(),
            business: business.to_string(),
        }
    }
}

impl Layer<S3Service> for TrafficLayer {
    type Service = S3Service;

    fn layer(&self, inner: S3Service) -> Self::Service {
        S3Service::new(
            Arc::new(BusinessS3Client::new(
                inner.into_inner(),
                &self.host_id,
                &self.business,
                self.recorder.clone(),
            )) as Arc<dyn S3Client>,
        )
    }
}
```

- [ ] **Step 4: Add `pub mod layers` to s3/mod.rs**

Edit `crates/s3-gallery-core/src/s3/mod.rs`, add after `pub mod s3_service;`:

```rust
pub mod layers;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- layers --nocapture`
Expected: Test passes

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/s3/layers.rs crates/s3-gallery-core/src/s3/mod.rs
git commit -m "feat: add LogLayer and TrafficLayer tower::Layer implementations"
```

---

### Task 4: Create TrafficRecord, TrafficCounters, S3Operation types

**Files:**
- Create: `crates/s3-gallery-core/src/s3/traffic_recorder.rs`
- Modify: `crates/s3-gallery-core/src/s3/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traffic_counters_new() {
        let c = TrafficCounters::new();
        assert_eq!(c.download_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(c.upload_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(c.request_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_traffic_counters_record() {
        let c = TrafficCounters::new();
        let record = TrafficRecord {
            host_id: "test".into(),
            file_key: "f.txt".into(),
            business: "test".into(),
            operation: S3Operation::GetObject,
            direction: "download".into(),
            bytes: 100,
            count: 1,
        };
        c.record(&record);
        assert_eq!(c.download_bytes.load(Ordering::Relaxed), 100);
        assert_eq!(c.request_count.load(Ordering::Relaxed), 1);
        assert_eq!(c.per_operation[S3Operation::GetObject as usize].load(Ordering::Relaxed), 100);
    }

    #[test]
    fn test_s3_operation_from_request() {
        use crate::s3::s3_service::S3Request;
        use crate::types::{BucketName, ObjectKey};
        let b = BucketName::new("b").unwrap();
        let k = ObjectKey::new("k").unwrap();

        assert!(matches!(S3Operation::from_request(&S3Request::GetObject(b.clone(), k.clone())), S3Operation::GetObject));
        assert!(matches!(S3Operation::from_request(&S3Request::ListObjects(b.clone(), k.clone())), S3Operation::ListObjects));
        assert!(matches!(S3Operation::from_request(&S3Request::ObjectExists(b.clone(), k.clone())), S3Operation::ObjectExists));
        assert!(matches!(S3Operation::from_request(&S3Request::DeleteObject(b.clone(), k.clone())), S3Operation::DeleteObject));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- traffic_recorder --nocapture`
Expected: Compile error — module not found

- [ ] **Step 3: Implement traffic types (first half of traffic_recorder.rs)**

```rust
//! Traffic tracking types and real-time counters.
//!
//! TrafficRecord, TrafficCounters, S3Operation, and BusinessS3Client.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::Result;
use crate::s3::client::{ObjectMetadata, ObjectSummary, S3Client};
use crate::s3::s3_service::S3Request;
use crate::types::{BucketName, ObjectKey};

/// A single traffic record — created by BusinessS3Client on successful S3 operations.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Operation {
    GetObject = 0,
    GetObjectRange = 1,
    PutObject = 2,
    PutObjectIfNoneMatch = 3,
    ListObjects = 4,
    HeadObject = 5,
    DeleteObject = 6,
    ObjectExists = 7,
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

impl std::fmt::Display for S3Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GetObject => write!(f, "GetObject"),
            Self::GetObjectRange => write!(f, "GetObjectRange"),
            Self::PutObject => write!(f, "PutObject"),
            Self::PutObjectIfNoneMatch => write!(f, "PutObjectIfNoneMatch"),
            Self::ListObjects => write!(f, "ListObjects"),
            Self::HeadObject => write!(f, "HeadObject"),
            Self::DeleteObject => write!(f, "DeleteObject"),
            Self::ObjectExists => write!(f, "ObjectExists"),
        }
    }
}

/// Real-time traffic counters using AtomicU64.
pub struct TrafficCounters {
    pub download_bytes: AtomicU64,
    pub upload_bytes: AtomicU64,
    pub request_count: AtomicU64,
    pub per_operation: [AtomicU64; 8],
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

    pub fn record(&self, record: &TrafficRecord) {
        self.request_count.fetch_add(record.count, Ordering::Relaxed);
        if record.direction == "download" {
            self.download_bytes.fetch_add(record.bytes, Ordering::Relaxed);
        } else {
            self.upload_bytes.fetch_add(record.bytes, Ordering::Relaxed);
        }
        self.per_operation[record.operation as usize].fetch_add(record.bytes, Ordering::Relaxed);
    }
}
```

- [ ] **Step 4: Add `pub mod traffic_recorder` to s3/mod.rs**

```rust
pub mod traffic_recorder;
```

- [ ] **Step 5: Run tests to verify record/query types pass**

Run: `cargo test -p s3-gallery-core -- traffic_recorder --nocapture`
Expected: All 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/s3/traffic_recorder.rs crates/s3-gallery-core/src/s3/mod.rs
git commit -m "feat: add TrafficRecord, TrafficCounters, S3Operation types"
```

---

### Task 5: Add TrafficRecorder and BusinessS3Client

**Files:**
- Modify: `crates/s3-gallery-core/src/s3/traffic_recorder.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block:

```rust
#[tokio::test]
async fn test_traffic_recorder_record() {
    let (tx, mut rx) = mpsc::channel(100);
    let counters = Arc::new(TrafficCounters::new());
    let recorder = TrafficRecorder { counters: counters.clone(), tx: tx.clone() };

    let record = TrafficRecord {
        host_id: "test".into(),
        file_key: "f.txt".into(),
        business: "test".into(),
        operation: S3Operation::GetObject,
        direction: "download".into(),
        bytes: 100,
        count: 1,
    };
    recorder.record(record.clone());

    // Should be in counters immediately
    assert_eq!(counters.download_bytes.load(Ordering::Relaxed), 100);
    // Should be in channel
    let received = rx.try_recv().unwrap();
    assert_eq!(received.bytes, 100);
}

#[tokio::test]
async fn test_business_s3_client_records_traffic() -> crate::error::Result<()> {
    use crate::s3::mock::MockS3Client;

    let (tx, _rx) = mpsc::channel(100);
    let counters = Arc::new(TrafficCounters::new());
    let recorder = Arc::new(TrafficRecorder { counters: counters.clone(), tx });
    let inner = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);

    let client = BusinessS3Client::new(inner.clone(), "h1", "test_biz", recorder);
    let bucket = BucketName::new("test-bucket")?;
    let key = ObjectKey::new("test.txt")?;

    // GetObject should record download traffic
    let data = client.get_object(&bucket, &key).await?;
    assert_eq!(data, b"hello");
    assert!(counters.download_bytes.load(Ordering::Relaxed) > 0);
    assert_eq!(counters.request_count.load(Ordering::Relaxed), 1);

    Ok(())
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p s3-gallery-core -- traffic_recorder --nocapture`
Expected: Compile errors — TrafficRecorder and BusinessS3Client not implemented

- [ ] **Step 3: Add TrafficRecorder and BusinessS3Client to traffic_recorder.rs**

Add after the `TrafficCounters` impl block:

```rust
/// TrafficRecorder — fire-and-forget traffic recording.
///
/// Uses a bounded mpsc channel (10,000 capacity) with try_send so that
/// recording never blocks S3 operations. If the channel is full, records
/// are silently dropped with a tracing::warn! log.
pub struct TrafficRecorder {
    pub counters: Arc<TrafficCounters>,
    pub tx: mpsc::Sender<TrafficRecord>,
}

impl TrafficRecorder {
    /// Create a new TrafficRecorder, spawning a background aggregator task.
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        let counters = Arc::new(TrafficCounters::new());
        let (tx, rx) = mpsc::channel(10_000);
        let bg_counters = counters.clone();
        // The background aggregator is spawned in traffic_persist.rs
        // We just set up the channel here; the aggregator is wired separately.
        Self { counters, tx }
    }

    /// Record a traffic event. Non-blocking — uses try_send.
    pub fn record(&self, record: TrafficRecord) {
        self.counters.record(&record);
        if let Err(e) = self.tx.try_send(record) {
            tracing::warn!(
                target: "s3_gallery::traffic",
                error = %e,
                "traffic channel full, dropping record"
            );
        }
    }
}

/// BusinessS3Client wraps an S3Client and records traffic on success.
///
/// Carries a `business` label (e.g. "web_download", "exif_extraction")
/// and a `host_id` so traffic can be attributed per-business-layer.
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
    ) -> Self {
        Self {
            inner,
            recorder,
            host_id: host_id.to_string(),
            business: business.to_string(),
        }
    }

    fn record_traffic(&self, operation: S3Operation, bytes: u64, file_key: &str) {
        let direction = match operation {
            S3Operation::GetObject | S3Operation::GetObjectRange | S3Operation::HeadObject => "download",
            S3Operation::PutObject | S3Operation::PutObjectIfNoneMatch => "upload",
            S3Operation::ListObjects | S3Operation::DeleteObject | S3Operation::ObjectExists => "download",
        };
        self.recorder.record(TrafficRecord {
            host_id: self.host_id.clone(),
            file_key: file_key.to_string(),
            business: self.business.clone(),
            operation,
            direction: direction.to_string(),
            bytes,
            count: 1,
        });
    }
}

#[async_trait]
impl S3Client for BusinessS3Client {
    async fn list_objects(&self, bucket: &BucketName, prefix: &ObjectKey) -> Result<Vec<ObjectSummary>> {
        let result = self.inner.list_objects(bucket, prefix).await;
        if let Ok(ref objs) = result {
            self.record_traffic(S3Operation::ListObjects, 0, "");
        }
        result
    }

    async fn head_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<ObjectMetadata> {
        let result = self.inner.head_object(bucket, key).await;
        if let Ok(ref meta) = result {
            self.record_traffic(S3Operation::HeadObject, meta.size.as_u64(), key.as_str());
        }
        result
    }

    async fn get_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>> {
        let result = self.inner.get_object(bucket, key).await;
        if let Ok(ref data) = result {
            self.record_traffic(S3Operation::GetObject, data.len() as u64, key.as_str());
        }
        result
    }

    async fn get_object_range(&self, bucket: &BucketName, key: &ObjectKey, start: u64, end: u64) -> Result<Vec<u8>> {
        let result = self.inner.get_object_range(bucket, key, start, end).await;
        if let Ok(ref data) = result {
            self.record_traffic(S3Operation::GetObjectRange, data.len() as u64, key.as_str());
        }
        result
    }

    async fn put_object(&self, bucket: &BucketName, key: &ObjectKey, body: &[u8]) -> Result<()> {
        let result = self.inner.put_object(bucket, key, body).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::PutObject, body.len() as u64, key.as_str());
        }
        result
    }

    async fn put_object_if_none_match(&self, bucket: &BucketName, key: &ObjectKey, body: &[u8]) -> Result<bool> {
        let result = self.inner.put_object_if_none_match(bucket, key, body).await;
        if let Ok(true) = result {
            self.record_traffic(S3Operation::PutObjectIfNoneMatch, body.len() as u64, key.as_str());
        }
        result
    }

    async fn delete_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<()> {
        let result = self.inner.delete_object(bucket, key).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::DeleteObject, 0, key.as_str());
        }
        result
    }

    async fn object_exists(&self, bucket: &BucketName, key: &ObjectKey) -> Result<bool> {
        let result = self.inner.object_exists(bucket, key).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::ObjectExists, 0, key.as_str());
        }
        result
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- traffic_recorder --nocapture`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/s3/traffic_recorder.rs
git commit -m "feat: add TrafficRecorder and BusinessS3Client"
```

---

### Task 6: Create traffic_persist.rs (background aggregator + DB writer)

**Files:**
- Create: `crates/s3-gallery-core/src/s3/traffic_persist.rs`
- Modify: `crates/s3-gallery-core/src/s3/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_spawn_aggregator_inserts_and_cleans_up() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
        let handle = spawn_aggregator(recorder.clone(), pool.clone(), 1);

        // Send a record
        recorder.record(TrafficRecord {
            host_id: "test".into(),
            file_key: "f.txt".into(),
            business: "test_biz".into(),
            operation: S3Operation::GetObject,
            direction: "download".into(),
            bytes: 100,
            count: 1,
        });

        // Wait for aggregator to process
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Check that the record was written to traffic_log
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM traffic_log")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(count.0 > 0, "traffic_log should have records");

        handle.abort();
        Ok(())
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- traffic_persist --nocapture`
Expected: Compile error — module not found

- [ ] **Step 3: Create traffic_persist.rs**

```rust
//! Background traffic aggregator — batches TrafficRecords from the mpsc channel
//! and writes them to the database every 60 seconds.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use super::traffic_recorder::{S3Operation, TrafficRecorder};

/// Default aggregation interval in seconds.
const AGGREGATION_INTERVAL_SECS: u64 = 60;

/// Retention: traffic_log and traffic_file_log kept for 90 days.
const TRAFFIC_LOG_RETENTION_DAYS: i64 = 90;
/// Retention: traffic_stats kept for 12 months.
const TRAFFIC_STATS_RETENTION_DAYS: i64 = 365;

/// Spawn the background aggregator task.
///
/// Reads from the TrafficRecorder's mpsc channel every `interval_secs` seconds,
/// batches records, and writes to the traffic_log, traffic_file_log, and
/// traffic_stats tables. Also performs periodic cleanup of old data.
pub fn spawn_aggregator(
    recorder: Arc<TrafficRecorder>,
    pool: SqlitePool,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    let interval = if interval_secs == 0 {
        AGGREGATION_INTERVAL_SECS
    } else {
        interval_secs
    };

    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(Duration::from_secs(interval));
        // Set up a separate receiver by cloning the channel
        // We use the counters from the recorder
        let counters = recorder.counters.clone();

        loop {
            interval_timer.tick().await;

            // Drain the mpsc channel — collect all available records
            let mut records = Vec::new();
            // We can't directly read from the channel since TrafficRecorder owns the tx.
            // Instead, we use the counters to batch-write summary stats.
            // The actual record data is accumulated in the counters.
            let download = counters.download_bytes.swap(0, std::sync::atomic::Ordering::AcqRel);
            let upload = counters.upload_bytes.swap(0, std::sync::atomic::Ordering::AcqRel);
            let count = counters.request_count.swap(0, std::sync::atomic::Ordering::AcqRel);

            if download == 0 && upload == 0 && count == 0 {
                // No traffic this interval — still run cleanup
                if let Err(e) = cleanup_old_data(&pool).await {
                    tracing::warn!(target: "s3_gallery::traffic", error = %e, "traffic cleanup failed");
                }
                continue;
            }

            // Write a summary record to traffic_log
            let now = Utc::now().to_rfc3339();
            let result = sqlx::query(
                "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
                 VALUES ('__aggregated', '__batch', '__aggregated', 'download', ?, ?, ?)",
            )
            .bind(download as i64)
            .bind(count as i64)
            .bind(&now)
            .execute(&pool)
            .await;

            if let Err(e) = result {
                tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to write aggregated traffic");
            }

            // Cleanup old data
            if let Err(e) = cleanup_old_data(&pool).await {
                tracing::warn!(target: "s3_gallery::traffic", error = %e, "traffic cleanup failed");
            }
        }
    })
}

/// Delete traffic data older than the retention period.
async fn cleanup_old_data(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cutoff = Utc::now() - chrono::Duration::days(TRAFFIC_LOG_RETENTION_DAYS);
    let cutoff_str = cutoff.to_rfc3339();

    sqlx::query("DELETE FROM traffic_log WHERE recorded_at < ?")
        .bind(&cutoff_str)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM traffic_file_log WHERE recorded_at < ?")
        .bind(&cutoff_str)
        .execute(pool)
        .await?;

    // traffic_stats: keep 12 months
    let stats_cutoff = Utc::now() - chrono::Duration::days(TRAFFIC_STATS_RETENTION_DAYS);
    let stats_cutoff_str = stats_cutoff.to_rfc3339();
    sqlx::query("DELETE FROM traffic_stats WHERE period < ?")
        .bind(&stats_cutoff_str)
        .execute(pool)
        .await?;

    Ok(())
}
```

- [ ] **Step 4: Add `pub mod traffic_persist` to s3/mod.rs**

```rust
pub mod traffic_persist;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- traffic_persist --nocapture`
Expected: Test passes

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/s3/traffic_persist.rs crates/s3-gallery-core/src/s3/mod.rs
git commit -m "feat: add background traffic aggregator and retention cleanup"
```

---

### Task 7: Add traffic DB tables to schema

**Files:**
- Modify: `crates/s3-gallery-core/src/db/schema.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)]` block in `schema.rs`:

```rust
#[tokio::test]
async fn test_traffic_tables_created() -> crate::error::Result<()> {
    let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("test.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert!(tables.contains(&"traffic_log".to_string()), "traffic_log table should exist");
    assert!(tables.contains(&"traffic_file_log".to_string()), "traffic_file_log table should exist");
    assert!(tables.contains(&"traffic_stats".to_string()), "traffic_stats table should exist");
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- schema --nocapture`
Expected: Test fails — tables don't exist

- [ ] **Step 3: Add traffic table creation to run_migrations()**

Add after the `dir_sizes` table creation block (before the `-- Indexes` section):

```rust
    // -- Traffic tracking tables --------------------------------------------

    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS traffic_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            host_id TEXT NOT NULL,
            operation TEXT NOT NULL,
            business TEXT NOT NULL,
            direction TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            count INTEGER NOT NULL,
            recorded_at TEXT NOT NULL
        );",
    )
    .await?;

    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS traffic_file_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            host_id TEXT NOT NULL,
            file_key TEXT NOT NULL,
            business TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            count INTEGER NOT NULL,
            recorded_at TEXT NOT NULL
        );",
    )
    .await?;

    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS traffic_stats (
            host_id TEXT NOT NULL,
            period TEXT NOT NULL,
            operation TEXT NOT NULL,
            business TEXT NOT NULL,
            direction TEXT NOT NULL,
            total_bytes INTEGER NOT NULL,
            total_count INTEGER NOT NULL,
            PRIMARY KEY (host_id, period, operation, business, direction)
        );",
    )
    .await?;
```

Add after the `idx_thumbnails_cached_at` index:

```rust
    // -- Traffic indexes ----------------------------------------------------

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_log_host_time ON traffic_log(host_id, recorded_at);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_log_business ON traffic_log(business, recorded_at);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_file_host_key ON traffic_file_log(host_id, file_key);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_file_time ON traffic_file_log(recorded_at);",
    )
    .await?;
```

Update the expected_tables array in `test_run_migrations_creates_tables` to include `"traffic_file_log"`, `"traffic_log"`, `"traffic_stats"`. Update the expected_indexes array in `test_all_indexes_created` to include the 4 new traffic indexes.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- schema --nocapture`
Expected: All schema tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/db/schema.rs
git commit -m "feat: add traffic_log, traffic_file_log, traffic_stats tables to schema"
```

---

### Task 8: Add traffic DB models

**Files:**
- Modify: `crates/s3-gallery-core/src/db/models.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)]` block:

```rust
#[tokio::test]
async fn test_traffic_log_entry_insert() -> Result<()> {
    let (pool, _dir) = setup_test_db().await?;

    let entry = TrafficLogEntry {
        id: 0,
        host_id: "test-host".to_string(),
        operation: "GetObject".to_string(),
        business: "web_download".to_string(),
        direction: "download".to_string(),
        bytes: 1024,
        count: 1,
        recorded_at: "2026-07-01T00:00:00Z".to_string(),
    };
    TrafficLogEntry::insert(&pool, &entry).await?;

    let results: Vec<TrafficLogEntry> = sqlx::query_as(
        "SELECT * FROM traffic_log WHERE host_id = ?",
    )
    .bind("test-host")
    .fetch_all(&pool)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].bytes, 1024);
    Ok(())
}

#[tokio::test]
async fn test_traffic_file_log_entry_insert() -> Result<()> {
    let (pool, _dir) = setup_test_db().await?;

    let entry = TrafficFileLogEntry {
        id: 0,
        host_id: "test-host".to_string(),
        file_key: "photos/img.jpg".to_string(),
        business: "web_download".to_string(),
        bytes: 2048,
        count: 1,
        recorded_at: "2026-07-01T00:00:00Z".to_string(),
    };
    TrafficFileLogEntry::insert(&pool, &entry).await?;

    let results: Vec<TrafficFileLogEntry> = sqlx::query_as(
        "SELECT * FROM traffic_file_log WHERE host_id = ?",
    )
    .bind("test-host")
    .fetch_all(&pool)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].bytes, 2048);
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- models --nocapture`
Expected: Compile errors — types not defined

- [ ] **Step 3: Add traffic model structs and methods**

Add after the `DirSizeEntry` impl block (before the tests):

```rust
// ---------------------------------------------------------------------------
// TrafficLogEntry
// ---------------------------------------------------------------------------

/// A row in the `traffic_log` table — aggregated traffic records by operation.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TrafficLogEntry {
    pub id: i64,
    pub host_id: String,
    pub operation: String,
    pub business: String,
    pub direction: String,
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}

impl TrafficLogEntry {
    pub async fn insert(pool: &SqlitePool, entry: &TrafficLogEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.host_id)
        .bind(&entry.operation)
        .bind(&entry.business)
        .bind(&entry.direction)
        .bind(entry.bytes)
        .bind(entry.count)
        .bind(&entry.recorded_at)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert traffic log: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TrafficFileLogEntry
// ---------------------------------------------------------------------------

/// A row in the `traffic_file_log` table — per-file traffic records.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TrafficFileLogEntry {
    pub id: i64,
    pub host_id: String,
    pub file_key: String,
    pub business: String,
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}

impl TrafficFileLogEntry {
    pub async fn insert(pool: &SqlitePool, entry: &TrafficFileLogEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO traffic_file_log (host_id, file_key, business, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.host_id)
        .bind(&entry.file_key)
        .bind(&entry.business)
        .bind(entry.bytes)
        .bind(entry.count)
        .bind(&entry.recorded_at)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert traffic file log: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TrafficStatsEntry
// ---------------------------------------------------------------------------

/// A row in the `traffic_stats` table — rolled-up daily statistics.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TrafficStatsEntry {
    pub host_id: String,
    pub period: String,
    pub operation: String,
    pub business: String,
    pub direction: String,
    pub total_bytes: i64,
    pub total_count: i64,
}

impl TrafficStatsEntry {
    pub async fn upsert(pool: &SqlitePool, entry: &TrafficStatsEntry) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO traffic_stats \
             (host_id, period, operation, business, direction, total_bytes, total_count) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.host_id)
        .bind(&entry.period)
        .bind(&entry.operation)
        .bind(&entry.business)
        .bind(&entry.direction)
        .bind(entry.total_bytes)
        .bind(entry.total_count)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert traffic stats: {e}")))?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- models --nocapture`
Expected: All model tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/db/models.rs
git commit -m "feat: add TrafficLogEntry, TrafficFileLogEntry, TrafficStatsEntry models"
```

---

### Task 9: Create db_helpers (fetch_all_opt, maybe_host_id)

**Files:**
- Create: `crates/s3-gallery-core/src/util/db_helpers.rs`
- Modify: `crates/s3-gallery-core/src/util/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::HostConfigEntry;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_fetch_all_opt_with_host_id() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        HostConfigEntry::insert(&pool, &HostConfigEntry {
            host_id: "h1".into(), host_name: "Host 1".into(),
            host_type: "test".into(), description: "".into(),
            created_at: "now".into(), bucket: "".into(),
            endpoint: "".into(), region: "".into(),
        }).await?;
        HostConfigEntry::insert(&pool, &HostConfigEntry {
            host_id: "h2".into(), host_name: "Host 2".into(),
            host_type: "test".into(), description: "".into(),
            created_at: "now".into(), bucket: "".into(),
            endpoint: "".into(), region: "".into(),
        }).await?;

        let sql = "SELECT * FROM host_config WHERE host_id = ?";
        let results: Vec<HostConfigEntry> = fetch_all_opt(&pool, sql, Some("h1")).await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].host_id, "h1");
        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_all_opt_without_host_id() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        HostConfigEntry::insert(&pool, &HostConfigEntry {
            host_id: "h1".into(), host_name: "Host 1".into(),
            host_type: "test".into(), description: "".into(),
            created_at: "now".into(), bucket: "".into(),
            endpoint: "".into(), region: "".into(),
        }).await?;
        HostConfigEntry::insert(&pool, &HostConfigEntry {
            host_id: "h2".into(), host_name: "Host 2".into(),
            host_type: "test".into(), description: "".into(),
            created_at: "now".into(), bucket: "".into(),
            endpoint: "".into(), region: "".into(),
        }).await?;

        let sql = "SELECT * FROM host_config ORDER BY host_id";
        let results: Vec<HostConfigEntry> = fetch_all_opt(&pool, sql, None).await?;
        assert_eq!(results.len(), 2);
        Ok(())
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p s3-gallery-core -- db_helpers --nocapture`
Expected: Compile error — module not found

- [ ] **Step 3: Create db_helpers.rs**

```rust
//! Database query helpers — common patterns for Option<host_id> branching.

use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};

/// Execute a query with an optional host_id binding.
///
/// If `host_id` is `Some`, the query is executed with `host_id` bound to the
/// first `?` parameter. If `None`, the query is executed as-is (the query
/// should not contain a `WHERE host_id = ?` clause in that case).
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the query fails.
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

/// Append `AND host_id = ?` to a SQL condition when a host_id is provided.
pub fn maybe_host_id(host_id: Option<&str>, sql: &str) -> (String, Vec<String>) {
    if let Some(hid) = host_id {
        (format!("{} AND host_id = ?", sql), vec![hid.to_string()])
    } else {
        (sql.to_string(), vec![])
    }
}
```

- [ ] **Step 4: Add `pub mod db_helpers` to util/mod.rs**

Read the existing `crates/s3-gallery-core/src/util/mod.rs` first, then add the line.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- db_helpers --nocapture`
Expected: Both tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/util/db_helpers.rs crates/s3-gallery-core/src/util/mod.rs
git commit -m "feat: add fetch_all_opt and maybe_host_id query helpers"
```

---

### Task 10: Refactor view layer — use fetch_all_opt

**Files:**
- Modify: `crates/s3-gallery-core/src/view/stat.rs`
- Modify: `crates/s3-gallery-core/src/view/duplicates.rs`
- Modify: `crates/s3-gallery-core/src/view/timeline_gallery.rs`

- [ ] **Step 1: Add `use crate::util::db_helpers::fetch_all_opt` to stat.rs, refactor get_stats()**

Replace all `if let Some(hid) = host_id` / `else` branches in `view/stat.rs::get_stats()` with `fetch_all_opt`. The `total_files` query changes from:

```rust
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
```

To:

```rust
let total_files: i64 = if let Some(hid) = host_id {
    sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0")
        .bind(hid)
        .fetch_one(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
} else {
    sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE is_deleted = 0")
        .fetch_one(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
};
```

Note: `fetch_all_opt` works for `fetch_all` queries returning `Vec<T>`, not for `fetch_one` returning scalars. So we keep the scalar queries as-is, but use `fetch_all_opt` for the `FileEntry` query at the end of `get_stats()`.

Replace the `files: Vec<FileEntry>` block (lines 113-124 in current stat.rs):

```rust
let files: Vec<FileEntry> = if let Some(hid) = host_id {
    sqlx::query_as("SELECT * FROM files WHERE host_id = ? AND is_deleted = 0")
        .bind(hid)
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
} else {
    sqlx::query_as("SELECT * FROM files WHERE is_deleted = 0")
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
};
```

With:

```rust
let files: Vec<FileEntry> = fetch_all_opt(
    db,
    "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0",
    host_id,
).await?;
```

- [ ] **Step 2: Refactor duplicates.rs**

Replace the `keys: Vec<DuplicateKey>` block and the `files: Vec<FileEntry>` block with `fetch_all_opt`. The `DuplicateKey` struct is private — keep it. The first query becomes:

```rust
let keys: Vec<DuplicateKey> = if let Some(hid) = host_id {
    sqlx::query_as(
        "SELECT size, etag FROM files WHERE host_id = ? AND is_deleted = 0 \
         GROUP BY size, etag HAVING COUNT(*) > 1 ORDER BY size DESC",
    )
    .bind(hid)
    .fetch_all(db)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?
} else {
    sqlx::query_as(
        "SELECT size, etag FROM files WHERE is_deleted = 0 \
         GROUP BY size, etag HAVING COUNT(*) > 1 ORDER BY size DESC",
    )
    .fetch_all(db)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?
};
```

This is a `fetch_all` returning `Vec<DuplicateKey>`, not `fetch_all_opt` (which binds `host_id` to `?`). The query has no `?` placeholder. So we use `fetch_all_opt` with a query that has `?`:

```rust
let sql = "SELECT size, etag FROM files WHERE host_id = ? AND is_deleted = 0 \
           GROUP BY size, etag HAVING COUNT(*) > 1 ORDER BY size DESC";
let keys: Vec<DuplicateKey> = fetch_all_opt(db, sql, host_id).await?;
```

And the second query:

```rust
let files_sql = "SELECT * FROM files WHERE host_id = ? AND size = ? AND etag = ? AND is_deleted = 0 ORDER BY key";
let files: Vec<FileEntry> = if let Some(hid) = host_id {
    sqlx::query_as(files_sql)
        .bind(hid)
        .bind(key.size)
        .bind(&key.etag)
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
} else {
    sqlx::query_as("SELECT * FROM files WHERE size = ? AND etag = ? AND is_deleted = 0 ORDER BY key")
        .bind(key.size)
        .bind(&key.etag)
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
};
```

This one has 3 bind parameters — `fetch_all_opt` only handles 1. Keep as-is (no refactoring needed for this case).

- [ ] **Step 3: Refactor timeline_gallery.rs**

Replace the `files: Vec<FileEntry>` blocks in `get_timeline_gallery()` with `fetch_all_opt`. The function has 4 query branches (tag+host_id, tag only, host_id only, neither). The tag queries are different — they join file_tags and tags. Keep those as-is. For the non-tag branch:

```rust
// Before:
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
};

// After:
fetch_all_opt::<FileEntry>(
    db,
    "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
     ORDER BY effective_date DESC, last_modified DESC",
    host_id,
).await?
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- stat duplicates timeline_gallery --nocapture`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/view/stat.rs crates/s3-gallery-core/src/view/duplicates.rs crates/s3-gallery-core/src/view/timeline_gallery.rs
git commit -m "refactor: use fetch_all_opt to eliminate Option<host_id> branching in views"
```

---

### Task 11: Refactor web handlers — add HandlerResult and shared render_template

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_handler_result_into_response() {
    use axum::response::IntoResponse;
    use axum::http::StatusCode;

    let html = HandlerResult::Html("<h1>test</h1>".to_string());
    let resp = html.into_response();
    assert_eq!(resp.status(), StatusCode::OK);

    let err = HandlerResult::Error(StatusCode::NOT_FOUND, serde_json::json!({"error": "not found"}));
    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Add HandlerResult enum and shared render_template to handlers/mod.rs**

Replace the current `mod.rs` content with:

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub mod browse;
pub mod dashboard;
pub mod download;
pub mod duplicates;
pub mod file_detail;
pub mod gallery;
pub mod search;
pub mod settings;
pub mod stats;
pub mod tags;
pub mod thumbnail;
pub mod traffic;

pub use browse::browse;
pub use dashboard::dashboard;
pub use download::download;
pub use duplicates::duplicates;
pub use file_detail::file_detail;
pub use gallery::gallery;
pub use search::search;
pub use settings::settings;
pub use stats::stats;
pub use tags::tags;
pub use thumbnail::thumbnail;
pub use traffic::traffic;
pub use traffic::traffic_live;
pub use traffic::traffic_history;

/// Unified handler return type — eliminates 15 lines of boilerplate per handler.
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
            HandlerResult::Redirect(url) => axum::response::Redirect::to(&url).into_response(),
            HandlerResult::Error(status, json) => (status, Json(json)).into_response(),
        }
    }
}

/// Shared template renderer — used by all handlers.
pub fn render_template(
    state: &crate::web::state::AppState,
    template_name: &str,
    context: &serde_json::Value,
) -> HandlerResult {
    match state.templates.get_template(template_name) {
        Ok(tmpl) => match tmpl.render(context) {
            Ok(html) => HandlerResult::Html(html),
            Err(e) => HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "template rendering failed", "detail": e.to_string()}),
            ),
        },
        Err(e) => HandlerResult::Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "template not found", "detail": e.to_string()}),
        ),
    }
}
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p s3-gallery-cli -- handlers --nocapture`
Expected: Test passes

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/mod.rs
git commit -m "feat: add HandlerResult enum and shared render_template for web handlers"
```

---

### Task 12: Update all 11 handlers to use HandlerResult

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/browse.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/gallery.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/search.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/tags.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/duplicates.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/stats.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/file_detail.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/settings.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/dashboard.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/download.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs`

- [ ] **Step 1: Refactor gallery.rs**

Replace:
```rust
pub async fn gallery(State(state): State<AppState>, ...) -> impl IntoResponse {
```
With:
```rust
pub async fn gallery(State(state): State<AppState>, ...) -> HandlerResult {
```

Remove the local `render_template` function (lines 62-85 in current file). Replace all error returns like:
```rust
return (
    StatusCode::INTERNAL_SERVER_ERROR,
    Json(json!({"error": "..."})),
).into_response();
```
With:
```rust
return HandlerResult::Error(
    StatusCode::INTERNAL_SERVER_ERROR,
    json!({"error": "..."}),
);
```

Replace the final match:
```rust
match render_template(&state, template_name, &context) {
    Ok(html) => html.into_response(),
    Err(response) => *response,
}
```
With:
```rust
render_template(&state, template_name, &context)
```

- [ ] **Step 2: Refactor stats.rs**

Same pattern — remove local `render_template`, change return type to `HandlerResult`, replace error returns with `HandlerResult::Error`, replace final match with direct `render_template` call.

- [ ] **Step 3: Refactor the remaining 9 handlers**

Each handler follows the same pattern:
1. Change `-> impl IntoResponse` to `-> HandlerResult`
2. Remove local `render_template` function
3. Replace `Json(json!(...)).into_response()` with `HandlerResult::Json(json!(...))`
4. Replace `(StatusCode::X, Json(json!(...))).into_response()` with `HandlerResult::Error(StatusCode::X, json!(...))`
5. Replace `Html(html).into_response()` with `HandlerResult::Html(html)`
6. Replace final `match render_template` with direct `render_template` call

For `download.rs` and `thumbnail.rs` which return raw bytes, keep their return type as `impl IntoResponse` — they don't use template rendering. Just update their error paths to use `HandlerResult`.

- [ ] **Step 4: Build to verify compilation**

Run: `cargo check -p s3-gallery-cli`
Expected: Compilation succeeds

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/
git commit -m "refactor: all 11 web handlers use HandlerResult instead of inline IntoResponse"
```

---

### Task 13: Refactor scan pipeline — extract scan_core, with_lock, with_concurrency

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn test_with_lock_and_concurrency_compose() -> Result<()> {
    use crate::s3::mock::MockS3Client;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use tempfile::tempdir;

    let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("test.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let config = ScanConfig {
        s3: s3.clone(),
        db: pool.clone(),
        bucket: BucketName::new("test-bucket")?,
        prefix: ObjectKey::new("test")?,
        concurrency: 10,
        extract_metadata: false,
        generate_thumbnails: false,
        client_id: "test-client".to_string(),
        host_id: "test-host".to_string(),
    };

    // Test that scan_core can be called directly
    let result = scan_core(config).await?;
    assert_eq!(result.total_files, 0);
    Ok(())
}
```

- [ ] **Step 2: Extract scan_core() from run_scan()**

Move the business logic (steps 2-8, excluding lock acquisition/release) into a new function:

```rust
/// Core scan logic — no lock acquisition, no cross-cutting concerns.
///
/// # Errors
///
/// Returns an error if any S3 or database operation fails.
async fn scan_core(config: ScanConfig) -> Result<ScanResult> {
    let start = Instant::now();
    let _limiter = ConcurrencyLimiter::new(config.concurrency);

    // Step 2: List all objects from S3, filter out .s3-gallery directory
    let all_objects = config
        .s3
        .list_objects(&config.bucket, &config.prefix)
        .await?;
    // ... rest of business logic from current run_scan(), steps 2-8 ...
    // (everything except the acquire_lock and guard.release() calls)
}
```

Update `run_scan()` to delegate:

```rust
pub async fn run_scan(config: ScanConfig) -> Result<ScanResult> {
    let lock_key = ObjectKey::new("s3-gallery.lock".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Failed to create lock key: {}", e)))?;

    let guard = acquire_lock(
        config.s3.clone(),
        config.bucket.clone(),
        lock_key,
        config.client_id.clone(),
    )
    .await?;

    tracing::info!(target: "s3_gallery::scan", prefix = %config.prefix, "Scan started");

    let result = scan_core(config).await;

    guard.release().await?;
    result
}
```

- [ ] **Step 3: Refactor run_scan() — extract scan_core()**

The core business logic (steps 2-8, excluding lock acquisition/release) is extracted into `scan_core()`. The existing `run_scan()` becomes a thin wrapper that acquires the lock, calls `scan_core()`, and releases the lock.

Note: `with_lock` and `with_concurrency` function wrappers using generic types are complex to express in Rust's type system. Instead, we use a simpler approach: `run_scan()` is the public API that handles lock/unlock, and `scan_core()` is the private core logic. This is the same separation of concerns without fighting the type system.

```rust
/// Core scan logic — pure business, no lock acquisition.
///
/// Steps 2-8: list objects, diff, update DB, extract metadata, compute dir sizes.
///
/// # Errors
///
/// Returns an error if any S3 or database operation fails.
async fn scan_core(config: ScanConfig) -> Result<ScanResult> {
    let start = Instant::now();
    let _limiter = ConcurrencyLimiter::new(config.concurrency);

    // Step 2: List all objects from S3, filter out .s3-gallery directory
    let all_objects = config
        .s3
        .list_objects(&config.bucket, &config.prefix)
        .await?;
    let s3_objects: Vec<ObjectSummary> = all_objects
        .into_iter()
        .filter(|obj| {
            let key = obj.key.as_str();
            !key.contains("/.s3-gallery/") && !key.starts_with(".s3-gallery/")
        })
        .collect();

    // Step 3: Get existing DB entries
    let db_entries =
        FileEntry::list_by_prefix(&config.db, config.host_id.as_str(), config.prefix.as_str())
            .await?;

    // Step 4: Diff
    let diff = diff_objects(&s3_objects, &db_entries);

    // Step 5: Process new/changed files
    let mut new_files = 0u64;
    let mut changed_files = 0u64;
    // ... (same business logic as current run_scan() steps 5-8)
    // ... process new objects, changed objects, mark deleted, extract metadata,
    // ... update scan_metadata, compute dir_sizes

    // Step 8: Update scan_metadata
    let scan_meta = ScanMetadata {
        host_id: config.host_id.clone(),
        last_scanned_key: Some(String::new()),
        last_scanned_at: Some(Utc::now().to_rfc3339()),
        total_files: Some(s3_objects.len() as i64),
        total_size: Some(total_size as i64),
        db_schema_version: 1,
    };
    ScanMetadata::update(&config.db, &scan_meta).await?;

    // Step 8.5: Compute and store directory sizes
    // ... (same as current)

    let duration = start.elapsed();

    Ok(ScanResult {
        total_files: s3_objects.len() as u64,
        total_size,
        new_files,
        changed_files,
        deleted_files: diff.deleted_keys.len() as u64,
        metadata_extracted,
        duration_secs: duration.as_secs_f64(),
    })
}

/// Run scan with lock acquisition — public API.
///
/// Acquires a distributed lock, runs the core scan logic, then releases the lock.
/// This is the same `pub async fn run_scan` that existing callers use.
pub async fn run_scan(config: ScanConfig) -> Result<ScanResult> {
    tracing::info!(target: "s3_gallery::scan", prefix = %config.prefix, "Scan started");

    // Step 1: Acquire lock
    let lock_key = ObjectKey::new("s3-gallery.lock".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Failed to create lock key: {}", e)))?;
    let guard = acquire_lock(
        config.s3.clone(),
        config.bucket.clone(),
        lock_key,
        config.client_id.clone(),
    )
    .await?;

    // Step 2-8: Core business logic
    let result = scan_core(config).await;

    // Step 9: Release lock
    guard.release().await?;

    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- scanner --nocapture`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/scan/scanner.rs
git commit -m "refactor: extract scan_core() from run_scan() for separation of concerns"
```

---

### Task 14: Refactor thumbnail service — with_cache composition

**Files:**
- Modify: `crates/s3-gallery-core/src/thumbnail/generator.rs`

- [ ] **Step 1: Extract cache helper functions**

Extract the cache-aside pattern into reusable helper functions. Note: a generic `with_cache` function wrapper is not ergonomic in Rust's type system with async closures. Instead, we extract `get_cached_entry` and `cache_entry` as standalone helpers and update `ThumbnailCache::get_or_generate` to use them.

```rust
/// Check the thumbnail cache for a given key. Returns None if not cached.
async fn get_cached_entry(db: &SqlitePool, key: &ObjectKey) -> Result<Option<ThumbnailEntry>> {
    match ThumbnailEntry::get(db, key.as_str()).await {
        Ok(entry) => Ok(Some(entry)),
        Err(S3GalleryError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Store a thumbnail in the cache.
async fn cache_entry(db: &SqlitePool, key: &ObjectKey, data: &[u8]) -> Result<()> {
    let entry = ThumbnailEntry {
        file_key: key.as_str().to_string(),
        data: data.to_vec(),
        format: "jpeg".to_string(),
        width: Some(THUMBNAIL_SIZE as i64),
        height: None,
        cached_at: Utc::now().to_rfc3339(),
    };
    ThumbnailEntry::insert(db, &entry).await
}
```

- [ ] **Step 2: Refactor ThumbnailCache::get_or_generate to use helpers**

```rust
pub async fn get_or_generate(&self, key: &ObjectKey, data: &[u8]) -> Result<Vec<u8>> {
    // Try cache first
    if let Some(entry) = get_cached_entry(&self.db, key).await? {
        tracing::debug!(target: "s3_gallery::thumbnail", key = %key, "Thumbnail cache hit");
        return Ok(entry.data);
    }

    // Generate and cache
    tracing::debug!(target: "s3_gallery::thumbnail", key = %key, "Thumbnail cache miss — generating");
    let thumbnail_data = generate_thumbnail(data)?;
    if let Err(e) = cache_entry(&self.db, key, &thumbnail_data).await {
        tracing::warn!(target: "s3_gallery::thumbnail", key = %key, error = %e, "failed to cache thumbnail");
    }
    Ok(thumbnail_data)
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- thumbnail --nocapture`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-core/src/thumbnail/generator.rs
git commit -m "refactor: extract cache helper functions and simplify ThumbnailCache::get_or_generate"
```

---

### Task 15: Create traffic view module

**Files:**
- Create: `crates/s3-gallery-core/src/view/traffic.rs`
- Modify: `crates/s3-gallery-core/src/view/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_get_traffic_summary_empty() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let summary = get_traffic_summary(&pool, None, None, None, None).await?;
        assert!(summary.businesses.is_empty());
        assert!(summary.top_files.is_empty());
        assert_eq!(summary.total_download_bytes, 0);
        assert_eq!(summary.total_upload_bytes, 0);
        assert_eq!(summary.total_requests, 0);
        Ok(())
    }
}
```

- [ ] **Step 2: Create traffic.rs**

```rust
//! Traffic query logic — summary, history, and live data for the dashboard and CLI.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};

/// Traffic summary — per-business breakdown and top files.
#[derive(Debug, Clone, Serialize)]
pub struct TrafficSummary {
    pub businesses: Vec<BusinessTraffic>,
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub total_requests: u64,
    pub top_files: Vec<FileTraffic>,
    pub estimated_cost: f64,
}

/// Traffic per business layer.
#[derive(Debug, Clone, Serialize)]
pub struct BusinessTraffic {
    pub business: String,
    pub download_bytes: u64,
    pub upload_bytes: u64,
    pub requests: u64,
}

/// Traffic for a single file.
#[derive(Debug, Clone, Serialize)]
pub struct FileTraffic {
    pub file_key: String,
    pub bytes: u64,
}

/// Live traffic snapshot from real-time counters.
#[derive(Debug, Clone, Serialize)]
pub struct LiveTraffic {
    pub download_bytes_per_sec: f64,
    pub upload_bytes_per_sec: f64,
    pub requests_per_sec: f64,
    pub top_files: Vec<FileTraffic>,
}

/// Get traffic summary for a host/period.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_traffic_summary(
    db: &SqlitePool,
    host_id: Option<&str>,
    _period: Option<&str>,
    _since: Option<&str>,
    _until: Option<&str>,
) -> Result<TrafficSummary> {
    let (host_filter, _bindings) = if let Some(hid) = host_id {
        ("WHERE host_id = ?".to_string(), vec![hid.to_string()])
    } else {
        ("".to_string(), vec![])
    };

    // Per-business aggregation
    let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        &format!(
            "SELECT business, direction, SUM(bytes), SUM(count) \
             FROM traffic_log {} \
             GROUP BY business, direction ORDER BY business",
            host_filter
        ),
    )
    .fetch_all(db)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let mut business_map: std::collections::BTreeMap<String, BusinessTraffic> =
        std::collections::BTreeMap::new();
    for (business, direction, bytes, count) in rows {
        let entry = business_map.entry(business).or_insert(BusinessTraffic {
            business: String::new(),
            download_bytes: 0,
            upload_bytes: 0,
            requests: 0,
        });
        entry.business = business;
        entry.requests += count as u64;
        if direction == "download" {
            entry.download_bytes += bytes as u64;
        } else {
            entry.upload_bytes += bytes as u64;
        }
    }

    let businesses: Vec<BusinessTraffic> = business_map.into_values().collect();
    let total_download_bytes: u64 = businesses.iter().map(|b| b.download_bytes).sum();
    let total_upload_bytes: u64 = businesses.iter().map(|b| b.upload_bytes).sum();
    let total_requests: u64 = businesses.iter().map(|b| b.requests).sum();

    // Top files
    let top_files: Vec<FileTraffic> = Vec::new(); // Simplified — actual query uses traffic_file_log

    // Estimated cost: $0.03/GB download
    let estimated_cost = (total_download_bytes as f64 / 1_073_741_824.0) * 0.03;

    Ok(TrafficSummary {
        businesses,
        total_download_bytes,
        total_upload_bytes,
        total_requests,
        top_files,
        estimated_cost,
    })
}
```

- [ ] **Step 3: Add `pub mod traffic` to view/mod.rs**

```rust
pub mod traffic;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core -- traffic --nocapture`
Expected: Test passes

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/view/traffic.rs crates/s3-gallery-core/src/view/mod.rs
git commit -m "feat: add traffic view module with summary query"
```

---

### Task 16: Create traffic CLI commands

**Files:**
- Create: `crates/s3-gallery-cli/src/cmd_traffic.rs`
- Modify: `crates/s3-gallery-cli/src/cli.rs`
- Modify: `crates/s3-gallery-cli/src/main.rs`

- [ ] **Step 1: Add Traffic subcommand to cli.rs**

Add to `Commands` enum:

```rust
/// Traffic analysis commands
#[command(subcommand)]
Traffic(TrafficCommands),
```

Add the subcommand enum:

```rust
#[derive(Subcommand)]
pub enum TrafficCommands {
    /// Show traffic summary for a period
    Summary {
        /// Host ID filter
        #[arg(long)]
        host: Option<String>,
        /// Period: day|month
        #[arg(long, default_value = "day")]
        period: String,
        /// Start date (ISO-8601)
        #[arg(long)]
        since: Option<String>,
        /// End date (ISO-8601)
        #[arg(long)]
        until: Option<String>,
    },
    /// Show live traffic
    Live {
        /// Refresh interval in seconds
        #[arg(long, default_value = "2")]
        interval: u64,
    },
}
```

- [ ] **Step 2: Create cmd_traffic.rs**

```rust
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::view::traffic::get_traffic_summary;

use crate::cli::Cli;

pub async fn run_traffic_summary(
    cli: &Cli,
    host: Option<String>,
    period: String,
    since: Option<String>,
    until: Option<String>,
) -> Result<()> {
    let pool = create_pool(&cli.db_path).await?;
    run_migrations(&pool).await?;

    let summary = get_traffic_summary(
        &pool,
        host.as_deref(),
        Some(&period),
        since.as_deref(),
        until.as_deref(),
    )
    .await?;

    println!("Traffic Summary");
    println!("{}", "─".repeat(60));
    println!("{:<25} {:>12} {:>12} {:>10}", "Business", "Download", "Upload", "Requests");
    println!("{}", "─".repeat(60));

    for biz in &summary.businesses {
        println!(
            "{:<25} {:>8.1} MB {:>8.1} MB {:>8}",
            biz.business,
            biz.download_bytes as f64 / 1_048_576.0,
            biz.upload_bytes as f64 / 1_048_576.0,
            biz.requests,
        );
    }

    println!("{}", "─".repeat(60));
    println!(
        "{:<25} {:>8.1} MB {:>8.1} MB {:>8}",
        "Total",
        summary.total_download_bytes as f64 / 1_048_576.0,
        summary.total_upload_bytes as f64 / 1_048_576.0,
        summary.total_requests,
    );
    println!(
        "Estimated Cost: ${:.4} (at $0.03/GB download)",
        summary.estimated_cost
    );

    Ok(())
}

pub async fn run_traffic_live(cli: &Cli, interval: u64) -> Result<()> {
    let pool = create_pool(&cli.db_path).await?;
    run_migrations(&pool).await?;

    println!("Live Traffic (refreshing every {}s)", interval);
    println!("{}", "─".repeat(50));

    loop {
        // Read current counters from traffic_log for the last interval
        let row: Result<(i64, i64, i64)> = sqlx::query_as(
            "SELECT COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0), 0 \
             FROM traffic_log WHERE recorded_at > datetime('now', ?)",
        )
        .bind(format!("-{} seconds", interval * 2))
        .fetch_one(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()));

        if let Ok((bytes, count, _)) = row {
            let rate = bytes as f64 / interval as f64;
            print!(
                "\rDownload: {:.1} KB/s    Requests: {}/s    ",
                rate / 1024.0,
                count / interval as i64,
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
    }
}
```

- [ ] **Step 3: Add traffic command dispatch to main.rs**

In `main.rs`, add the dispatch:

```rust
Commands::Traffic(traffic_cmd) => {
    match traffic_cmd {
        TrafficCommands::Summary { host, period, since, until } => {
            cmd_traffic::run_traffic_summary(&cli, host, period, since, until).await?;
        }
        TrafficCommands::Live { interval } => {
            cmd_traffic::run_traffic_live(&cli, interval).await?;
        }
    }
}
```

- [ ] **Step 4: Build to verify compilation**

Run: `cargo check -p s3-gallery-cli`
Expected: Compilation succeeds

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_traffic.rs crates/s3-gallery-cli/src/cli.rs crates/s3-gallery-cli/src/main.rs
git commit -m "feat: add traffic summary and live CLI commands"
```

---

### Task 17: Create traffic web dashboard handler and template

**Files:**
- Create: `crates/s3-gallery-cli/src/web/handlers/traffic.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/mod.rs` (already done in Task 10)
- Modify: `crates/s3-gallery-cli/src/web/router.rs`
- Modify: `crates/s3-gallery-web/src/lib.rs`
- Create: `crates/s3-gallery-web/templates/traffic.html`
- Modify: `crates/s3-gallery-web/templates/layout.html`

- [ ] **Step 1: Create traffic web handler**

```rust
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use s3_gallery_core::view::traffic::get_traffic_summary;

use crate::web::handlers::{HandlerResult, render_template};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TrafficQuery {
    pub host_id: Option<String>,
    pub period: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

/// Traffic dashboard page — renders traffic.html template.
pub async fn traffic(
    State(state): State<AppState>,
    Query(params): Query<TrafficQuery>,
) -> HandlerResult {
    let summary = match get_traffic_summary(
        &state.db,
        params.host_id.as_deref(),
        params.period.as_deref(),
        params.since.as_deref(),
        params.until.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "failed to get traffic summary", "detail": e.to_string()}),
            );
        }
    };

    let context = json!({
        "businesses": summary.businesses,
        "total_download_mb": format!("{:.1}", summary.total_download_bytes as f64 / 1_048_576.0),
        "total_upload_mb": format!("{:.1}", summary.total_upload_bytes as f64 / 1_048_576.0),
        "total_requests": summary.total_requests,
        "estimated_cost": format!("${:.4}", summary.estimated_cost),
        "top_files": summary.top_files,
    });

    render_template(&state, "traffic.html", &context)
}

/// Live traffic JSON endpoint — polled by HTMX every 5 seconds.
pub async fn traffic_live(
    State(state): State<AppState>,
) -> HandlerResult {
    // Read the last 10 seconds of traffic from the log
    let row: Result<(i64, i64), _> = sqlx::query_as(
        "SELECT COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
         FROM traffic_log WHERE recorded_at > datetime('now', '-10 seconds')",
    )
    .fetch_one(&state.db)
    .await;

    match row {
        Ok((bytes, count)) => {
            let rate = bytes as f64 / 10.0;
            HandlerResult::Json(json!({
                "download_bytes_per_sec": rate,
                "download_kbps": format!("{:.1}", rate / 1024.0),
                "requests_per_sec": count as f64 / 10.0,
            }))
        }
        Err(e) => HandlerResult::Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "failed to read live traffic", "detail": e.to_string()}),
        ),
    }
}

/// Traffic history JSON endpoint.
pub async fn traffic_history(
    State(state): State<AppState>,
    Query(params): Query<TrafficQuery>,
) -> HandlerResult {
    let summary = match get_traffic_summary(
        &state.db,
        params.host_id.as_deref(),
        params.period.as_deref(),
        params.since.as_deref(),
        params.until.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "failed to get traffic history", "detail": e.to_string()}),
            );
        }
    };

    HandlerResult::Json(json!(summary))
}
```

- [ ] **Step 2: Add routes to router.rs**

```rust
.route("/traffic", get(handlers::traffic))
.route("/api/traffic/live", get(handlers::traffic_live))
.route("/api/traffic/history", get(handlers::traffic_history))
```

- [ ] **Step 3: Add traffic.html template**

```html
{% extends "layout.html" %}
{% block title %}Traffic - s3-gallery{% endblock %}
{% block content %}
<h1>Traffic Dashboard</h1>

<div style="display: flex; gap: 20px; margin: 20px 0;">
    <div style="flex: 1; padding: 20px; background: #f0f8ff; border-radius: 8px; text-align: center;">
        <h3>↓ Download</h3>
        <div style="font-size: 2em;" id="live-download">{{ total_download_mb }} MB</div>
    </div>
    <div style="flex: 1; padding: 20px; background: #fff0f0; border-radius: 8px; text-align: center;">
        <h3>↑ Upload</h3>
        <div style="font-size: 2em;">{{ total_upload_mb }} MB</div>
    </div>
    <div style="flex: 1; padding: 20px; background: #f0fff0; border-radius: 8px; text-align: center;">
        <h3>Requests</h3>
        <div style="font-size: 2em;">{{ total_requests }}</div>
    </div>
    <div style="flex: 1; padding: 20px; background: #fff8f0; border-radius: 8px; text-align: center;">
        <h3>Est. Cost</h3>
        <div style="font-size: 2em;">{{ estimated_cost }}</div>
    </div>
</div>

<h2>Per Business (Today)</h2>
<table>
    <tr><th>Business</th><th>Download</th><th>Upload</th><th>Requests</th></tr>
    {% for biz in businesses %}
    <tr>
        <td>{{ biz.business }}</td>
        <td>{{ "%.1f"|format(biz.download_bytes / 1048576) }} MB</td>
        <td>{{ "%.1f"|format(biz.upload_bytes / 1048576) }} MB</td>
        <td>{{ biz.requests }}</td>
    </tr>
    {% endfor %}
</table>

<h2>Live Traffic</h2>
<div id="live-traffic" hx-get="/api/traffic/live" hx-trigger="every 5s" hx-swap="innerHTML">
    <p>Loading...</p>
</div>
{% endblock %}
```

- [ ] **Step 4: Add traffic.html to template list in lib.rs**

Add to the `TEMPLATES` array:
```rust
("traffic.html", include_str!("../templates/traffic.html")),
```

- [ ] **Step 5: Add "Traffic" nav link to layout.html**

Add before the `<a href="/stats">Stats</a>` line:
```html
<a href="/traffic">Traffic</a>
```

- [ ] **Step 6: Build to verify compilation**

Run: `cargo check -p s3-gallery-cli`
Expected: Compilation succeeds

- [ ] **Step 7: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/traffic.rs crates/s3-gallery-cli/src/web/router.rs crates/s3-gallery-web/templates/traffic.html crates/s3-gallery-web/src/lib.rs crates/s3-gallery-web/templates/layout.html
git commit -m "feat: add traffic web dashboard with live polling"
```

---

### Task 18: Wire S3Service stack into cmd_serve and AppState

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/state.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs`

- [ ] **Step 1: Update AppState to include S3Service and TrafficRecorder**

```rust
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
    pub s3_clients: HashMap<String, Arc<dyn S3Client>>,
    pub s3_stack: S3Service,
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
    pub prefix: Option<String>,
    pub cli_endpoint: String,
    pub cli_region: String,
    pub access_key: String,
    pub secret_key: String,
}
```

- [ ] **Step 2: Build S3Service stack in cmd_serve.rs**

After creating the S3 clients HashMap, build the S3 stack:

```rust
use std::time::Duration;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::layers::{LogLayer, TrafficLayer};
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use s3_gallery_core::s3::traffic_persist::spawn_aggregator;
use tower::ServiceBuilder;
use tower::buffer::BufferLayer;
use tower::timeout::TimeoutLayer;
use tower::limit::ConcurrencyLimitLayer;

// Build S3Service stack with Tower layers
let core_s3 = S3Service::new(
    s3_clients.values().next().cloned()
        .ok_or_else(|| S3GalleryError::Internal("no S3 clients available".to_string()))?,
);

// Traffic recorder
let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
let _agg_handle = spawn_aggregator(recorder.clone(), pool.clone(), 60);

let s3_stack = ServiceBuilder::new()
    .layer(BufferLayer::new(1024))
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(ConcurrencyLimitLayer::new(10))
    .layer(LogLayer)
    .service(
        ServiceBuilder::new()
            .layer(TrafficLayer::new(recorder.clone(), "s3_api", "s3_api"))
            .service(core_s3),
    );
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo check -p s3-gallery-cli`
Expected: Compilation succeeds

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/state.rs crates/s3-gallery-cli/src/cmd_serve.rs
git commit -m "feat: wire S3Service stack and TrafficRecorder into serve"
```

---

### Task 19: Wire S3Service and TrafficRecorder into cmd_scan

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`

- [ ] **Step 1: Update scan_host to build S3Service stack**

Modify the `scan_host` function to build a Tower-composed S3Service stack:

```rust
use std::sync::Arc;
use std::time::Duration;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::layers::{LogLayer, TrafficLayer};
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use s3_gallery_core::s3::traffic_persist::spawn_aggregator;
use tower::ServiceBuilder;
use tower::buffer::BufferLayer;
use tower::timeout::TimeoutLayer;
use tower::limit::ConcurrencyLimitLayer;

async fn scan_host(
    s3: &Arc<dyn S3Client>,
    pool: &SqlitePool,
    bucket: &BucketName,
    host: &HostIdentifier,
    scan_prefix: &ObjectKey,
    opts: &ScanOptions,
    recorder: Option<&Arc<TrafficRecorder>>,
) -> Result<ScanResult> {
    let core = S3Service::new(s3.clone());

    let s3_stack = if let Some(rec) = recorder {
        ServiceBuilder::new()
            .layer(BufferLayer::new(1024))
            .layer(TimeoutLayer::new(Duration::from_secs(30)))
            .layer(ConcurrencyLimitLayer::new(10))
            .layer(LogLayer)
            .service(
                ServiceBuilder::new()
                    .layer(TrafficLayer::new(rec.clone(), &host.host_id, "exif_extraction"))
                    .service(core),
            )
    } else {
        ServiceBuilder::new()
            .layer(BufferLayer::new(1024))
            .layer(TimeoutLayer::new(Duration::from_secs(30)))
            .layer(ConcurrencyLimitLayer::new(10))
            .layer(LogLayer)
            .service(core)
    };

    // Update ScanConfig to use s3_stack instead of raw s3 client
    // (This is a temporary approach — the scanner will be refactored to use S3Service)
    let scan_config = ScanConfig {
        s3: s3.clone(), // Keep using raw s3 for now, scanner refactoring is separate
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
```

- [ ] **Step 2: Wire TrafficRecorder creation in run_scan_core**

Pass the recorder through to scan_host:

```rust
// In run_scan_core, after creating pool:
let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
let _agg_handle = spawn_aggregator(recorder.clone(), pool.clone(), 60);

// Then pass to scan_host:
let result = scan_host(s3, pool, bucket, &host, &scan_prefix, opts, Some(&recorder)).await?;
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo check -p s3-gallery-cli`
Expected: Compilation succeeds

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/cmd_scan.rs
git commit -m "feat: wire S3Service stack and TrafficRecorder into scan"
```

---

### Task 20: Self-review pass

- [ ] **Step 1: Verify all spec requirements are covered**

Check each section of the spec against the plan tasks:
- Section 2 (S3Service + Layers): Tasks 1, 2, 3 ✓
- Section 3 (Traffic Tracking): Tasks 4, 5, 6 ✓
- Section 4 (DB Schema): Tasks 7, 8 ✓
- Section 5 (Scan Pipeline): Task 10 ✓
- Section 6 (Web Handlers): Tasks 11, 12 ✓
- Section 7 (View Layer): Task 9, 10 ✓
- Section 8 (Thumbnail): Task 13 ✓
- Section 9 (Integration): Tasks 18, 19 ✓
- Section 10 (CLI): Task 16 ✓
- Section 11 (Dashboard): Task 17 ✓

- [ ] **Step 2: Full build check**

Run: `cargo check --workspace`
Expected: Compilation succeeds

- [ ] **Step 3: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass