# Multi-host Handler Adaptation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adapt stats, timeline, duplicates, and tags pages to show data from all hosts instead of only the first host.

**Architecture:** Change view layer functions to accept `Option<&str>` for host_id. When `None`, omit the `WHERE host_id = ?` filter. Handlers pass `None` for all-host aggregation. Stats additionally gets a `get_all_host_stats` function for per-host breakdown.

**Tech Stack:** Rust, SQLite (sqlx), s3-gallery-core view layer, s3-gallery-cli web handlers, minijinja templates

**Estimated total time:** 60 min

---

### Task 1: View layer — stat.rs (Option<&str> + get_all_host_stats)

**Files:**
- Modify: `crates/s3-gallery-core/src/view/stat.rs`

- [ ] **Step 1: Change `get_stats` signature and SQL queries**

Change `get_stats` to accept `Option<&str>`:

```rust
pub async fn get_stats(db: &SqlitePool, host_id: Option<&str>) -> Result<FileStats> {
    let mut by_category = HashMap::new();
    let mut by_file_type = HashMap::new();

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

    // ... same pattern for all other queries ...
}
```

Apply the `if let Some(hid)` pattern to ALL 6 SQL queries in the function:
- `SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0`
- `SELECT SUM(size) FROM files WHERE host_id = ? AND is_deleted = 0`
- `SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 1`
- `SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0 AND metadata_state = 'extracted'`
- `SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0 AND metadata_state = 'pending'`
- `SELECT * FROM files WHERE host_id = ? AND is_deleted = 0`

Each query gets the `if let Some(hid)` treatment: when host_id is Some, bind it and add WHERE host_id = ?; when None, use the query without the host_id filter.

- [ ] **Step 2: Add `get_all_host_stats` function**

Add after `get_stats`:

```rust
/// Get per-host statistics and total aggregate.
///
/// Returns `(total_stats, vec_of_(host_id, stats))`.
///
/// # Errors
///
/// Returns an error if any database query fails.
pub async fn get_all_host_stats(db: &SqlitePool) -> Result<(FileStats, Vec<(String, FileStats)>)> {
    // Get total across all hosts
    let total = get_stats(db, None).await?;

    // Get distinct host_ids
    let host_ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT host_id FROM files WHERE is_deleted = 0 ORDER BY host_id"
    )
    .fetch_all(db)
    .await
    .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;

    let mut per_host = Vec::with_capacity(host_ids.len());
    for hid in &host_ids {
        let stats = get_stats(db, Some(hid)).await?;
        per_host.push((hid.clone(), stats));
    }

    Ok((total, per_host))
}
```

- [ ] **Step 3: Update tests**

Update `test_get_stats` to pass `Some("test-host")` instead of `"test-host"`:

```rust
let stats = get_stats(&pool, Some("test-host")).await?;
```

Update `test_get_stats_empty` similarly:

```rust
let stats = get_stats(&pool, Some("test-host")).await?;
```

Add a new test for `get_all_host_stats`:

```rust
#[tokio::test]
async fn test_get_all_host_stats() -> Result<()> {
    let (pool, _dir) = setup_test_db().await?;
    seed_test_files(&pool).await?;

    let (total, per_host) = get_all_host_stats(&pool).await?;

    // Total should match the single host
    assert_eq!(total.total_files, 3);
    assert_eq!(total.deleted_files, 1);

    // Per-host should have one entry
    assert_eq!(per_host.len(), 1);
    assert_eq!(per_host[0].0, "test-host");
    assert_eq!(per_host[0].1.total_files, 3);

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify**

```bash
cargo test -p s3-gallery-core -- view::stat 2>&1
```

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/view/stat.rs
git commit -m "feat: support Option<&str> host_id in get_stats, add get_all_host_stats"
```

---

### Task 2: View layer — timeline, duplicates, tags

**Files:**
- Modify: `crates/s3-gallery-core/src/view/timeline.rs`
- Modify: `crates/s3-gallery-core/src/view/duplicates.rs`
- Modify: `crates/s3-gallery-core/src/view/tags.rs`

- [ ] **Step 1: Change `get_timeline` to accept `Option<&str>`**

In `crates/s3-gallery-core/src/view/timeline.rs`:

```rust
pub async fn get_timeline(db: &SqlitePool, host_id: Option<&str>) -> Result<Vec<TimelineEntry>> {
    let files: Vec<FileEntry> = if let Some(hid) = host_id {
        sqlx::query_as(
            "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 ORDER BY last_modified DESC",
        )
        .bind(hid)
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    } else {
        sqlx::query_as(
            "SELECT * FROM files WHERE is_deleted = 0 ORDER BY last_modified DESC",
        )
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    };

    // Rest of function unchanged
    // ...
}
```

Update tests to pass `Some("test-host")`:

```rust
let timeline = get_timeline(&pool, Some("test-host")).await?;
let timeline = get_timeline(&pool, Some("test-host")).await?;
```

- [ ] **Step 2: Change `find_duplicates` to accept `Option<&str>`**

In `crates/s3-gallery-core/src/view/duplicates.rs`:

```rust
pub async fn find_duplicates(db: &SqlitePool, host_id: Option<&str>) -> Result<Vec<DuplicateGroup>> {
    #[derive(Debug, sqlx::FromRow)]
    struct DuplicateKey {
        size: i64,
        etag: String,
    }

    let keys: Vec<DuplicateKey> = if let Some(hid) = host_id {
        sqlx::query_as(
            "SELECT size, etag
             FROM files
             WHERE host_id = ? AND is_deleted = 0
             GROUP BY size, etag
             HAVING COUNT(*) > 1
             ORDER BY size DESC",
        )
        .bind(hid)
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    } else {
        sqlx::query_as(
            "SELECT size, etag
             FROM files
             WHERE is_deleted = 0
             GROUP BY size, etag
             HAVING COUNT(*) > 1
             ORDER BY size DESC",
        )
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    }?;

    let mut result = Vec::new();

    for key in keys {
        let files: Vec<FileEntry> = if let Some(hid) = host_id {
            sqlx::query_as(
                "SELECT * FROM files
                 WHERE host_id = ? AND size = ? AND etag = ? AND is_deleted = 0
                 ORDER BY key",
            )
            .bind(hid)
            .bind(key.size)
            .bind(&key.etag)
            .fetch_all(db)
            .await
            .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT * FROM files
                 WHERE size = ? AND etag = ? AND is_deleted = 0
                 ORDER BY key",
            )
            .bind(key.size)
            .bind(&key.etag)
            .fetch_all(db)
            .await
            .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
        }?;

        let size = u64::try_from(key.size).unwrap_or(0);

        result.push(DuplicateGroup {
            size: FileSize::new(size),
            files,
        });
    }

    Ok(result)
}
```

Update tests to pass `Some("test-host")`:

```rust
let duplicates = find_duplicates(&pool, Some("test-host")).await?;
let duplicates = find_duplicates(&pool, Some("test-host")).await?;
```

- [ ] **Step 3: Change `list_tags` and `get_files_by_tag` to accept `Option<&str>`**

In `crates/s3-gallery-core/src/view/tags.rs`:

```rust
pub async fn list_tags(db: &SqlitePool, host_id: Option<&str>) -> Result<Vec<TagEntry>> {
    let tags: Vec<TagEntry> = if let Some(hid) = host_id {
        sqlx::query_as(
            "SELECT DISTINCT t.* FROM tags t
             INNER JOIN file_tags ft ON t.tag_id = ft.tag_id
             INNER JOIN files f ON ft.file_key = f.key
             WHERE f.host_id = ?
             ORDER BY t.tag_name",
        )
        .bind(hid)
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    } else {
        sqlx::query_as(
            "SELECT DISTINCT t.* FROM tags t
             INNER JOIN file_tags ft ON t.tag_id = ft.tag_id
             INNER JOIN files f ON ft.file_key = f.key
             ORDER BY t.tag_name",
        )
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    }?;

    Ok(tags)
}
```

```rust
pub async fn get_files_by_tag(
    db: &SqlitePool,
    host_id: Option<&str>,
    tag_name: &str,
) -> Result<Vec<FileEntry>> {
    let files: Vec<FileEntry> = if let Some(hid) = host_id {
        sqlx::query_as(
            "SELECT f.* FROM files f
             INNER JOIN file_tags ft ON f.key = ft.file_key
             INNER JOIN tags t ON ft.tag_id = t.tag_id
             WHERE t.tag_name = ? AND f.host_id = ? AND f.is_deleted = 0
             ORDER BY f.key",
        )
        .bind(tag_name)
        .bind(hid)
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    } else {
        sqlx::query_as(
            "SELECT f.* FROM files f
             INNER JOIN file_tags ft ON f.key = ft.file_key
             INNER JOIN tags t ON ft.tag_id = t.tag_id
             WHERE t.tag_name = ? AND f.is_deleted = 0
             ORDER BY f.key",
        )
        .bind(tag_name)
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?
    }?;

    Ok(files)
}
```

Update tests to pass `Some("test-host")`:

```rust
// In test_list_tags:
let tags = list_tags(&pool, Some("test-host")).await?;

// In test_get_files_by_tag:
let files = get_files_by_tag(&pool, Some("test-host"), "photo").await?;
let files = get_files_by_tag(&pool, Some("test-host"), "video").await?;
let files = get_files_by_tag(&pool, Some("test-host"), "nonexistent").await?;
```

- [ ] **Step 4: Run tests to verify**

```bash
cargo test -p s3-gallery-core 2>&1
```

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/view/timeline.rs crates/s3-gallery-core/src/view/duplicates.rs crates/s3-gallery-core/src/view/tags.rs
git commit -m "feat: support Option<&str> host_id in timeline, duplicates, tags views"
```

---

### Task 3: Update web handlers to pass None

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/stats.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/timeline.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/duplicates.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/tags.rs`

- [ ] **Step 1: Update stats handler**

In `crates/s3-gallery-cli/src/web/handlers/stats.rs`, replace:

```rust
    let host_id = match state.hosts.first() {
        Some(h) => h.host_id.clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"no hosts","detail":"No hosts in database"}))).into_response();
        }
    };

    let file_stats = match stat_view::get_stats(&state.db, &host_id).await {
```

With:

```rust
    let (total_stats, per_host_stats) = match stat_view::get_all_host_stats(&state.db).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(handler = "stats", error = %e, "failed to get stats");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to get stats",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };
```

Then update the context construction to include both totals and per-host data:

```rust
    // Convert by_category HashMap to sorted list for template
    let mut by_category: Vec<CategoryCount> = total_stats
        .by_category
        .iter()
        .map(|(name, count)| CategoryCount {
            name: name.clone(),
            count: *count,
        })
        .collect();
    by_category.sort_by(|a, b| a.name.cmp(&b.name));

    // Build per-host list
    let per_host_list: Vec<serde_json::Value> = per_host_stats
        .iter()
        .map(|(hid, stats)| {
            let mut host_cat: Vec<CategoryCount> = stats
                .by_category
                .iter()
                .map(|(name, count)| CategoryCount {
                    name: name.clone(),
                    count: *count,
                })
                .collect();
            host_cat.sort_by(|a, b| a.name.cmp(&b.name));
            json!({
                "host_id": hid,
                "stats": {
                    "total_files": stats.total_files,
                    "total_size": stats.total_size.to_string(),
                    "by_category": host_cat,
                }
            })
        })
        .collect();

    let context = json!({
        "totals": {
            "total_files": total_stats.total_files,
            "total_size": total_stats.total_size.to_string(),
            "by_category": by_category,
        },
        "per_host": per_host_list,
    });
```

- [ ] **Step 2: Update timeline handler**

In `crates/s3-gallery-cli/src/web/handlers/timeline.rs`, replace:

```rust
    let host_id = match state.hosts.first() {
        Some(h) => h.host_id.clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"no hosts","detail":"No hosts in database"}))).into_response();
        }
    };

    let timeline_entries = match timeline_view::get_timeline(&state.db, &host_id).await {
```

With:

```rust
    let timeline_entries = match timeline_view::get_timeline(&state.db, None).await {
```

Also update the `file_entry_to_json` function to include `host_id`:

```rust
fn file_entry_to_json(file: &s3_gallery_core::db::models::FileEntry) -> serde_json::Value {
    json!({
        "key": file.key,
        "etag": file.etag,
        "size": file.size,
        "last_modified": file.last_modified,
        "content_type": file.content_type,
        "file_type": file.file_type,
        "metadata_state": file.metadata_state,
        "is_deleted": file.is_deleted,
        "host_id": file.host_id,
    })
}
```

- [ ] **Step 3: Update duplicates handler**

In `crates/s3-gallery-cli/src/web/handlers/duplicates.rs`, replace:

```rust
    let host_id = match state.hosts.first() {
        Some(h) => h.host_id.clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"no hosts","detail":"No hosts in database"}))).into_response();
        }
    };

    let duplicate_groups = match duplicates_view::find_duplicates(&state.db, &host_id).await {
```

With:

```rust
    let duplicate_groups = match duplicates_view::find_duplicates(&state.db, None).await {
```

Also update `file_entry_to_json` to include `host_id`:

```rust
fn file_entry_to_json(file: &s3_gallery_core::db::models::FileEntry) -> serde_json::Value {
    json!({
        "key": file.key,
        "etag": file.etag,
        "size": file.size,
        "last_modified": file.last_modified,
        "content_type": file.content_type,
        "file_type": file.file_type,
        "metadata_state": file.metadata_state,
        "is_deleted": file.is_deleted,
        "host_id": file.host_id,
    })
}
```

- [ ] **Step 4: Update tags handler**

In `crates/s3-gallery-cli/src/web/handlers/tags.rs`, replace:

```rust
    let host_id = match state.hosts.first() {
        Some(h) => h.host_id.clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"no hosts","detail":"No hosts in database"}))).into_response();
        }
    };

    let tag_list = match tags_view::list_tags(&state.db, &host_id).await {
```

With:

```rust
    let tag_list = match tags_view::list_tags(&state.db, None).await {
```

- [ ] **Step 5: Build and verify**

```bash
cargo build -p s3-gallery-cli 2>&1
```

Expected: Compilation succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-cli/src/web/handlers/stats.rs crates/s3-gallery-cli/src/web/handlers/timeline.rs crates/s3-gallery-cli/src/web/handlers/duplicates.rs crates/s3-gallery-cli/src/web/handlers/tags.rs
git commit -m "feat: update handlers to pass None for all-host aggregation"
```

---

### Task 4: Update templates for multi-host display

**Files:**
- Modify: `crates/s3-gallery-web/templates/stats.html`
- Modify: `crates/s3-gallery-web/templates/timeline.html`
- Modify: `crates/s3-gallery-web/templates/duplicates.html`
- Modify: `crates/s3-gallery-web/templates/tags.html`

- [ ] **Step 1: Update stats.html**

Replace `crates/s3-gallery-web/templates/stats.html`:

```html
{% extends "layout.html" %}
{% block title %}Stats - s3-gallery{% endblock %}
{% block content %}
<h1>Statistics</h1>

<h2>Total (All Hosts)</h2>
<table>
    <tr><th>Total files</th><td>{{ totals.total_files }}</td></tr>
    <tr><th>Total size</th><td>{{ totals.total_size }}</td></tr>
</table>
<h3>By Category</h3>
<table>
    {% for item in totals.by_category %}
    <tr><td>{{ item.name }}</td><td>{{ item.count }}</td></tr>
    {% endfor %}
</table>

<h2>Per Host</h2>
{% for host in per_host %}
<h3>{{ host.host_id }}</h3>
<table>
    <tr><th>Total files</th><td>{{ host.stats.total_files }}</td></tr>
    <tr><th>Total size</th><td>{{ host.stats.total_size }}</td></tr>
</table>
<table>
    {% for item in host.stats.by_category %}
    <tr><td>{{ item.name }}</td><td>{{ item.count }}</td></tr>
    {% endfor %}
</table>
{% endfor %}
{% endblock %}
```

- [ ] **Step 2: Update timeline.html**

Replace `crates/s3-gallery-web/templates/timeline.html`:

```html
{% extends "layout.html" %}
{% block title %}Timeline - s3-gallery{% endblock %}
{% block content %}
<h1>Timeline</h1>
{% for entry in timeline %}
<h2>{{ entry.date }}</h2>
<div class="gallery-grid">
    {% for file in entry.files %}
    <div>
        <span style="font-size:0.8em;color:#666;background:#eee;padding:2px 6px;border-radius:3px;margin-right:4px;">{{ file.host_id }}</span>
        <a href="/files/{{ file.key }}">{{ file.key }}</a>
    </div>
    {% endfor %}
</div>
{% endfor %}
{% endblock %}
```

- [ ] **Step 3: Update duplicates.html**

Replace `crates/s3-gallery-web/templates/duplicates.html`:

```html
{% extends "layout.html" %}
{% block title %}Duplicates - s3-gallery{% endblock %}
{% block content %}
<h1>Duplicate Files</h1>
{% if groups %}
    {% for group in groups %}
    <h2>Duplicate Group ({{ group.size }})</h2>
    <ul>
        {% for file in group.files %}
        <li>
            <span style="font-size:0.8em;color:#666;background:#eee;padding:2px 6px;border-radius:3px;margin-right:4px;">{{ file.host_id }}</span>
            <a href="/files/{{ file.key }}">{{ file.key }}</a>
        </li>
        {% endfor %}
    </ul>
    {% endfor %}
{% else %}
<p>No duplicate files found.</p>
{% endif %}
{% endblock %}
```

- [ ] **Step 4: Update tags.html**

Replace `crates/s3-gallery-web/templates/tags.html`:

```html
{% extends "layout.html" %}
{% block title %}Tags - s3-gallery{% endblock %}
{% block content %}
<h1>Tags</h1>
<ul>
    {% for tag in tags %}
    <li><a href="/search?tag={{ tag.tag_name }}">{{ tag.tag_name }}</a></li>
    {% endfor %}
</ul>
{% endblock %}
```

Tags template is unchanged — the tag list is already global, just now includes tags from all hosts.

- [ ] **Step 5: Build and verify**

```bash
cargo build 2>&1
```

Expected: Compilation succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-web/templates/
git commit -m "feat: update templates for multi-host display"
```

---

### Task 5: Verify full build and integration

**Files:**
- None (build only)

- [ ] **Step 1: Full build**

```bash
cargo build 2>&1
```

Expected: All crates compile without errors.

- [ ] **Step 2: Run all tests**

```bash
cargo test 2>&1
```

Expected: All 67+ tests pass.

- [ ] **Step 3: Final commit (if any changes)**

```bash
git add -A
git commit -m "chore: finalize multi-host handler adaptation" 2>/dev/null || echo "No changes to commit"
```