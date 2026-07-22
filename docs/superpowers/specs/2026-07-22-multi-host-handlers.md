# Multi-host Handler Adaptation Design

**Goal:** Adapt all global web handlers (stats, timeline, duplicates, tags) to show data from all hosts, not just the first host in the database.

**Architecture:** Change the view layer functions to accept `Option<&str>` for host_id — when `None`, omit the `WHERE host_id = ?` filter. Handlers pass `None` to get all-host data. Stats additionally returns per-host breakdown via a new `get_all_host_stats` function.

**Tech Stack:** Rust, SQLite (sqlx), s3-gallery-core view layer, s3-gallery-cli web handlers, minijinja templates

---

## 1. View Layer Changes (`s3-gallery-core`)

### 1.1 Stats (`crates/s3-gallery-core/src/view/stat.rs`)

**Change `get_stats` signature:**
```rust
pub async fn get_stats(db: &SqlitePool, host_id: Option<&str>) -> Result<FileStats>
```

When `host_id = Some("host")` → filter with `WHERE host_id = ?` (current behavior).
When `host_id = None` → omit `WHERE host_id = ?` from all queries (aggregate across all hosts).

**Add new function `get_all_host_stats`:**
```rust
/// Get stats for each host individually, plus total across all hosts.
pub async fn get_all_host_stats(db: &SqlitePool) -> Result<(FileStats, Vec<(String, FileStats)>)>
```

Returns:
- `FileStats` — total aggregated across all hosts
- `Vec<(String, FileStats)>` — per-host breakdown

Implementation: query `SELECT DISTINCT host_id FROM files WHERE is_deleted = 0`, then call `get_stats` for each host.

### 1.2 Timeline (`crates/s3-gallery-core/src/view/timeline.rs`)

**Change `get_timeline` signature:**
```rust
pub async fn get_timeline(db: &SqlitePool, host_id: Option<&str>) -> Result<Vec<TimelineEntry>>
```

Same pattern: `Some(host_id)` filters, `None` aggregates across all hosts.

### 1.3 Duplicates (`crates/s3-gallery-core/src/view/duplicates.rs`)

**Change `find_duplicates` signature:**
```rust
pub async fn find_duplicates(db: &SqlitePool, host_id: Option<&str>) -> Result<Vec<DuplicateGroup>>
```

Same pattern. The `DuplicateGroup.files` already contains `FileEntry` with `host_id` field, so the template can display which host each file belongs to and distinguish same-host vs cross-host duplicates.

### 1.4 Tags (`crates/s3-gallery-core/src/view/tags.rs`)

**Change `list_tags` signature:**
```rust
pub async fn list_tags(db: &SqlitePool, host_id: Option<&str>) -> Result<Vec<TagEntry>>
```

When `host_id = None`, remove the `WHERE f.host_id = ?` from the tag join query, returning tags from all hosts.

**Change `get_files_by_tag` signature:**
```rust
pub async fn get_files_by_tag(db: &SqlitePool, host_id: Option<&str>, tag_name: &str) -> Result<Vec<FileEntry>>
```

---

## 2. Handler Changes (`s3-gallery-cli`)

### 2.1 Stats handler (`crates/s3-gallery-cli/src/web/handlers/stats.rs`)

Replace:
```rust
let host_id = match state.hosts.first() {
    Some(h) => h.host_id.clone(),
    None => return error...
};
let file_stats = stat_view::get_stats(&state.db, &host_id).await?;
```

With:
```rust
let (total_stats, per_host_stats) = stat_view::get_all_host_stats(&state.db).await?;
```

Template context includes both `total_stats` and `per_host_stats` (list of `{host_id, stats}`).

### 2.2 Timeline handler (`crates/s3-gallery-cli/src/web/handlers/timeline.rs`)

Replace:
```rust
let host_id = state.hosts.first()...;
let timeline_entries = timeline_view::get_timeline(&state.db, &host_id).await?;
```

With:
```rust
let timeline_entries = timeline_view::get_timeline(&state.db, None).await?;
```

### 2.3 Duplicates handler (`crates/s3-gallery-cli/src/web/handlers/duplicates.rs`)

Replace:
```rust
let host_id = state.hosts.first()...;
let duplicate_groups = duplicates_view::find_duplicates(&state.db, &host_id).await?;
```

With:
```rust
let duplicate_groups = duplicates_view::find_duplicates(&state.db, None).await?;
```

### 2.4 Tags handler (`crates/s3-gallery-cli/src/web/handlers/tags.rs`)

Replace:
```rust
let host_id = state.hosts.first()...;
let tag_list = tags_view::list_tags(&state.db, &host_id).await?;
```

With:
```rust
let tag_list = tags_view::list_tags(&state.db, None).await?;
```

---

## 3. Template Changes (`s3-gallery-web`)

### 3.1 stats.html

Add per-host breakdown section below the total summary:

```html
<h1>Statistics</h1>
<h2>Total (All Hosts)</h2>
<table>
  <tr><th>Total files</th><td>{{ totals.total_files }}</td></tr>
  <tr><th>Total size</th><td>{{ totals.total_size }}</td></tr>
</table>
<h2>By Category</h2>
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
{% endfor %}
```

### 3.2 timeline.html

Each file entry includes its `host_id`:

```html
{% for file in entry.files %}
<div>
  <span class="host-tag">{{ file.host_id }}</span>
  <a href="/files/{{ file.key }}">{{ file.key }}</a>
</div>
{% endfor %}
```

### 3.3 duplicates.html

Each file in a duplicate group shows its `host_id`. If all files share the same host_id, label as "Same host". Otherwise label as "Cross-host".

### 3.4 tags.html

Each tag shows the list of hosts that have files with this tag.

---

## 4. Test Changes

### 4.1 View layer tests

Update existing tests to pass `Some("host_id")` instead of `"host_id"` to maintain backward compatibility. Add new tests for `None` (all hosts) behavior.

### 4.2 Handler tests

No changes needed — handler tests that use mock data already work with the view layer changes.

---

## 5. Backward Compatibility

- All existing callers of view functions still work by passing `Some("host_id")` instead of `"host_id"`
- The `HostConfigEntry::list_all` and `state.hosts` remain unchanged
- Only the host_id filtering behavior changes in the view layer