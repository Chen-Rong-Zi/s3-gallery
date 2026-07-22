# .ossgallery Redesign — Single DB + Rename

**Date:** 2026-07-22
**Status:** Draft

## Summary

Redesign the `.ossgallery` directory concept to clarify the boundary between local
and remote state, merge all per-host databases into a single local SQLite database,
and rename the entire project from `ossgalley` to `s3-gallery`.

## Motivation

The current design conflates two concerns:

1. **Remote metadata** — `.ossgallery/` on S3 should mark a host directory and hold
   host configuration (e.g. `host.config.json`).
2. **Local DB cache** — Each host's DB file is downloaded into a local `.ossgallery/`
   directory, creating unnecessary filesystem clutter and confusion.

Furthermore, the crate and binary names (`ossgalley-*`) do not match the project
name (`s3-gallery`). All names should be unified.

## Architecture

### Naming

| Current | New |
|---|---|
| Project root `ossgalley/` | `s3-gallery/` (already done) |
| Crate `ossgalley-core` | `s3-gallery-core` |
| Crate `ossgalley-cli` | `s3-gallery-cli` |
| Crate `ossgalley-web` | `s3-gallery-web` |
| Binary name `ossgalley` | `s3-gallery` |
| Error type `OssgalleyError` | `S3GalleryError` |
| Result type alias `OssgalleyResult` | `S3GalleryResult` (or keep as `Result`) |
| Env prefix `OSSGALLEY_*` | `S3_GALLERY_*` |
| Path segment `.ossgallery` | `.s3-gallery` |

### Remote directory structure (S3 bucket)

```
Bucket root
├── s3-gallery.db              ← unified DB (backup/sync)
├── s3-gallery.lock            ← scan lock (bucket root)
├── camera-1/.s3-gallery/
│   └── host.config.json       ← host metadata
├── camera-2/.s3-gallery/
│   └── host.config.json
└── ...
```

Key points:
- `.s3-gallery/` exists **only on S3**, never locally.
- Each host prefix gets a `.s3-gallery/host.config.json` created by `init`.
- The unified DB `s3-gallery.db` sits at the **bucket root**, not under any host prefix.
- The lock file `s3-gallery.lock` also sits at the bucket root.

### Local storage

- **Single SQLite file** at `./s3-gallery.db` (default, configurable via `--db-path`
  or `S3_GALLERY_DB_PATH`).
- No local `.s3-gallery/` directory is created.
- DB location is independent of which host is being scanned or served.

### Database schema

#### `files` table — add `host_id` column

```sql
CREATE TABLE IF NOT EXISTS files (
    host_id TEXT NOT NULL,
    key TEXT NOT NULL,
    etag TEXT NOT NULL,
    size INTEGER NOT NULL,
    last_modified TEXT NOT NULL,
    content_type TEXT,
    file_type TEXT NOT NULL,
    metadata_state TEXT NOT NULL DEFAULT 'pending',
    is_deleted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (host_id, key)
);
```

#### `scan_metadata` table — add `host_id` as primary key

```sql
CREATE TABLE IF NOT EXISTS scan_metadata (
    host_id TEXT NOT NULL PRIMARY KEY,
    last_scanned_key TEXT,
    last_scanned_at TEXT,
    total_files INTEGER,
    total_size INTEGER,
    db_schema_version INTEGER NOT NULL DEFAULT 1
);
```

#### Other tables — unchanged

- `host_config` — already has `host_id` PK, no change needed
- `metadata` — linked via `file_key` FK to `files`, indirectly scoped by host
- `thumbnails` — linked via `file_key` FK to `files`
- `tags`, `file_tags` — unchanged
- `classification_rules`, `extractor_rules` — unchanged

#### Indexes

Add `idx_files_host_id` on `files(host_id)`.
Existing indexes remain.

## Component changes

### `s3-gallery-core` (was `ossgalley-core`)

**`s3/config.rs` (HostIdentifier):**
- Rename all `.ossgallery` → `.s3-gallery`
- Change `db_path()` to return `s3-gallery.db` (bucket root, not under host prefix)
- Change `lock_path()` to return `s3-gallery.lock` (bucket root)
- Change `config_path()` to return `{prefix}/.s3-gallery/host.config.json`
- Change `ossgallery_dir()` to return `{prefix}/.s3-gallery/`

**`error.rs`:**
- Rename `OssgalleyError` → `S3GalleryError`
- Rename result type alias accordingly

**`db/schema.rs`:**
- Update schema to match new table definitions (`host_id` in `files`, `scan_metadata`)
- Add migration from v1 to v2

**`db/models.rs`:**
- Update `FileEntry` struct — add `host_id` field
- Update `ScanMetadata` — add `host_id` field
- Update all CRUD methods to include `host_id`

**`db/status.rs`:**
- Simplify: no longer need to check remote DB for status (local is authoritative)
- `check_db_status` becomes a simple local check

**`view/` module:**
- All view queries gain `WHERE host_id = ?` filter
- Pass host_id from caller

### `s3-gallery-cli` (was `ossgalley-cli`)

**`cli.rs`:**
- Rename binary name from `ossgalley` to `s3-gallery`
- Change default `--db-path` from `.ossgallery/ossgallery.db` to `s3-gallery.db`
- Change env vars from `OSSGALLEY_*` to `S3_GALLERY_*`

**`cmd_init.rs`:**
- Keep command, but only print `"init: not yet implemented"` and exit
- No DB or S3 operations

**`cmd_scan.rs`:**
- Remove local `.ossgallery/` directory creation
- Use `cli.db_path` directly for DB
- Pass `host_id` when writing to `files` and `scan_metadata`
- After scan completes, auto `db push`

**`cmd_serve.rs`:**
- Remove remote DB download logic
- Open local DB directly, filter by host
- If DB doesn't exist, attempt `db pull` first

**`cmd_db.rs`:**
- `db pull`: download `s3-gallery.db` from bucket root
- `db push`: upload `s3-gallery.db` to bucket root
- `db status`: compare local vs remote mtime
- `db lock`: check `s3-gallery.lock` at bucket root
- `db unlock`: delete `s3-gallery.lock` at bucket root

**`web/` handlers:**
- Update import paths to use new crate names
- Pass `host_id` through to view queries

### `s3-gallery-web` (was `ossgalley-web`)

- Rename crate; no functional changes
- Template content may reference `s3-gallery` in UI text

## Data flow

### Scan flow (new)

```
1. Open local s3-gallery.db (create + migrate if not exists)
2. Optional: db pull to sync latest from remote
3. Acquire s3-gallery.lock on S3
4. List objects under host prefix
5. Write/update files with host_id = host_prefix
6. Update scan_metadata for this host
7. Release lock
8. Auto db push
```

### Serve flow (new)

```
1. Open local s3-gallery.db
2. If not exists → db pull, then open
3. All queries filter by host_id
```

## Migration

Since there is no production data yet, no formal migration is needed. The schema
version in `scan_metadata` will be bumped to 2, and the new schema will be created
fresh. Old `oss.db` / `.ossgallery/` files can be deleted manually.

## Non-goals

- No changes to the `view` command (it already works with LocalView)
- No changes to the scan algorithm (only data model changes)
- No changes to the web templates (except branding text)
- No changes to MinIO/test setup