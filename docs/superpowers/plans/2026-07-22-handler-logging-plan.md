# Handler Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `tracing::info!` and `tracing::error!` calls to all 13 web handlers in the serve command.

**Architecture:** Each handler gets entry logging (key params), success logging (data volume), and error logging (full error before response). No new dependencies — `tracing` crate is already available.

**Tech Stack:** Rust, Axum, tracing crate

---

### Task 1: dashboard handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/dashboard.rs`

- [ ] **Step 1: Add entry log**

Replace:
```rust
pub async fn dashboard(_state: State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "s3-gallery-web"
    }))
}
```

With:
```rust
pub async fn dashboard(_state: State<AppState>) -> impl IntoResponse {
    tracing::info!("dashboard: rendering");
    Json(json!({
        "status": "ok",
        "service": "s3-gallery-web"
    }))
}
```

- [ ] **Step 2: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/dashboard.rs
git commit -m "feat: add logging to dashboard handler"
```

---

### Task 2: browse handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/browse.rs`

- [ ] **Step 1: Add entry and success logs**

After `let sort_order = parse_sort_order(params.sort_order.as_deref());` (line ~172), add:
```rust
    tracing::info!(handler = "browse", path = %path, sort_by = %sort_field, sort_order = %sort_order, "listing directory");
```

After `if is_htmx { "browse_table.html" } else { "browse.html" };` (line ~238), add:
```rust
    tracing::info!(handler = "browse", path = %path, entries = %entry_views.len(), template = %template_name, "directory listed");
```

- [ ] **Step 2: Add error logs**

Replace the error response blocks (there are 2) to log before returning:

In the `list_directory` error path (line ~181-191):
```rust
        Err(e) => {
            tracing::error!(handler = "browse", path = %path, sort_by = %sort_field, error = %e, "failed to list directory");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to list directory",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
```

In the `render_template` error paths (inside `render_template` function), add logging to the existing `map_err` closures:

For template not found (line ~127-137):
```rust
        .map_err(|e| {
            tracing::error!(handler = "browse", template = %template_name, error = %e, "template not found");
            Box::new(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "template not found",
                        "detail": e.to_string()
                    })),
                )
                    .into_response(),
            )
        })?;
```

For template rendering failed (line ~140-151):
```rust
    let html = template.render(context).map_err(|e| {
        tracing::error!(handler = "browse", template = %template_name, error = %e, "template rendering failed");
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "template rendering failed",
                    "detail": e.to_string()
                })),
            )
                .into_response(),
        )
    })?;
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/browse.rs
git commit -m "feat: add logging to browse handler"
```

---

### Task 3: gallery handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/gallery.rs`

- [ ] **Step 1: Add entry log**

After `let limit = i64::from(ITEMS_PER_PAGE) + 1;` (line ~108), add:
```rust
    tracing::info!(handler = "gallery", page = %page, "serving gallery page");
```

- [ ] **Step 2: Add success log**

After `let template_name = if is_htmx { ... } else { ... };` (line ~178-182), add:
```rust
    tracing::info!(handler = "gallery", page = %page, items = %items.len(), has_more = %has_more, template = %template_name, "gallery rendered");
```

- [ ] **Step 3: Add error log**

Replace the query error path (line ~135-145):
```rust
        Err(e) => {
            tracing::error!(handler = "gallery", page = %page, error = %e, "failed to query gallery images");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to query gallery images",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/gallery.rs
git commit -m "feat: add logging to gallery handler"
```

---

### Task 4: search handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/search.rs`

- [ ] **Step 1: Add entry log**

After `let is_htmx = ...` (line ~90-93), add:
```rust
    let query = params.q.as_deref().unwrap_or("");
    let tag = params.tag.as_deref().unwrap_or("");
    tracing::info!(handler = "search", query = %query, tag = %tag, "search requested");
```

- [ ] **Step 2: Add success log**

After `let results: Vec<serde_json::Value> = ...` (line ~137), add:
```rust
    tracing::info!(handler = "search", query = %query, tag = %tag, results = %results.len(), "search completed");
```

- [ ] **Step 3: Add error logs**

Replace the `search_by_name` error path (line ~103-111):
```rust
                Err(e) => {
                    tracing::error!(handler = "search", query = %query_trimmed, error = %e, "search by name failed");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "search failed",
                            "detail": e.to_string()
                        })),
                    )
                        .into_response();
                }
```

Replace the `search_by_tag` error path (line ~120-130):
```rust
                Err(e) => {
                    tracing::error!(handler = "search", tag = %tag_trimmed, error = %e, "search by tag failed");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "search by tag failed",
                            "detail": e.to_string()
                        })),
                    )
                        .into_response();
                }
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/search.rs
git commit -m "feat: add logging to search handler"
```

---

### Task 5: timeline handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/timeline.rs`

- [ ] **Step 1: Read current file**

- [ ] **Step 2: Add entry log at function start, success log after successful fetch, and error log before error responses**

Entry log:
```rust
    tracing::info!(handler = "timeline", "serving timeline");
```

Success log after `Ok(entries)`:
```rust
    tracing::info!(handler = "timeline", entries = %entries.len(), "timeline rendered");
```

Error log before error responses:
```rust
    tracing::error!(handler = "timeline", error = %e, "failed to get timeline");
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/timeline.rs
git commit -m "feat: add logging to timeline handler"
```

---

### Task 6: tags handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/tags.rs`

- [ ] **Step 1: Read current file**

- [ ] **Step 2: Add entry, success, and error logs**

Entry log:
```rust
    tracing::info!(handler = "tags", "listing tags");
```

Success log:
```rust
    tracing::info!(handler = "tags", tags = %tags.len(), "tags listed");
```

Error log:
```rust
    tracing::error!(handler = "tags", error = %e, "failed to list tags");
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/tags.rs
git commit -m "feat: add logging to tags handler"
```

---

### Task 7: file_detail handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/file_detail.rs`

- [ ] **Step 1: Add entry log**

After `Path(key): Path<String>` (line ~99), add:
```rust
    tracing::info!(handler = "file_detail", key = %key, "serving file detail");
```

- [ ] **Step 2: Add success log**

After `let has_thumbnail = ...` (line ~157), add:
```rust
    tracing::info!(handler = "file_detail", key = %key, namespaces = %metadata_by_namespace.len(), has_thumbnail = %has_thumbnail, "file detail rendered");
```

- [ ] **Step 3: Add error logs**

Replace the `get_by_key` error paths (line ~107-125):
```rust
            return match e {
                S3GalleryError::NotFound(_) => {
                    tracing::error!(handler = "file_detail", key = %key, error = %e, "file not found");
                    (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "error": "file not found",
                            "detail": format!("No file with key: {key}")
                        })),
                    )
                        .into_response()
                }
                _ => {
                    tracing::error!(handler = "file_detail", key = %key, error = %e, "database error fetching file");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "database error",
                            "detail": e.to_string()
                        })),
                    )
                        .into_response()
                }
            };
```

Replace the `get_by_file_key` error path (line ~131-141):
```rust
        Err(e) => {
            tracing::error!(handler = "file_detail", key = %key, error = %e, "failed to fetch metadata");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to fetch metadata",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/file_detail.rs
git commit -m "feat: add logging to file_detail handler"
```

---

### Task 8: duplicates handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/duplicates.rs`

- [ ] **Step 1: Read current file**

- [ ] **Step 2: Add entry, success, and error logs**

Entry log:
```rust
    tracing::info!(handler = "duplicates", "finding duplicates");
```

Success log:
```rust
    tracing::info!(handler = "duplicates", groups = %groups.len(), "duplicates found");
```

Error log:
```rust
    tracing::error!(handler = "duplicates", error = %e, "failed to find duplicates");
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/duplicates.rs
git commit -m "feat: add logging to duplicates handler"
```

---

### Task 9: stats handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/stats.rs`

- [ ] **Step 1: Read current file**

- [ ] **Step 2: Add entry, success, and error logs**

Entry log:
```rust
    tracing::info!(handler = "stats", "computing stats");
```

Success log:
```rust
    tracing::info!(handler = "stats", total_files = %stats.total_files, total_size = %stats.total_size, "stats computed");
```

Error log:
```rust
    tracing::error!(handler = "stats", error = %e, "failed to get stats");
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/stats.rs
git commit -m "feat: add logging to stats handler"
```

---

### Task 10: settings handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/settings.rs`

- [ ] **Step 1: Read current file**

- [ ] **Step 2: Add entry log**

```rust
    tracing::info!(handler = "settings", "rendering settings");
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/settings.rs
git commit -m "feat: add logging to settings handler"
```

---

### Task 11: thumbnail handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs`

- [ ] **Step 1: Add entry log**

After `Path(key): Path<String>` (line ~23), add:
```rust
    tracing::info!(handler = "thumbnail", key = %key, "serving thumbnail");
```

- [ ] **Step 2: Add cache hit log**

After `Ok(entry)` (line ~29), add:
```rust
    tracing::info!(handler = "thumbnail", key = %key, cache = "hit", "thumbnail served from cache");
```

- [ ] **Step 3: Add cache miss log**

After `Err(S3GalleryError::NotFound(_))` (line ~40), add:
```rust
    tracing::info!(handler = "thumbnail", key = %key, cache = "miss", "thumbnail not cached, fetching from S3");
```

- [ ] **Step 4: Add success log for generated thumbnail**

After `Ok(data)` (line ~76), add:
```rust
    tracing::info!(handler = "thumbnail", key = %key, cache = "generated", size = %data.len(), "thumbnail generated and cached");
```

- [ ] **Step 5: Add error logs**

Replace the DB error path (line ~43-54):
```rust
        Err(e) => {
            tracing::error!(handler = "thumbnail", key = %key, error = %e, "database error fetching thumbnail");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"database error\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response();
        }
```

Replace the invalid key error path (line ~66-75):
```rust
        Err(e) => {
            tracing::error!(handler = "thumbnail", key = %key, error = %e, "invalid thumbnail key");
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"invalid key\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response();
        }
```

Replace the S3 error paths (line ~87-108):
```rust
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::error!(handler = "thumbnail", key = %key, "thumbnail source not found on S3");
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"file not found\",\"detail\":\"No file with key: {key}\"}}"
                )
                .into_bytes(),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(handler = "thumbnail", key = %key, error = %e, "thumbnail generation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"thumbnail generation failed\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response()
        }
```

- [ ] **Step 6: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 7: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/thumbnail.rs
git commit -m "feat: add logging to thumbnail handler"
```

---

### Task 12: download handler

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/download.rs`

- [ ] **Step 1: Add entry log**

After `Path(key): Path<String>` (line ~42), add:
```rust
    tracing::info!(handler = "download", key = %key, "download requested");
```

- [ ] **Step 2: Add success log**

After `Ok(data)` (line ~92), add:
```rust
    tracing::info!(handler = "download", key = %key, size = %data.len(), "file downloaded");
```

- [ ] **Step 3: Add error logs**

Replace the `get_by_key` error paths (line ~48-70):
```rust
        Err(e) => {
            return match e {
                S3GalleryError::NotFound(_) => {
                    tracing::error!(handler = "download", key = %key, "file not found in database");
                    (
                        StatusCode::NOT_FOUND,
                        [("content-type", "application/json")],
                        format!(
                            "{{\"error\":\"file not found\",\"detail\":\"No file with key: {key}\"}}"
                        )
                        .into_bytes(),
                    )
                        .into_response()
                }
                _ => {
                    tracing::error!(handler = "download", key = %key, error = %e, "database error");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [("content-type", "application/json")],
                        format!(
                            "{{\"error\":\"database error\",\"detail\":\"{}\"}}",
                            e.to_string().replace('"', "\\\"")
                        )
                        .into_bytes(),
                    )
                        .into_response()
                }
            };
        }
```

Replace the invalid key error path (line ~77-88):
```rust
        Err(e) => {
            tracing::error!(handler = "download", key = %key, error = %e, "invalid object key");
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"invalid key\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response();
        }
```

Replace the S3 error paths (line ~108-131):
```rust
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::error!(handler = "download", key = %key, "file not found on S3");
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"file not found\",\"detail\":\"No file with key: {key}\"}}"
                )
                .into_bytes(),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(handler = "download", key = %key, error = %e, "S3 fetch failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"S3 fetch failed\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response()
        }
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/download.rs
git commit -m "feat: add logging to download handler"
```

---

### Task 13: Final build and verify

**Files:**
- Build: entire project

- [ ] **Step 1: Build CLI crate**

Run: `cargo build -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -p s3-gallery-cli 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/
git commit -m "feat: add logging to all remaining serve handlers"
```