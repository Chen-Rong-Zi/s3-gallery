# Directory Size Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After scan completes, compute the size of every directory prefix and store it in the database, so the browse page shows real directory sizes instead of `0`.

**Architecture:** Add a `dir_sizes` table. After scan processes all files, iterate the object list to extract all directory prefixes, aggregate sizes, and upsert into the table. The `ls` module reads from `dir_sizes` when listing directories. The `LsEntry` struct gains a `file_count` field.

**Tech Stack:** Rust, SQLite (sqlx), s3-gallery-core (scanner + view/ls), s3-gallery-cli (web/browse handler), s3-gallery-web (templates)

**Estimated total time:** 60 min

---

### Task 1: Schema + Model — dir_sizes table and DirSizeEntry

**Files:**
- Modify: `crates/s3-gallery-core/src/db/schema.rs`
- Modify: `crates/s3-gallery-core/src/db/models.rs`

- [ ] **Step 1: Add dir_sizes table to schema**

In `crates/s3-gallery-core/src/db/schema.rs`, after the `scan_metadata` table creation (around line 167), add:

```rust
    // dir_sizes
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

- [ ] **Step 2: Add DirSizeEntry model**

In `crates/s3-gallery-core/src/db/models.rs`, after the `ScanMetadata` impl block (around line 694), add:

```rust
// ---------------------------------------------------------------------------
// DirSizeEntry
// ---------------------------------------------------------------------------

/// A row in the `dir_sizes` table, storing aggregate size for a directory.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DirSizeEntry {
    /// Host that owns this directory.
    pub host_id: String,
    /// Directory path (always ends with '/').
    pub dir_path: String,
    /// Total size of all files under this directory.
    pub total_size: i64,
    /// Total number of files under this directory.
    pub total_files: i64,
}

impl DirSizeEntry {
    /// Get the size of a specific directory.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no entry exists.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn list_by_prefix(
        pool: &SqlitePool,
        host_id: &str,
        prefix: &str,
    ) -> Result<Vec<DirSizeEntry>> {
        // Exclude the exact prefix match itself, only return subdirectories
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

    /// Upsert a directory size entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn upsert(
        pool: &SqlitePool,
        host_id: &str,
        dir_path: &str,
        total_size: i64,
        total_files: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO dir_sizes (host_id, dir_path, total_size, total_files) \
             VALUES (?, ?, ?, ?)"
        )
        .bind(host_id)
        .bind(dir_path)
        .bind(total_size)
        .bind(total_files)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert dir size: {e}")))?;
        Ok(())
    }

    /// Delete all dir_sizes entries for a given host.
    /// Used when re-scanning a host from scratch.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn delete_by_host(pool: &SqlitePool, host_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM dir_sizes WHERE host_id = ?")
            .bind(host_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete dir sizes: {e}")))?;
        Ok(())
    }
}
```

- [ ] **Step 3: Add DirSizeEntry to the public exports in models.rs**

Check the top of `models.rs` — there should be `pub use` re-exports or the structs are used directly. Ensure `DirSizeEntry` is importable.

- [ ] **Step 4: Build and test**

```bash
cargo build -p s3-gallery-core 2>&1
cargo test -p s3-gallery-core -- db::models 2>&1
```

Expected: Compilation succeeds, existing tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/db/schema.rs crates/s3-gallery-core/src/db/models.rs
git commit -m "feat: add dir_sizes table and DirSizeEntry model"
```

---

### Task 2: Scanner — compute dir sizes after scan

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`

- [ ] **Step 1: Add the dir size computation step**

In `crates/s3-gallery-core/src/scan/scanner.rs`, after Step 8 (update scan_metadata, line 207) and before Step 9 (release lock, line 209), insert:

```rust
    // Step 8.5: Compute and store directory sizes
    {
        use std::collections::HashMap;
        let mut dir_agg: HashMap<String, (u64, u64)> = HashMap::new();

        for obj in &s3_objects {
            let key = obj.key.as_str();
            // Extract all directory prefixes from the key
            // e.g., "photos/2023/autumn/img.jpg" → "photos/", "photos/2023/", "photos/2023/autumn/"
            let mut pos = 0;
            while let Some(slash) = key[pos..].find('/') {
                let prefix_end = pos + slash + 1; // include trailing '/'
                let dir_path = &key[..prefix_end];
                let entry = dir_agg.entry(dir_path.to_string()).or_insert((0, 0));
                entry.0 += obj.size.as_u64();
                entry.1 += 1;
                pos = prefix_end;
            }
        }

        // Batch upsert in a transaction
        let mut tx = config.db.begin().await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to begin transaction: {e}")))?;
        for (dir_path, (total_size, total_files)) in &dir_agg {
            sqlx::query(
                "INSERT OR REPLACE INTO dir_sizes (host_id, dir_path, total_size, total_files) \
                 VALUES (?, ?, ?, ?)"
            )
            .bind(&config.host_id)
            .bind(dir_path)
            .bind(*total_size as i64)
            .bind(*total_files as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert dir size: {e}")))?;
        }
        tx.commit().await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to commit transaction: {e}")))?;

        tracing::debug!(
            target: "s3_gallery::scan",
            host_id = %config.host_id,
            directories = dir_agg.len(),
            "Directory sizes computed"
        );
    }
```

- [ ] **Step 2: Add the `sqlx` import if needed**

The `sqlx` crate is already available via the `config.db` field. The `use std::collections::HashMap` is inside the code block, so no new top-level imports are needed.

- [ ] **Step 3: Build and verify**

```bash
cargo build -p s3-gallery-core 2>&1
```

Expected: Compilation succeeds.

- [ ] **Step 4: Run scan tests**

```bash
cargo test -p s3-gallery-core -- scan 2>&1
```

Expected: All scan tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-core/src/scan/scanner.rs
git commit -m "feat: compute and store directory sizes at end of scan"
```

---

### Task 3: Ls module + Browse handler — show real dir sizes

**Files:**
- Modify: `crates/s3-gallery-core/src/view/ls.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/browse.rs`
- Modify: `crates/s3-gallery-web/templates/browse.html`

- [ ] **Step 1: Update LsEntry to include file_count**

In `crates/s3-gallery-core/src/view/ls.rs`, change the `LsEntry` struct:

```rust
/// A single entry in a directory listing.
#[derive(Debug, Clone)]
pub struct LsEntry {
    /// Name of the file or directory.
    pub name: String,
    /// File size (0 for directories that haven't been computed).
    pub size: FileSize,
    /// Number of files in this directory (0 for files).
    pub file_count: u64,
    /// Whether this is a directory.
    pub is_directory: bool,
    /// ISO-8601 timestamp of last modification.
    pub last_modified: String,
}
```

Update the `directory` constructor:

```rust
impl LsEntry {
    fn directory(name: String) -> Self {
        LsEntry {
            name,
            size: FileSize::new(0),
            file_count: 0,
            is_directory: true,
            last_modified: String::new(),
        }
    }
    // from_file stays the same, just add file_count: 0
}
```

Update `from_file` to include `file_count: 0`:

```rust
    fn from_file(file: &FileEntry) -> Self {
        let size = u64::try_from(file.size).unwrap_or(0);
        LsEntry {
            name: file.key.rsplit('/').next().unwrap_or("").to_string(),
            size: FileSize::new(size),
            file_count: 0,
            is_directory: false,
            last_modified: file.last_modified.clone(),
        }
    }
```

- [ ] **Step 2: Update `list_directory` to fetch dir sizes**

In `crates/s3-gallery-core/src/view/ls.rs`, after collecting directories and before building the entries list, add a query to fetch dir sizes:

```rust
pub async fn list_directory(
    pool: &SqlitePool,
    host_id: &str,
    prefix: &str,
    sort_field: SortField,
    sort_order: SortOrder,
) -> Result<Vec<LsEntry>> {
    // ... existing code that builds directories HashSet ...

    // Fetch directory sizes from dir_sizes table
    let dir_sizes = DirSizeEntry::list_by_prefix(pool, host_id, prefix).await
        .unwrap_or_default();
    let size_map: std::collections::HashMap<String, (i64, i64)> = dir_sizes
        .iter()
        .map(|d| (d.dir_path.clone(), (d.total_size, d.total_files)))
        .collect();

    // ... existing code that builds entries ...

    for dir in directories {
        let dir_path = format!("{}{}/", prefix, dir);
        let (size, count) = size_map.get(&dir_path).copied().unwrap_or((0, 0));
        let mut entry = LsEntry::directory(dir);
        entry.size = FileSize::new(size as u64);
        entry.file_count = count as u64;
        entries.push(entry);
    }

    // ... rest of existing code ...
}
```

Add the import for `DirSizeEntry` at the top of the file:

```rust
use crate::db::models::DirSizeEntry;
```

- [ ] **Step 3: Update browse handler to pass file_count to template**

In `crates/s3-gallery-cli/src/web/handlers/browse.rs`, update the `EntryView` struct:

```rust
#[derive(Debug, Clone, serde::Serialize)]
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

Update the `EntryView` construction in the `list_directory` result section (around line 257):

```rust
    let entry_views: Vec<EntryView> = entries
        .iter()
        .map(|entry| {
            let entry_path = if effective_prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{}{}", effective_prefix, entry.name)
            };
            EntryView {
                name: entry.name.clone(),
                path: entry_path,
                size: if entry.is_directory {
                    if entry.file_count > 0 {
                        format!("{} ({})", entry.size, entry.file_count)
                    } else {
                        "-".to_string()
                    }
                } else {
                    entry.size.to_string()
                },
                file_count: entry.file_count,
                file_type: entry.file_type.to_string(),
                last_modified: entry.last_modified.clone(),
                is_directory: entry.is_directory,
            }
        })
        .collect();
```

Also update the root-level host entries (around line 185):

```rust
    let entry_views: Vec<EntryView> = state
        .hosts
        .iter()
        .map(|h| EntryView {
            name: h.host_id.clone(),
            path: h.host_id.clone(),
            size: "-".to_string(),
            file_count: 0,
            file_type: "directory".to_string(),
            last_modified: h.created_at.clone(),
            is_directory: true,
        })
        .collect();
```

- [ ] **Step 4: Update browse.html template**

Read `crates/s3-gallery-web/templates/browse.html` and add a `Files` column header and show the file count for directories:

```html
<th>Name</th>
<th>Size</th>
<th>Files</th>
<th>Last Modified</th>
```

And for each row:
```html
<td><a href="/browse?path={{ entry.path }}">{{ entry.name }}</a></td>
<td>{{ entry.size }}</td>
<td>{{ entry.file_count }}</td>
<td>{{ entry.last_modified }}</td>
```

- [ ] **Step 5: Build and verify**

```bash
cargo build 2>&1
```

Expected: Compilation succeeds.

- [ ] **Step 6: Run tests**

```bash
cargo test 2>&1
```

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/s3-gallery-core/src/view/ls.rs crates/s3-gallery-cli/src/web/handlers/browse.rs crates/s3-gallery-web/templates/browse.html
git commit -m "feat: show real directory sizes from dir_sizes table in browse"
```

---

### Task 4: Verify full build and integration

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

Expected: All tests pass.

- [ ] **Step 3: Final commit (if any changes)**

```bash
git add -A
git commit -m "chore: finalize directory size tracking" 2>/dev/null || echo "No changes to commit"
```