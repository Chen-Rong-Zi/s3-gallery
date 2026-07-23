# Directory Size Tracking Design

**Goal:** After scan completes, compute the size of every directory prefix and store it in the database, so the browse page can show real directory sizes instead of `0`.

**Architecture:** Add a new `dir_sizes` table. After scan processes all files, iterate the object list to extract all directory prefixes, aggregate sizes, and upsert into the table. The `ls` module reads from `dir_sizes` when listing directories.

**Tech Stack:** Rust, SQLite (sqlx), s3-gallery-core (scanner + view/ls), s3-gallery-cli (web/browse handler), s3-gallery-web (templates)

---

## 1. Schema

New table in `crates/s3-gallery-core/src/db/schema.rs`:

```sql
CREATE TABLE IF NOT EXISTS dir_sizes (
    host_id TEXT NOT NULL,
    dir_path TEXT NOT NULL,
    total_size INTEGER NOT NULL DEFAULT 0,
    total_files INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (host_id, dir_path)
);
```

Each directory is identified by `(host_id, dir_path)` — different hosts can have the same directory name without conflict.

A directory path always ends with `/` to distinguish it from a file key. Examples:
- `photos/`
- `photos/2023/`
- `photos/2023/autumn/`

---

## 2. Scan: Compute Directory Sizes

### Where to insert

In `crates/s3-gallery-core/src/scan/scanner.rs`, function `run_scan`, after Step 8 (update scan_metadata) and before Step 9 (release lock).

### Computation logic

```rust
// Step 8.5: Compute directory sizes
use std::collections::HashMap;

let mut dir_sizes: HashMap<String, (u64, u64)> = HashMap::new(); // dir_path -> (total_size, total_files)

for obj in &s3_objects {
    let key = obj.key.as_str();
    // Extract all directory prefixes from the key
    // e.g., "photos/2023/autumn/img001.jpg" → "photos/", "photos/2023/", "photos/2023/autumn/"
    let mut pos = 0;
    while let Some(slash) = key[pos..].find('/') {
        let prefix_end = pos + slash + 1; // include the trailing slash
        let dir_path = &key[..prefix_end];
        let entry = dir_sizes.entry(dir_path.to_string()).or_insert((0, 0));
        entry.0 += obj.size.as_u64();
        entry.1 += 1;
        pos = prefix_end;
    }
}

// Batch upsert into dir_sizes table
// Use a transaction for efficiency
{
    let mut tx = config.db.begin().await?;
    for (dir_path, (total_size, total_files)) in &dir_sizes {
        sqlx::query(
            "INSERT OR REPLACE INTO dir_sizes (host_id, dir_path, total_size, total_files) \
             VALUES (?, ?, ?, ?)"
        )
        .bind(&config.host_id)
        .bind(dir_path)
        .bind(*total_size as i64)
        .bind(*total_files as i64)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
}
```

### Cleanup note

When files are deleted during scan (diff.deleted_keys), the old dir_sizes rows are stale. The computation above fully recomputes from the current `s3_objects` list, so it naturally handles additions, deletions, and changes.

---

## 3. Model: DirSizeEntry

New struct in `crates/s3-gallery-core/src/db/models.rs`:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DirSizeEntry {
    pub host_id: String,
    pub dir_path: String,
    pub total_size: i64,
    pub total_files: i64,
}

impl DirSizeEntry {
    /// Get the size of a specific directory.
    pub async fn get(pool: &SqlitePool, host_id: &str, dir_path: &str) -> Result<DirSizeEntry> {
        sqlx::query_as::<_, DirSizeEntry>(
            "SELECT * FROM dir_sizes WHERE host_id = ? AND dir_path = ?"
        )
        .bind(host_id)
        .bind(dir_path)
        .fetch_optional(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to get dir size: {e}")))?
        .ok_or_else(|| {
            S3GalleryError::NotFound(format!("Dir size not found: {host_id}/{dir_path}"))
        })
    }

    /// List all subdirectory sizes under a given prefix.
    /// e.g., list_prefix("photos/") returns "photos/2023/", "photos/videos/", etc.
    pub async fn list_by_prefix(
        pool: &SqlitePool,
        host_id: &str,
        prefix: &str,
    ) -> Result<Vec<DirSizeEntry>> {
        sqlx::query_as::<_, DirSizeEntry>(
            "SELECT * FROM dir_sizes \
             WHERE host_id = ? AND dir_path != ? AND dir_path LIKE ? \
             ORDER BY dir_path"
        )
        .bind(host_id)
        .bind(prefix)
        .bind(format!("{}%", prefix))
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list dir sizes: {e}")))
    }
}
```

---

## 4. Ls Module: Show Real Directory Sizes

### Update `LsEntry`

Add `file_count` field:

```rust
pub struct LsEntry {
    pub name: String,
    pub size: FileSize,
    pub file_count: u64,
    pub is_directory: bool,
    pub last_modified: String,
}
```

### Update `list_directory`

When listing a directory at path `prefix`:
1. Query `dir_sizes` for all entries matching `prefix`
2. For each subdirectory name found in the file listing, look up its size from `dir_sizes`
3. Instead of `size: 0`, use the actual size and file count

---

## 5. Browse Handler / Template

### `EntryView` struct

Add `file_count` field:

```rust
struct EntryView {
    name: String,
    path: String,
    size: String,
    file_count: u64,
    file_type: String,
    last_modified: String,
    is_directory: bool,
}
```

### Template

In browse.html, the directory row shows:
- Name (link)
- Size (formatted, e.g., "1.2 GB")
- Files (count)
- Last modified

---

## 6. Migration

Add a new migration step after the existing schema creation:

```rust
// dir_sizes table
execute_query(
    pool,
    "CREATE TABLE IF NOT EXISTS dir_sizes (
        host_id TEXT NOT NULL,
        dir_path TEXT NOT NULL,
        total_size INTEGER NOT NULL DEFAULT 0,
        total_files INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (host_id, dir_path)
    );",
)
.await?;
```

And bump `db_schema_version` in `scan_metadata` from 1 to 2 for fresh scans. Existing databases will get the new table created on next migration run.

---

## 7. Files Changed

| File | Change |
|------|--------|
| `crates/s3-gallery-core/src/db/schema.rs` | Add `dir_sizes` table creation |
| `crates/s3-gallery-core/src/db/models.rs` | Add `DirSizeEntry` struct + methods |
| `crates/s3-gallery-core/src/scan/scanner.rs` | Add Step 8.5: compute and store dir sizes |
| `crates/s3-gallery-core/src/view/ls.rs` | Read dir sizes from DB, add `file_count` to `LsEntry` |
| `crates/s3-gallery-cli/src/web/handlers/browse.rs` | Pass `file_count` to template |
| `crates/s3-gallery-web/templates/browse.html` | Show size and file count for directories |