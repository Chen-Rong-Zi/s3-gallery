# Serve Auto-Detect Bucket from DB Design

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create the implementation plan, then superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task.

**Goal:** `serve` command should automatically detect the bucket name from the local database, rather than requiring the user to specify `--bucket` on the CLI.

**Architecture:** Add a `bucket` column to the existing `host_config` table. During `scan`, persist the bucket name. During `serve`, read it back. The `--bucket` CLI option becomes optional for `serve` (fallback to DB), but remains required for `scan` and `db` commands.

**Tech Stack:** SQLite (sqlx), Rust, clap CLI

---

## Background

The `serve` command currently requires `--bucket` to be specified on the CLI. If the user runs `serve` with a different bucket than the one used during `scan`, the S3 client connects to the wrong bucket and all file downloads fail with `NoSuchKey`. The user has already scanned the correct bucket — the bucket name should be stored in the database and reused automatically.

## Schema Change

### host_config table

Add a `bucket` column to the existing `host_config` table:

```sql
ALTER TABLE host_config ADD COLUMN bucket TEXT NOT NULL DEFAULT '';
```

### HostConfigEntry struct

Add a `bucket` field:

```rust
pub struct HostConfigEntry {
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub created_at: String,
    pub bucket: String,  // NEW
}
```

Add a new method `upsert_bucket` to insert or update the bucket field for a host:

```rust
pub async fn upsert_bucket(
    pool: &SqlitePool,
    host_id: &str,
    host_name: &str,
    bucket: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket) \
         VALUES (?, ?, 'unknown', '', datetime('now'), ?) \
         ON CONFLICT(host_id) DO UPDATE SET bucket = excluded.bucket"
    )
    .bind(host_id)
    .bind(host_name)
    .bind(bucket)
    .execute(pool)
    .await
    .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert bucket: {e}")))?;
    Ok(())
}
```

Note: `host_config` table is currently created by migration but never populated.
`upsert_bucket` handles both cases: insert new row (first scan) or update existing row (re-scan).

### Migration Strategy

In `run_migrations()`, add an `ALTER TABLE` statement wrapped in error-ignoring logic (column may already exist):

```rust
// Attempt to add bucket column (ignore if already exists)
let _ = sqlx::query("ALTER TABLE host_config ADD COLUMN bucket TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await;
```

## CLI Changes

### `--bucket` becomes optional for `serve`

```rust
/// OSS bucket name (optional — read from DB if not provided)
#[arg(short = 'b', long)]
pub bucket: Option<String>,  // was: String
```

Only the `serve` subcommand's bucket field changes. The top-level `Cli.bucket` remains `Option<String>`, and each subcommand resolves it as needed:

- `scan` → requires `--bucket` (error if missing)
- `db` → requires `--bucket` (error if missing)
- `serve` → optional, falls back to DB

## Data Flow

### Scan: persist bucket

```rust
// In cmd_scan.rs, after successful scan:
let host_id_str = host_id.host_id.clone(); // e.g. "photos"
HostConfigEntry::upsert_bucket(&pool, &host_id_str, host, &cli.bucket).await?;
```

The `host_id` is the host name string (e.g., "photos"). The `bucket` is the CLI `--bucket` value.

### Serve: resolve bucket

```rust
// In cmd_serve.rs:
// 1. Open DB
let pool = create_pool(db_path).await?;
run_migrations(&pool).await?;

// 2. Resolve bucket: CLI arg > DB > error
let bucket_name = match &cli.bucket {
    Some(b) => BucketName::new(b)?,
    None => {
        let config = HostConfigEntry::get(&pool, host).await?;
        if config.bucket.is_empty() {
            return Err(S3GalleryError::InvalidConfig(
                "No bucket specified. Use --bucket or run scan first.".to_string()
            ));
        }
        BucketName::new(&config.bucket)?
    }
};

// 3. Create S3 client with resolved bucket
let config = OssConfig::validate(
    bucket_name.clone(),
    &cli.endpoint,
    &cli.region,
    &cli.access_key,
    &cli.secret_key,
    10,
)?;
let real = RealS3Client::from_config(&config);
let s3 = Arc::new(real) as Arc<dyn S3Client>;

// 4. Create RemoteView with correct bucket
let remote_view = RemoteView::new(local_view.db().clone(), s3, bucket_name.clone());
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| `serve` with `--bucket` | Use CLI value (override) |
| `serve` without `--bucket`, bucket in DB | Use DB value |
| `serve` without `--bucket`, no bucket in DB | Error: "No bucket specified. Run scan first or use --bucket." |
| `scan` without `--bucket` | Error: bucket is required for scan |
| `db` without `--bucket` | Error: bucket is required for db commands |
| DB migration fails to add column | Ignored (column may already exist) |

## Backward Compatibility

- Existing databases without the `bucket` column: migration adds it with empty default
- Old `serve` commands with `--bucket`: continue to work unchanged
- `scan` on existing databases: writes bucket name after scan completes

## Files to Modify

- `crates/s3-gallery-core/src/db/schema.rs` — add ALTER TABLE migration
- `crates/s3-gallery-core/src/db/models.rs` — add bucket field to HostConfigEntry, add update_bucket method
- `crates/s3-gallery-cli/src/cli.rs` — make bucket optional for serve
- `crates/s3-gallery-cli/src/cmd_scan.rs` — save bucket to DB after scan
- `crates/s3-gallery-cli/src/cmd_serve.rs` — read bucket from DB if not in CLI