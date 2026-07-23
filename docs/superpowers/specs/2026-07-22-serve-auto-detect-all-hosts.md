# Serve Auto-Detect All Hosts Design

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create the implementation plan, then superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task.

**Goal:** `serve` command should auto-detect all hosts from the database instead of requiring a `host` argument. Multiple hosts may span different buckets, endpoints, and regions.

**Architecture:** Add `endpoint` and `region` columns to `host_config` table. During `scan`, persist all three connection parameters. During `serve`, read all hosts from DB, group by endpoint to create S3 clients, and serve files from all hosts. The `--prefix` option filters by directory prefix.

**Tech Stack:** SQLite (sqlx), Rust, clap CLI, axum

---

## Background

The `serve` command currently requires a `host` argument (e.g., `s3-gallery serve photos`). This is redundant because:
1. The database already knows which hosts have been scanned
2. The database stores each host's bucket (from the previous feature)
3. The user should just `s3-gallery serve` and get all their files

Additionally, different hosts may use different S3 endpoints and regions. These connection parameters should also be stored in the database so `serve` can auto-configure itself.

## Schema Change

### host_config table

Add `endpoint` and `region` columns:

```sql
ALTER TABLE host_config ADD COLUMN endpoint TEXT NOT NULL DEFAULT '';
ALTER TABLE host_config ADD COLUMN region TEXT NOT NULL DEFAULT '';
```

### HostConfigEntry struct

```rust
pub struct HostConfigEntry {
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub created_at: String,
    pub bucket: String,
    pub endpoint: String,  // NEW
    pub region: String,    // NEW
}
```

### New method: `list_all`

```rust
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<HostConfigEntry>> {
    sqlx::query_as::<_, HostConfigEntry>("SELECT * FROM host_config ORDER BY host_id")
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list host configs: {e}")))
}
```

### Replace `upsert_bucket` with `upsert_host_config`

```rust
pub async fn upsert_host_config(
    pool: &SqlitePool,
    host_id: &str,
    bucket: &str,
    endpoint: &str,
    region: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket, endpoint, region) \
         VALUES (?, 'unknown', 'unknown', '', datetime('now'), ?, ?, ?) \
         ON CONFLICT(host_id) DO UPDATE SET bucket = excluded.bucket, endpoint = excluded.endpoint, region = excluded.region"
    )
    .bind(host_id)
    .bind(bucket)
    .bind(endpoint)
    .bind(region)
    .execute(pool)
    .await?;
    Ok(())
}
```

### Migration Strategy

In `run_migrations()`, add two ALTER TABLE statements:

```rust
let _ = sqlx::query("ALTER TABLE host_config ADD COLUMN endpoint TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await;
let _ = sqlx::query("ALTER TABLE host_config ADD COLUMN region TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await;
```

## CLI Changes

### `serve` subcommand

- Remove `host` positional argument (it becomes optional, or removed entirely)
- Add `--prefix` optional argument for directory filtering

```rust
#[command(alias = "web")]
Serve {
    /// File prefix filter (e.g. "photos/")
    #[arg(long)]
    prefix: Option<String>,
    /// Web server port
    #[arg(long, default_value = "8080")]
    port: u16,
    /// Read-only mode
    #[arg(long)]
    readonly: bool,
},
```

### `serve` command usage

```
s3-gallery serve                    # Serve all files from all hosts
s3-gallery serve --prefix photos/   # Serve only files under photos/ prefix
```

## Data Flow

### Scan: persist host config

```rust
// In cmd_scan.rs, after successful scan:
HostConfigEntry::upsert_host_config(
    &db_pool,
    &host_id.host_id,
    bucket_str,
    &cli.endpoint,
    &cli.region,
).await?;
```

### Serve: auto-detect all hosts

```rust
pub async fn run_serve(cli: &Cli, port: u16, readonly: bool, prefix: Option<String>) -> Result<()> {
    // 1. Open DB
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;

    // 2. Read all hosts from DB
    let hosts = HostConfigEntry::list_all(&pool).await?;
    if hosts.is_empty() {
        return Err(S3GalleryError::NotFound(
            "No hosts found. Run 's3-gallery scan <host>' first.".to_string(),
        ));
    }

    // 3. Group hosts by endpoint, create S3 clients
    let mut s3_clients: HashMap<String, Arc<dyn S3Client>> = HashMap::new();
    for host in &hosts {
        let endpoint = if host.endpoint.is_empty() { &cli.endpoint } else { &host.endpoint };
        if !s3_clients.contains_key(endpoint) {
            let config = OssConfig::validate(
                BucketName::new(&host.bucket)?,
                endpoint,
                &host.region,
                &cli.access_key,
                &cli.secret_key,
                10,
            )?;
            let client = RealS3Client::from_config(&config);
            s3_clients.insert(endpoint.clone(), Arc::new(client) as Arc<dyn S3Client>);
        }
    }

    // 4. Create views (one per host? or unified?)
    let local_view = LocalView::new(pool.clone());
    // ...

    // 5. Build AppState with all hosts + S3 clients
    let app_state = AppState {
        templates: Arc::new(env),
        db: Arc::new(pool),
        hosts: hosts.clone(),
        s3_clients: Arc::new(s3_clients),
        prefix: prefix.clone(),
    };

    // 6. Start server
    // ...
}
```

### Download flow

When a user requests a file download:
1. Handler receives `host_id` and `key` from the URL
2. Looks up `host_id` in `app_state.hosts` to find the bucket
3. Gets the S3 client for the host's endpoint from `app_state.s3_clients`
4. Downloads `{bucket}/{key}` from S3

## Host Configuration List Page

The web server's root path (`/`) shows a list of all scanned hosts with their metadata:

- Host ID (e.g., "photos")
- Bucket name
- File count
- Last scan time

Each host entry links to the file browser filtered by that host's prefix.

## Web UI Changes

### New endpoint: GET /api/hosts
Returns JSON list of all hosts with their metadata.

### Updated endpoint: GET /api/files
Currently takes `host` and `prefix` params. Updated to support:
- `GET /api/files?prefix=photos/` — list files from all hosts matching prefix
- Response includes `host_id` and `bucket` per file entry for download routing

### New endpoint: GET /api/download/{host_id}/{key}
Downloads a file from the correct bucket and S3 client based on host_id.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| `serve` with no hosts in DB | Error: "No hosts found. Run 's3-gallery scan <host>' first." |
| `serve` with --prefix matching no files | Show empty gallery, not an error |
| Download with unknown host_id | Error: "Host not found" |
| S3 download fails for a host | Return error with host_id and bucket context |
| host has empty endpoint | Fall back to CLI `--endpoint` / default |
| host has empty region | Fall back to CLI `--region` / default |

## Backward Compatibility

- Existing databases without `endpoint`/`region` columns: migration adds them with empty defaults
- Existing `host_config` rows: `endpoint` and `region` are empty, `serve` falls back to CLI defaults
- Old `scan` results: next scan updates the row with endpoint/region
- CLI `--bucket` for `serve`: still supported as override for single-host mode

## Files to Modify

- `crates/s3-gallery-core/src/db/schema.rs` — add ALTER TABLE migrations
- `crates/s3-gallery-core/src/db/models.rs` — add endpoint/region fields, list_all, replace upsert_bucket with upsert_host_config
- `crates/s3-gallery-cli/src/cli.rs` — remove host arg from serve, add --prefix
- `crates/s3-gallery-cli/src/cmd_scan.rs` — save endpoint/region to DB
- `crates/s3-gallery-cli/src/cmd_serve.rs` — complete rewrite for multi-host
- `crates/s3-gallery-cli/src/web/state.rs` — update AppState structure
- `crates/s3-gallery-cli/src/web/handlers/` — update file listing and download handlers
- `crates/s3-gallery-cli/src/web/router.rs` — update routing for new endpoints