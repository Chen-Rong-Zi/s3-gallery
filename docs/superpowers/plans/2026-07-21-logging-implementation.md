# Configurable Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-module configurable logging with `tracing` targets, centered on a `LoggedS3Client` decorator.

**Architecture:** A `LoggedS3Client` wrapper implements `S3Client` and delegates all 8 methods to an inner `Arc<dyn S3Client>`, logging each call with timing, bucket, key, and result. Other modules (scan, lock, remote_view, thumbnail) add `tracing` calls directly at key operation boundaries. Per-feature-path control via `RUST_LOG` with `tracing-subscriber`'s `EnvFilter`.

**Tech Stack:** `tracing 0.1`, `tracing-subscriber 0.3`, `EnvFilter`

---

### Task 1: Create `LoggedS3Client` wrapper

**Files:**
- Create: `crates/ossgalley-core/src/s3/logged.rs`
- Modify: `crates/ossgalley-core/src/s3/mod.rs` — add `pub mod logged;`

- [ ] **Step 1: Create `crates/ossgalley-core/src/s3/logged.rs`**

This file implements `S3Client` for `LoggedS3Client`, wrapping every method with timing + structured logging.

```rust
//! Logging wrapper around an S3Client.
//!
//! LoggedS3Client delegates every method to an inner `Arc<dyn S3Client>` and
//! logs each call with timing, bucket, key, and result.  Each method uses its
//! own `tracing` target (e.g. `ossgalley::s3::get_object`) so log output can
//! be controlled per-method via `RUST_LOG`.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{BucketName, ObjectKey};

use super::client::{ObjectMetadata, ObjectSummary, S3Client};

/// Logging wrapper around an S3Client.
///
/// Wraps any `Arc<dyn S3Client>` and logs every S3 operation with:
/// - target (per-method, e.g. `ossgalley::s3::get_object`)
/// - bucket, key
/// - elapsed time in milliseconds
/// - success (debug level) or failure (warn level)
/// - method-specific fields (size, count, etc.)
#[derive(Debug, Clone)]
pub struct LoggedS3Client {
    inner: Arc<dyn S3Client>,
}

impl LoggedS3Client {
    /// Wrap an S3Client with logging.
    #[must_use]
    pub fn new(inner: Arc<dyn S3Client>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl S3Client for LoggedS3Client {
    async fn list_objects(
        &self,
        bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ObjectSummary>> {
        let start = Instant::now();
        let result = self.inner.list_objects(bucket, prefix).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(objs) => tracing::debug!(
                target: "ossgalley::s3::list_objects",
                bucket = %bucket,
                prefix = %prefix,
                count = objs.len(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 ListObjects"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::list_objects",
                bucket = %bucket,
                prefix = %prefix,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 ListObjects failed"
            ),
        }
        result
    }

    async fn head_object(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<ObjectMetadata> {
        let start = Instant::now();
        let result = self.inner.head_object(bucket, key).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(meta) => tracing::debug!(
                target: "ossgalley::s3::head_object",
                bucket = %bucket,
                key = %key,
                etag = %meta.etag,
                size = meta.size.as_u64(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 HeadObject"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::head_object",
                bucket = %bucket,
                key = %key,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 HeadObject failed"
            ),
        }
        result
    }

    async fn get_object(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<Vec<u8>> {
        let start = Instant::now();
        let result = self.inner.get_object(bucket, key).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(data) => tracing::debug!(
                target: "ossgalley::s3::get_object",
                bucket = %bucket,
                key = %key,
                size = data.len(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 GetObject"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::get_object",
                bucket = %bucket,
                key = %key,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 GetObject failed"
            ),
        }
        result
    }

    async fn get_object_range(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        start_byte: u64,
        end_byte: u64,
    ) -> Result<Vec<u8>> {
        let start = Instant::now();
        let result = self.inner.get_object_range(bucket, key, start_byte, end_byte).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(data) => tracing::debug!(
                target: "ossgalley::s3::get_object_range",
                bucket = %bucket,
                key = %key,
                range = %format!("{}-{}", start_byte, end_byte),
                size = data.len(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 GetObjectRange"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::get_object_range",
                bucket = %bucket,
                key = %key,
                range = %format!("{}-{}", start_byte, end_byte),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 GetObjectRange failed"
            ),
        }
        result
    }

    async fn put_object(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<()> {
        let start = Instant::now();
        let result = self.inner.put_object(bucket, key, body).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(()) => tracing::debug!(
                target: "ossgalley::s3::put_object",
                bucket = %bucket,
                key = %key,
                body_size = body.len(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 PutObject"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::put_object",
                bucket = %bucket,
                key = %key,
                body_size = body.len(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 PutObject failed"
            ),
        }
        result
    }

    async fn put_object_if_none_match(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool> {
        let start = Instant::now();
        let result = self.inner.put_object_if_none_match(bucket, key, body).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(created) => {
                if *created {
                    tracing::debug!(
                        target: "ossgalley::s3::put_object_if_none_match",
                        bucket = %bucket,
                        key = %key,
                        body_size = body.len(),
                        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                        created = true,
                        "S3 PutObjectIfNoneMatch — created"
                    );
                } else {
                    tracing::info!(
                        target: "ossgalley::s3::put_object_if_none_match",
                        bucket = %bucket,
                        key = %key,
                        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                        created = false,
                        "S3 PutObjectIfNoneMatch — object already exists"
                    );
                }
            }
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::put_object_if_none_match",
                bucket = %bucket,
                key = %key,
                body_size = body.len(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 PutObjectIfNoneMatch failed"
            ),
        }
        result
    }

    async fn delete_object(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<()> {
        let start = Instant::now();
        let result = self.inner.delete_object(bucket, key).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(()) => tracing::debug!(
                target: "ossgalley::s3::delete_object",
                bucket = %bucket,
                key = %key,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 DeleteObject"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::delete_object",
                bucket = %bucket,
                key = %key,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 DeleteObject failed"
            ),
        }
        result
    }

    async fn object_exists(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<bool> {
        let start = Instant::now();
        let result = self.inner.object_exists(bucket, key).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(exists) => tracing::debug!(
                target: "ossgalley::s3::object_exists",
                bucket = %bucket,
                key = %key,
                exists = *exists,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                "S3 ObjectExists"
            ),
            Err(e) => tracing::warn!(
                target: "ossgalley::s3::object_exists",
                bucket = %bucket,
                key = %key,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                error = %e,
                "S3 ObjectExists failed"
            ),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use crate::types::BucketName;

    #[tokio::test]
    async fn test_logged_client_delegates_get_object() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        // Put via inner, get via logged — should return same data
        inner.put_object(&bucket, &key, b"hello").await?;
        let data = logged.get_object(&bucket, &key).await?;
        assert_eq!(data, b"hello");
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_list_objects() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;

        inner.put_object(&bucket, &ObjectKey::new("a/1.txt")?, b"a").await?;
        inner.put_object(&bucket, &ObjectKey::new("a/2.txt")?, b"b").await?;

        let prefix = ObjectKey::new("a/")?;
        let objs = logged.list_objects(&bucket, &prefix).await?;
        assert_eq!(objs.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_head_object() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        inner.put_object(&bucket, &key, b"hello").await?;
        let meta = logged.head_object(&bucket, &key).await?;
        assert_eq!(meta.size.as_u64(), 5);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_get_object_range() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        inner.put_object(&bucket, &key, b"hello world").await?;
        let data = logged.get_object_range(&bucket, &key, 0, 5).await?;
        assert_eq!(data, b"hello");
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_put_object() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        logged.put_object(&bucket, &key, b"data").await?;
        assert!(inner.object_exists(&bucket, &key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_put_if_none_match() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        let created = logged.put_object_if_none_match(&bucket, &key, b"data").await?;
        assert!(created);

        let created2 = logged.put_object_if_none_match(&bucket, &key, b"data2").await?;
        assert!(!created2);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_delete_object() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        inner.put_object(&bucket, &key, b"data").await?;
        logged.delete_object(&bucket, &key).await?;
        assert!(!inner.object_exists(&bucket, &key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_object_exists() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        assert!(!logged.object_exists(&bucket, &key).await?);
        inner.put_object(&bucket, &key, b"data").await?;
        assert!(logged.object_exists(&bucket, &key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_propagates_errors() -> Result<()> {
        let inner = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let logged = LoggedS3Client::new(inner.clone());
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("nonexistent.txt")?;

        let err = logged.get_object(&bucket, &key).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
        Ok(())
    }
}
```

- [ ] **Step 2: Register the module in `s3/mod.rs`**

Add `pub mod logged;` to the existing module declarations.

Edit `crates/ossgalley-core/src/s3/mod.rs`. The current content is:
```rust
pub mod client;
pub mod config;
pub mod lock;
pub mod mock;
```

Append `pub mod logged;` (alphabetically between `lock` and `mock`):

```rust
pub mod client;
pub mod config;
pub mod lock;
pub mod logged;
pub mod mock;
```

- [ ] **Step 3: Run tests to verify LoggedS3Client works**

```bash
cd /Users/macbook/Project/ossgalley
cargo test -p ossgalley-core -- s3::logged 2>&1
```

Expected: All 9 tests pass (8 delegation + 1 error propagation).

- [ ] **Step 4: Commit**

```bash
git add crates/ossgalley-core/src/s3/logged.rs crates/ossgalley-core/src/s3/mod.rs
git commit -m "feat: add LoggedS3Client wrapper with per-method tracing

Decorator pattern wrapping Arc<dyn S3Client> with structured logging
for all 8 S3 methods. Each method uses target ossgalley::s3::<method>
for per-method RUST_LOG control.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Add tracing calls to `scan/scanner.rs`

**Files:**
- Modify: `crates/ossgalley-core/src/scan/scanner.rs`

- [ ] **Step 1: Add `use tracing;` to the imports**

Edit `crates/ossgalley-core/src/scan/scanner.rs`. After the existing use statements, add no new import needed — `tracing` macros are available as a dependency. The file already has `use std::time::Instant;` for the scan timer.

- [ ] **Step 2: Add tracing calls at key lifecycle points**

Insert the following logging calls into `run_scan()`:

**After lock acquisition** (after line ~70, after `let guard = acquire_lock(...)`):
```rust
tracing::info!(
    target: "ossgalley::scan",
    prefix = %config.prefix,
    client_id = %config.client_id,
    "Scan started"
);
```

**After listing objects** (after line ~77, after `let s3_objects = ...`):
```rust
tracing::info!(
    target: "ossgalley::scan",
    total_objects = s3_objects.len(),
    "Objects listed from S3"
);
```

**After diff** (after line ~83, after `let diff = diff_objects(...)`):
```rust
tracing::debug!(
    target: "ossgalley::scan",
    new = diff.new_objects.len(),
    changed = diff.changed_objects.len(),
    deleted = diff.deleted_keys.len(),
    "Diff completed"
);
```

**Before returning** (before `Ok(ScanResult {...})`, after `guard.release().await?;`):
```rust
tracing::info!(
    target: "ossgalley::scan",
    total_files = s3_objects.len(),
    new_files = new_files,
    changed_files = changed_files,
    deleted_files = diff.deleted_keys.len(),
    duration_secs = duration.as_secs_f64(),
    "Scan completed"
);
```

- [ ] **Step 3: Verify tests still pass**

```bash
cd /Users/macbook/Project/ossgalley
cargo test -p ossgalley-core -- scan::scanner 2>&1
```

Expected: All scanner tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/ossgalley-core/src/scan/scanner.rs
git commit -m "feat: add tracing to scan module

Log scan lifecycle events (start, list, diff, completion) with
target ossgalley::scan for per-module RUST_LOG control.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Add tracing calls to `s3/lock.rs`

**Files:**
- Modify: `crates/ossgalley-core/src/s3/lock.rs`

- [ ] **Step 1: Add tracing calls to lock operations**

**In `acquire_lock()`** — after successful acquisition (after `if acquired {`):
```rust
tracing::info!(
    target: "ossgalley::lock",
    lock_key = %lock_key,
    client_id = %client_id,
    "Lock acquired"
);
```

**In `acquire_lock()`** — in the `else` branch (LockContention):
```rust
tracing::warn!(
    target: "ossgalley::lock",
    lock_key = %lock_key,
    client_id = %client_id,
    "Lock contention — held by another client"
);
```

**In `LockGuard::release()`** — before `self.s3.delete_object(...)`:
```rust
tracing::debug!(
    target: "ossgalley::lock",
    lock_key = %self.lock_key,
    client_id = %self.client_id,
    "Lock released"
);
```

**In `LockGuard::renew()`** — after `self.s3.put_object(...)`:
```rust
tracing::debug!(
    target: "ossgalley::lock",
    lock_key = %self.lock_key,
    client_id = %self.client_id,
    "Lock renewed"
);
```

- [ ] **Step 2: Verify tests still pass**

```bash
cd /Users/macbook/Project/ossgalley
cargo test -p ossgalley-core -- lock 2>&1
```

Expected: All lock tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/ossgalley-core/src/s3/lock.rs
git commit -m "feat: add tracing to lock module

Log lock acquire/release/renew/contention events with
target ossgalley::lock for per-module RUST_LOG control.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Add tracing calls to `view/remote.rs`

**Files:**
- Modify: `crates/ossgalley-core/src/view/remote.rs`

- [ ] **Step 1: Add tracing calls to RemoteView methods**

**In `fetch_file_content()`** — before `return`:
```rust
// After the S3 call returns Ok:
tracing::debug!(
    target: "ossgalley::remote_view",
    key = %key,
    size = data.len(),
    "Fetched file content from S3"
);
```

Wrap the call to return the data with logging:
```rust
pub async fn fetch_file_content(&self, key: &ObjectKey) -> Result<Vec<u8>> {
    let data = self.s3.get_object(&self.bucket, key).await?;
    tracing::debug!(
        target: "ossgalley::remote_view",
        key = %key,
        size = data.len(),
        "Fetched file content from S3"
    );
    Ok(data)
}
```

**In `fetch_thumbnail()`** — cache hit:
```rust
// In the Ok(entry) branch:
tracing::debug!(
    target: "ossgalley::remote_view",
    key = %key,
    "Thumbnail cache hit"
);
```

**In `fetch_thumbnail()`** — after S3 fetch + generation (before `Ok(thumbnail)`):
```rust
tracing::info!(
    target: "ossgalley::remote_view",
    key = %key,
    "Thumbnail generated from S3"
);
```

**In `fetch_byte_range()`** — before returning:
```rust
tracing::debug!(
    target: "ossgalley::remote_view",
    key = %key,
    start = start,
    end = end,
    size = data.len(),
    "Fetched byte range from S3"
);
```

**In `fetch_object_exists()`** — before returning:
```rust
tracing::debug!(
    target: "ossgalley::remote_view",
    key = %key,
    exists = exists,
    "Checked object existence on S3"
);
```

- [ ] **Step 2: Verify tests still pass**

```bash
cd /Users/macbook/Project/ossgalley
cargo test -p ossgalley-core -- remote 2>&1
```

Expected: All remote view tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/ossgalley-core/src/view/remote.rs
git commit -m "feat: add tracing to remote_view module

Log file content fetch, thumbnail fetch/cache, byte range, and
object existence checks with target ossgalley::remote_view.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Add tracing calls to `thumbnail/generator.rs`

**Files:**
- Modify: `crates/ossgalley-core/src/thumbnail/generator.rs`

- [ ] **Step 1: Add tracing calls to thumbnail generation**

**In `generate_thumbnail()`** — at the start:
```rust
tracing::debug!(
    target: "ossgalley::thumbnail",
    input_size = data.len(),
    "Generating thumbnail"
);
```

**In `generate_thumbnail()`** — after successful generation (before `Ok(output)`):
```rust
tracing::debug!(
    target: "ossgalley::thumbnail",
    output_size = output.len(),
    "Thumbnail generated successfully"
);
```

**In `generate_thumbnail()`** — the error path is already handled by `?`, so the error is logged at the call site. No separate warn! needed here.

**In `ThumbnailCache::get_or_generate()`** — cache hit:
```rust
// In the if let Some(entry) = ... branch:
tracing::debug!(
    target: "ossgalley::thumbnail",
    key = %key,
    "Thumbnail cache hit"
);
```

**In `ThumbnailCache::get_or_generate()`** — cache miss / new generation:
```rust
tracing::debug!(
    target: "ossgalley::thumbnail",
    key = %key,
    "Thumbnail cache miss — generating"
);
```

**In `ThumbnailCache::cache()`** — after successful cache:
```rust
tracing::debug!(
    target: "ossgalley::thumbnail",
    key = %key,
    size = data.len(),
    "Thumbnail cached"
);
```

- [ ] **Step 2: Verify tests still pass**

```bash
cd /Users/macbook/Project/ossgalley
cargo test -p ossgalley-core -- thumbnail 2>&1
```

Expected: All thumbnail tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/ossgalley-core/src/thumbnail/generator.rs
git commit -m "feat: add tracing to thumbnail module

Log thumbnail generation lifecycle (start, cache hit/miss, result)
with target ossgalley::thumbnail for per-module RUST_LOG control.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: Update `crates/ossgalley-web/src/main.rs` to use `EnvFilter`

**Files:**
- Modify: `crates/ossgalley-web/src/main.rs`

- [ ] **Step 1: Add `tracing_subscriber::EnvFilter` import**

The current main.rs has:
```rust
tracing_subscriber::fmt::init();
```

Replace with:
```rust
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    )
    .init();
```

Also add the `use` for `EnvFilter` — it's available under `tracing_subscriber`. Since `tracing_subscriber` is already a dependency, and `EnvFilter` is part of the `tracing-subscriber` crate (with the `env-filter` feature), check that the `env-filter` feature is enabled in the Cargo.toml.

Current `crates/ossgalley-web/Cargo.toml`:
```toml
tracing-subscriber = "0.3"
```

Change to:
```toml
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 2: Verify the web crate compiles**

```bash
cd /Users/macbook/Project/ossgalley
cargo check -p ossgalley-web 2>&1
```

Expected: Compilation succeeds.

- [ ] **Step 3: Commit**

```bash
git add crates/ossgalley-web/src/main.rs crates/ossgalley-web/Cargo.toml
git commit -m "feat: enable EnvFilter for RUST_LOG-based log control

Replace tracing_subscriber::fmt::init() with explicit EnvFilter
setup. Defaults to 'info' level when RUST_LOG is not set.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: Run full test suite to verify no regressions

**Files:** None (test-only)

- [ ] **Step 1: Run all tests in the workspace**

```bash
cd /Users/macbook/Project/ossgalley
cargo test --workspace 2>&1
```

Expected: All tests pass (no regressions). If any fail, diagnose and fix.

- [ ] **Step 2: Run clippy to check for warnings**

```bash
cd /Users/macbook/Project/ossgalley
cargo clippy --workspace -- -D warnings 2>&1
```

Expected: No warnings.

- [ ] **Step 3: Final commit (if any fixes were needed)**

```bash
git add -A
git commit -m "chore: fix test/clippy issues after logging implementation

Co-Authored-By: Claude <noreply@anthropic.com>"
```