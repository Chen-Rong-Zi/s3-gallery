# Configurable Logging for ossgalley

> **Date:** 2026-07-21
> **Status:** Approved Design
> **Author:** ossgalley team

## Problem Statement

ossgalley currently has no logging instrumentation in its core crate. S3
interactions, scan operations, lock acquisition, thumbnail generation, and
remote view operations all execute silently, making it difficult to debug
issues, monitor performance, or understand the system's behavior in
production.

We need a logging system that:
1. Covers all S3 interaction points (get, put, list, head, delete, exists)
2. Covers other module boundaries (scan, lock, thumbnail, remote_view)
3. Can be enabled/disabled **per feature path** at runtime
4. Records timing information (duration per operation)
5. Distinguishes success vs failure at appropriate log levels
6. Is minimally invasive to existing code

## Design

### Approach: Decorator Pattern + `tracing` Targets

We use a `LoggedS3Client` wrapper that implements `S3Client` and delegates
all calls to an inner `Arc<dyn S3Client>`, adding structured `tracing` logs
around each method. Other modules (scan, lock, remote_view, thumbnail) add
`tracing` calls directly at their key operation boundaries.

Per-feature-path control is achieved through `tracing`'s built-in `target`
field, combined with `EnvFilter` via `RUST_LOG`.

### Architecture

```
┌─────────────────────────────────────────────────┐
│                  Application                     │
│                                                   │
│  ┌─────────────────────────────────────────────┐ │
│  │         LoggedS3Client                      │ │
│  │  ┌───────────────────────────────────────┐  │ │
│  │  │  each method: log start + result      │  │ │
│  │  │  target: ossgalley::s3::<method>      │  │ │
│  │  └───────────────────────────────────────┘  │ │
│  │              │ delegates to                  │ │
│  │  ┌───────────────────────────────────────┐  │ │
│  │  │  RealS3Client / MockS3Client          │  │ │
│  │  └───────────────────────────────────────┘  │ │
│  └─────────────────────────────────────────────┘ │
│                                                   │
│  ┌────────────────┐  ┌──────────────────┐        │
│  │ Scanner        │  │ Lock             │        │
│  │ target: scan   │  │ target: lock     │        │
│  └────────────────┘  └──────────────────┘        │
│  ┌────────────────┐  ┌──────────────────┐        │
│  │ RemoteView     │  │ ThumbnailCache   │        │
│  │ target: view   │  │ target: thumbnail│        │
│  └────────────────┘  └──────────────────┘        │
└─────────────────────────────────────────────────┘
```

### Target Namespace Convention

All ossgalley targets use the `ossgalley::` prefix:

| Target | Module | Coverage |
|--------|--------|----------|
| `ossgalley::s3::list_objects` | s3/logged.rs | S3 LIST |
| `ossgalley::s3::head_object` | s3/logged.rs | S3 HEAD |
| `ossgalley::s3::get_object` | s3/logged.rs | S3 GET |
| `ossgalley::s3::get_object_range` | s3/logged.rs | S3 GET Range |
| `ossgalley::s3::put_object` | s3/logged.rs | S3 PUT |
| `ossgalley::s3::put_object_if_none_match` | s3/logged.rs | S3 PUT If-None-Match |
| `ossgalley::s3::delete_object` | s3/logged.rs | S3 DELETE |
| `ossgalley::s3::object_exists` | s3/logged.rs | S3 HEAD (exists) |
| `ossgalley::scan` | scan/scanner.rs | Scan lifecycle |
| `ossgalley::lock` | s3/lock.rs | Lock acquire/release |
| `ossgalley::remote_view` | view/remote.rs | S3 fetch operations |
| `ossgalley::thumbnail` | thumbnail/generator.rs | Thumbnail generation |

### Log Levels

| Level | When to use |
|-------|-------------|
| `error!` | Unexpected failures that should never happen (not used in this design — use `warn!` for S3 errors, let the caller decide severity) |
| `warn!` | S3 operation failures, lock contention, thumbnail generation failures |
| `info!` | Module lifecycle events (scan start/end, lock acquired/released, thumbnail generated) |
| `debug!` | Normal S3 operations (successful get/put/list/delete), cache hits |
| `trace!` | Reserved for future use (not used initially) |

### LoggedS3Client: Per-Method Logging

Each of the 8 S3Client methods is wrapped identically:

1. Record `Instant::now()` before the call
2. Await the inner delegate
3. Calculate elapsed time
4. Log success at `debug!` level with: target, bucket, key, elapsed_ms, size
5. Log failure at `warn!` level with: target, bucket, key, elapsed_ms, error

```rust
// Pseudocode for each method
let start = Instant::now();
let result = self.inner.method(args).await;
let elapsed = start.elapsed();
match &result {
    Ok(data) => tracing::debug!(
        target: "ossgalley::s3::<method>",
        bucket = %bucket,
        key = %key,
        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
        // + method-specific fields (size, count, etc.)
        "<method description>"
    ),
    Err(e) => tracing::warn!(
        target: "ossgalley::s3::<method>",
        bucket = %bucket,
        key = %key,
        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
        error = %e,
        "<method description> failed"
    ),
}
result
```

### Module-Level Logging

#### Scanner (`scan/scanner.rs`)

```
▶ scan started  → info, fields: prefix, client_id
▶ objects listed → info, fields: total_count
▶ diff result    → debug, fields: new, changed, deleted
▶ scan completed → info, fields: duration_secs, total, new, changed, deleted
```

#### Lock (`s3/lock.rs`)

```
▶ lock acquired  → info, fields: lock_key, client_id
▶ lock contended → warn, fields: lock_key (acquire_lock returns LockContention)
▶ lock released  → debug, fields: lock_key
▶ lock renewed   → debug, fields: lock_key
```

#### RemoteView (`view/remote.rs`)

```
▶ fetch_file_content → debug, fields: key, size
▶ fetch_thumbnail (cache hit) → debug, fields: key
▶ fetch_thumbnail (S3 fetch) → info, fields: key
▶ fetch_byte_range → debug, fields: key, start, end
▶ fetch_object_exists → debug, fields: key, exists
```

#### Thumbnail Cache (`thumbnail/generator.rs`)

```
▶ generate_thumbnail start → debug, fields: input_size
▶ generate_thumbnail success → debug, fields: output_size
▶ generate_thumbnail failed → warn, fields: error
▶ cache hit → debug
▶ cache miss → debug (also logged at info level in RemoteView)
```

### Runtime Control

Users control log output via the `RUST_LOG` environment variable, which is
parsed by `tracing-subscriber`'s `EnvFilter`:

```bash
# Enable all ossgalley S3 debug logs
RUST_LOG=ossgalley::s3=debug cargo run

# Enable S3 list_objects only + scan info
RUST_LOG=ossgalley::s3::list_objects=debug,ossgalley::scan=info cargo run

# Enable everything except ossgalley (show only ossgalley warnings+)
RUST_LOG=info,ossgalley=warn cargo run

# Default (no RUST_LOG set) — info+ from all targets, same as current behavior
# (no RUST_LOG set)
```

The web crate's `main.rs` already initializes `tracing-subscriber`:
```rust
tracing_subscriber::fmt::init();
```

We need to update this to use `EnvFilter`:
```rust
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            // Default: show info+ from all targets (matching current behavior)
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    )
    .init();
```

### Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/ossgalley-core/src/s3/logged.rs` | **Create** | `LoggedS3Client` wrapper |
| `crates/ossgalley-core/src/s3/mod.rs` | Modify | Add `pub mod logged;` |
| `crates/ossgalley-core/src/scan/scanner.rs` | Modify | Add `tracing` calls |
| `crates/ossgalley-core/src/s3/lock.rs` | Modify | Add `tracing` calls |
| `crates/ossgalley-core/src/view/remote.rs` | Modify | Add `tracing` calls |
| `crates/ossgalley-core/src/thumbnail/generator.rs` | Modify | Add `tracing` calls |
| `crates/ossgalley-web/src/main.rs` | Modify | Use `EnvFilter` instead of plain `fmt::init()` |

### Testing

1. **Unit tests for `LoggedS3Client`**: Verify that logging does not change
   behavior — wrap a `MockS3Client`, call methods, verify results are
   identical to unwrapped calls.
2. **No test for log output**: Testing log output is fragile. We trust
   `tracing` to emit events correctly. The structural tests (correct
   delegation, error propagation) are sufficient.
3. **Existing tests must pass**: All 297+ existing tests must continue to
   pass. The `LoggedS3Client` is opt-in; existing code paths that use
   `MockS3Client` directly are unaffected.

### Future Considerations

- **File-based config**: If `RUST_LOG` becomes unwieldy, a YAML/TOML config
  file with per-target log levels can be added without changing the logging
  calls (just the subscriber initialization).
- **Structured JSON output**: `tracing-subscriber` supports JSON output for
  log aggregation. Switch by changing the subscriber, not the log calls.
- **Metrics**: The timing data in log events could be extracted into metrics
  (Prometheus histograms) in the future.