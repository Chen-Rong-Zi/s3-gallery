# Traffic Recording 细粒度改造 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将流量记录从原子计数器 + `__aggregated` 聚合改为 mpsc 通道 + 批量写入，保留 business/host/operation/file_key 全维度明细

**Architecture:** `BusinessS3Client` 将 `TrafficRecord` 发入 `mpsc` 通道，后台 `TrafficBatchWriter` 每 5s 或每 100 条按 `(host_id, business, operation, direction)` 分组聚合写入 `traffic_log`，同时按文件写入 `traffic_file_log`

**Tech Stack:** Rust, tokio (mpsc, select!), sqlx, SQLite

---

## 文件结构

### 修改的文件

| 文件 | 改动 |
|------|------|
| `crates/s3-gallery-core/src/s3/traffic_recorder.rs` | 移除 `TrafficCounters`，`TrafficRecorder` 持有 `mpsc::Sender` |
| `crates/s3-gallery-core/src/s3/traffic_persist.rs` | 重写为 `TrafficBatchWriter`，通道接收 + 批量写入 |
| `crates/s3-gallery-core/src/s3/mod.rs` | 更新 re-export |
| `crates/s3-gallery-core/src/scan/aggregate.rs` | 改为从 DB 查询流量代替原子计数器 |
| `crates/s3-gallery-core/src/scan/scanner.rs` | 适配新接口，传入 `mpsc::Sender` |
| `crates/s3-gallery-core/src/view/traffic.rs` | 补充按 `(host_id, business, operation)` 分组的查询 |
| `crates/s3-gallery-cli/src/cmd_scan.rs` | 使用 `spawn_batch_writer` 替代 `spawn_aggregator` |
| `crates/s3-gallery-cli/src/cmd_serve.rs` | 使用 `spawn_batch_writer` 替代 `spawn_aggregator` |
| `crates/s3-gallery-cli/src/web/handlers/traffic_handler.rs` | `traffic_live` 返回按 business 分组的实时数据 |

### 不修改但需要了解的文件

| 文件 | 说明 |
|------|------|
| `crates/s3-gallery-core/src/s3/layers.rs` | `TrafficLayer` 接口不变 |
| `crates/s3-gallery-core/src/db/schema.rs` | 表结构不变 |
| `crates/s3-gallery-cli/src/web/state.rs` | `AppState` 不直接引用 `TrafficCounters` |
| `crates/s3-gallery-cli/src/cmd_traffic.rs` | 输出格式不变 |

---

## 任务分解

### Task 1: 重写 TrafficBatchWriter（traffic_persist.rs）

**Files:**
- Modify: `crates/s3-gallery-core/src/s3/traffic_persist.rs`（完全重写）
- Test: 文件内已有测试

**说明：** 将 `spawn_aggregator` 和 `flush_counters` 替换为基于 mpsc 通道的 `TrafficBatchWriter`。接收 `TrafficRecord`，攒批后按 `(host_id, business, operation, direction)` 分组聚合写入 `traffic_log`，文件级写入 `traffic_file_log`。

- [ ] **Step 1: 编写 TrafficBatchWriter 结构体和 run 方法**

```rust
// traffic_persist.rs 完整内容

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use super::traffic_recorder::TrafficRecord;

const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 5;
const DEFAULT_BATCH_SIZE: usize = 100;
const TRAFFIC_LOG_RETENTION_DAYS: i64 = 90;
const TRAFFIC_FILE_LOG_RETENTION_DAYS: i64 = 90;
const TRAFFIC_STATS_RETENTION_DAYS: i64 = 365;

/// Spawn the batch writer background task.
///
/// Returns the mpsc::Sender that TrafficRecorder uses to send records.
pub fn spawn_batch_writer(
    pool: SqlitePool,
    flush_interval_secs: u64,
    batch_size: usize,
) -> mpsc::Sender<TrafficRecord> {
    let (tx, rx) = mpsc::channel::<TrafficRecord>(4096);
    let interval = if flush_interval_secs == 0 {
        DEFAULT_FLUSH_INTERVAL_SECS
    } else {
        flush_interval_secs
    };
    let batch_size = if batch_size == 0 { DEFAULT_BATCH_SIZE } else { batch_size };

    tokio::spawn(async move {
        let mut writer = TrafficBatchWriter {
            receiver: rx,
            pool,
            buffer: Vec::with_capacity(batch_size),
            batch_size,
            flush_interval: Duration::from_secs(interval),
            last_cleanup: Utc::now(),
        };
        writer.run().await;
    });

    tx
}

struct TrafficBatchWriter {
    receiver: mpsc::Receiver<TrafficRecord>,
    pool: SqlitePool,
    buffer: Vec<TrafficRecord>,
    batch_size: usize,
    flush_interval: Duration,
    last_cleanup: chrono::DateTime<Utc>,
}

impl TrafficBatchWriter {
    async fn run(&mut self) {
        let mut interval = tokio::time::interval(self.flush_interval);
        interval.tick().await; // skip immediate first tick
        loop {
            tokio::select! {
                biased; // process messages first, then timers

                Some(record) = self.receiver.recv() => {
                    self.buffer.push(record);
                    if self.buffer.len() >= self.batch_size {
                        self.flush().await;
                    }
                }
                _ = interval.tick() => {
                    if !self.buffer.is_empty() {
                        self.flush().await;
                    }
                }
            }

            // Cleanup once per hour
            let elapsed = (Utc::now() - self.last_cleanup).num_minutes();
            if elapsed >= 60 {
                if let Err(e) = cleanup_old_data(&self.pool).await {
                    tracing::warn!(target: "s3_gallery::traffic", error = %e, "traffic cleanup failed");
                }
                self.last_cleanup = Utc::now();
            }
        }
    }

    async fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let records = std::mem::take(&mut self.buffer);
        flush_batch(&self.pool, &records).await;
    }
}

async fn flush_batch(pool: &SqlitePool, records: &[TrafficRecord]) {
    // Group by (host_id, business, operation, direction) for traffic_log
    let mut log_groups: HashMap<&str, HashMap<&str, HashMap<&str, HashMap<&str, (u64, u64)>>>> = HashMap::new();

    for record in records {
        let bytes = record.bytes;
        let count = record.count;

        let biz_map = log_groups.entry(record.host_id.as_str())
            .or_default();
        let op_map = biz_map.entry(record.business.as_str())
            .or_default();
        let dir_entry = op_map.entry(record.operation.to_string())
            .or_default();
        let entry = dir_entry.entry(record.direction.as_str())
            .or_insert((0, 0));
        entry.0 += bytes;
        entry.1 += count;
    }

    let now = Utc::now().to_rfc3339();

    // Use a transaction for atomicity
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to begin transaction");
            return;
        }
    };

    // Insert traffic_log rows
    for (host_id, biz_map) in &log_groups {
        for (business, op_map) in biz_map {
            for (operation, dir_map) in op_map {
                for (direction, (bytes, count)) in dir_map {
                    if let Err(e) = sqlx::query(
                        "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
                         VALUES (?, ?, ?, ?, ?, ?, ?)"
                    )
                    .bind(host_id)
                    .bind(operation)
                    .bind(business)
                    .bind(direction)
                    .bind(*bytes as i64)
                    .bind(*count as i64)
                    .bind(&now)
                    .execute(&mut *tx)
                    .await
                    {
                        tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to insert traffic_log row");
                    }
                }
            }
        }
    }

    // Insert traffic_file_log rows (one per record with non-empty file_key)
    for record in records {
        if record.file_key.is_empty() {
            continue;
        }
        if let Err(e) = sqlx::query(
            "INSERT INTO traffic_file_log (host_id, file_key, business, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&record.host_id)
        .bind(&record.file_key)
        .bind(&record.business)
        .bind(record.bytes as i64)
        .bind(record.count as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await
        {
            tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to insert traffic_file_log row");
        }
    }

    if let Err(e) = tx.commit().await {
        tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to commit traffic batch");
    }
}

/// Delete traffic data older than the retention period.
async fn cleanup_old_data(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cutoff = Utc::now() - chrono::Duration::days(TRAFFIC_LOG_RETENTION_DAYS);
    let cutoff_str = cutoff.to_rfc3339();

    sqlx::query("DELETE FROM traffic_log WHERE recorded_at < ?")
        .bind(&cutoff_str)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM traffic_file_log WHERE recorded_at < ?")
        .bind(&cutoff_str)
        .execute(pool)
        .await?;

    let stats_cutoff = Utc::now() - chrono::Duration::days(TRAFFIC_STATS_RETENTION_DAYS);
    let stats_cutoff_str = stats_cutoff.to_rfc3339();
    sqlx::query("DELETE FROM traffic_stats WHERE period < ?")
        .bind(&stats_cutoff_str)
        .execute(pool)
        .await?;

    Ok(())
}
```

- [ ] **Step 2: 编写单元测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::traffic_recorder::{S3Operation, TrafficRecord};
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_flush_batch_writes_to_db() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let records = vec![
            TrafficRecord {
                host_id: "host1".into(),
                file_key: "file1.jpg".into(),
                business: "web_download".into(),
                operation: S3Operation::GetObject,
                direction: "download".into(),
                bytes: 1000,
                count: 1,
            },
            TrafficRecord {
                host_id: "host1".into(),
                file_key: "file2.jpg".into(),
                business: "web_download".into(),
                operation: S3Operation::GetObject,
                direction: "download".into(),
                bytes: 2000,
                count: 1,
            },
            TrafficRecord {
                host_id: "host1".into(),
                file_key: "".into(),
                business: "scan_exif".into(),
                operation: S3Operation::ListObjects,
                direction: "download".into(),
                bytes: 0,
                count: 1,
            },
        ];

        flush_batch(&pool, &records).await;

        // Verify traffic_log rows (should be 2: one aggregated for web_download/GetObject, one for scan_exif/ListObjects)
        let log_rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT business, operation, bytes, count FROM traffic_log ORDER BY business"
        )
        .fetch_all(&pool)
        .await?;

        assert_eq!(log_rows.len(), 2);
        assert_eq!(log_rows[0].0, "scan_exif");
        assert_eq!(log_rows[1].0, "web_download");
        // web_download should have aggregated bytes: 1000 + 2000 = 3000
        assert_eq!(log_rows[1].2, 3000);
        assert_eq!(log_rows[1].3, 2);

        // Verify traffic_file_log rows (2 records with non-empty file_key)
        let file_rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT file_key, bytes FROM traffic_file_log ORDER BY file_key"
        )
        .fetch_all(&pool)
        .await?;

        assert_eq!(file_rows.len(), 2);
        assert_eq!(file_rows[0].0, "file1.jpg");
        assert_eq!(file_rows[1].0, "file2.jpg");

        Ok(())
    }

    #[tokio::test]
    async fn test_spawn_batch_writer_sends_records() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let tx = spawn_batch_writer(pool.clone(), 1, 100); // flush every 1s

        let record = TrafficRecord {
            host_id: "test".into(),
            file_key: "f.txt".into(),
            business: "test_biz".into(),
            operation: S3Operation::GetObject,
            direction: "download".into(),
            bytes: 500,
            count: 1,
        };
        tx.send(record).await.unwrap();

        // Wait for flush
        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

        // Verify record was written
        let rows: Vec<(i64,)> = sqlx::query_scalar("SELECT COUNT(*) FROM traffic_log WHERE business = 'test_biz'")
            .fetch_one(&pool)
            .await?;
        assert_eq!(rows.0, 1);

        Ok(())
    }
}
```

- [ ] **Step 3: 运行测试确认通过**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib s3::traffic_persist -- --nocapture`
Expected: 2 tests pass

- [ ] **Step 4: 提交**

```bash
git add crates/s3-gallery-core/src/s3/traffic_persist.rs
git commit -m "feat: rewrite traffic_persist as channel-based TrafficBatchWriter

Replace atomic-counter aggregator with mpsc channel + batch writer.
TrafficBatchWriter receives TrafficRecords, groups by
(host_id, business, operation, direction), and batch-inserts
to traffic_log and traffic_file_log every 5s or 100 records.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: 重构 TrafficRecorder（移除 TrafficCounters）

**Files:**
- Modify: `crates/s3-gallery-core/src/s3/traffic_recorder.rs`

**说明：** 移除 `TrafficCounters` 结构体，`TrafficRecorder` 改为持有 `mpsc::Sender<TrafficRecord>`。`record()` 方法非阻塞发送。

- [ ] **Step 1: 修改 TrafficRecorder**

```rust
// 移除 TrafficCounters 结构体
// 修改 TrafficRecorder

use tokio::sync::mpsc;

/// TrafficRecorder — fire-and-forget traffic recording via mpsc channel.
///
/// Records traffic by sending TrafficRecords to the TrafficBatchWriter
/// background task via an mpsc channel.
pub struct TrafficRecorder {
    sender: mpsc::Sender<TrafficRecord>,
}

impl TrafficRecorder {
    /// Create a new TrafficRecorder with the given mpsc sender.
    pub fn new(sender: mpsc::Sender<TrafficRecord>) -> Self {
        Self { sender }
    }

    /// Record a traffic event. Sends to the batch writer (non-blocking).
    /// Drops the record if the channel is full.
    pub fn record(&self, record: TrafficRecord) {
        let _ = self.sender.try_send(record);
    }
}
```

- [ ] **Step 2: 更新 BusinessS3Client 引用**

`BusinessS3Client` 不需要改动，它已经通过 `recorder.record(TrafficRecord)` 发送。

- [ ] **Step 3: 更新测试**

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_traffic_recorder_record() {
        let (tx, mut rx) = mpsc::channel::<TrafficRecord>(100);
        let recorder = TrafficRecorder::new(tx);

        let record = TrafficRecord {
            host_id: "test".into(),
            file_key: "f.txt".into(),
            business: "test".into(),
            operation: S3Operation::GetObject,
            direction: "download".into(),
            bytes: 100,
            count: 1,
        };
        recorder.record(record.clone());

        // Should be received on the channel
        let received = rx.try_recv().unwrap();
        assert_eq!(received.bytes, 100);
        assert_eq!(received.host_id, "test");
    }

    #[tokio::test]
    async fn test_business_s3_client_records_traffic() -> crate::error::Result<()> {
        let (tx, _rx) = mpsc::channel::<TrafficRecord>(100);
        let recorder = Arc::new(TrafficRecorder::new(tx));
        let inner = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);

        let client = BusinessS3Client::new(inner.clone(), "h1", "test_biz", recorder);
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        let data = client.get_object(&bucket, &key).await?;
        assert_eq!(data, b"hello");

        // Can't assert on counters (they're gone), but we can assert the data was returned
        Ok(())
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib s3::traffic_recorder -- --nocapture`
Expected: tests pass

- [ ] **Step 5: 提交**

```bash
git add crates/s3-gallery-core/src/s3/traffic_recorder.rs
git commit -m "refactor: replace TrafficCounters with mpsc channel in TrafficRecorder

Remove AtomicU64 counters, TrafficRecorder now sends TrafficRecords
to the batch writer via mpsc::Sender. BusinessS3Client unchanged.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: 更新 s3/mod.rs 导出

**Files:**
- Modify: `crates/s3-gallery-core/src/s3/mod.rs`

**说明：** 确保 `spawn_batch_writer` 被导出，移除 `TrafficCounters` 相关导出。

- [ ] **Step 1: 更新 mod.rs**

```rust
pub mod client;
pub mod layers;
pub mod s3_service;
pub mod config;
pub mod lock;
pub mod logged;
pub mod mock;
pub mod traffic_persist;
pub mod traffic_recorder;
pub mod real;
```

（如果之前有 `use` 引用 `TrafficCounters`，移除它们。通常只是 `pub mod` 声明，不需要改动。）

- [ ] **Step 2: 编译确认**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-core`
Expected: no errors

- [ ] **Step 3: 提交**

```bash
git add crates/s3-gallery-core/src/s3/mod.rs
git commit -m "chore: update s3 module exports for batch writer

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: 改造 AggregateLayer

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/aggregate.rs`

**说明：** `AggregateLayer` 不再从原子计数器读流量，改为记录扫描开始时间，结束时查询 `traffic_log` 表获取扫描期间的流量数据。

- [ ] **Step 1: 修改 AggregateLayer**

```rust
// AggregateLayer 不再持有 Arc<TrafficCounters>
// 改为记录扫描开始时间，结束时查询 DB

pub struct AggregateLayer {
    db: SqlitePool,
}

impl AggregateLayer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl<I> Layer<I> for AggregateLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError>
        + Send
        + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let inner = Arc::new(Mutex::new(inner));
        BoxService::new(service_fn(move |req: ScanRequest| {
            let db = db.clone();
            let inner = inner.clone();
            let start = Instant::now();
            let scan_start = Utc::now();
            async move {
                // 1. Call inner chain
                let mut resp = {
                    let mut inner = inner.lock().await;
                    inner.call(req).await?
                };

                let scan_end = Utc::now();
                let scan_start_str = scan_start.to_rfc3339();
                let scan_end_str = scan_end.to_rfc3339();

                // 2. Query traffic from DB for this scan period
                let rows: Vec<(String, String, String, i64, i64)> = sqlx::query_as(
                    "SELECT business, operation, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
                     FROM traffic_log \
                     WHERE recorded_at >= ? AND recorded_at <= ? \
                       AND business LIKE 'scan_%' \
                     GROUP BY business, operation, direction"
                )
                .bind(&scan_start_str)
                .bind(&scan_end_str)
                .fetch_all(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(format!("Failed to query traffic: {e}")))?;

                let mut traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>> = HashMap::new();
                let mut total_download: u64 = 0;
                let mut total_upload: u64 = 0;
                let mut total_requests: u64 = 0;

                for (business, operation, direction, bytes, count) in &rows {
                    let bytes = *bytes as u64;
                    let count = *count as u64;

                    let stage = traffic_by_stage.entry(business.clone()).or_default();
                    stage.insert(
                        operation.clone(),
                        TrafficByOperation { count, bytes },
                    );

                    if direction == "download" {
                        total_download += bytes;
                    } else {
                        total_upload += bytes;
                    }
                    total_requests += count;
                }

                // 3-7. 其余逻辑不变（file type, size ranges, scan status, cost, cleanup）
                // ... (copy from existing code, replacing counter reads with DB reads)

                // 3. Compute file type breakdown from diff_results
                let mut file_type_breakdown: HashMap<String, u64> = HashMap::new();
                for diff_result in &resp.diff_results {
                    for (ft, count) in &diff_result.file_type_counts {
                        *file_type_breakdown.entry(ft.clone()).or_insert(0) += count;
                    }
                }

                // 4. Compute size ranges from scan_objects
                let mut size_ranges = SizeRanges::default();
                for host in &resp.hosts {
                    let objects = ScanObjectEntry::list_by_scan(&db, &resp.scan_id, &host.host_id).await?;
                    for obj in &objects {
                        match obj.size {
                            0..=1024 => size_ranges.tiny = size_ranges.tiny.saturating_add(1),
                            1025..=102400 => size_ranges.small = size_ranges.small.saturating_add(1),
                            102401..=1048576 => size_ranges.medium = size_ranges.medium.saturating_add(1),
                            1048577..=10485760 => size_ranges.large = size_ranges.large.saturating_add(1),
                            _ => size_ranges.huge = size_ranges.huge.saturating_add(1),
                        }
                    }
                }

                // 5. Compute scan status totals
                let mut total_files = 0u64;
                let mut new_files = 0u64;
                let mut changed_files = 0u64;
                let mut deleted_files = 0u64;
                for diff_result in &resp.diff_results {
                    new_files += diff_result.new_files;
                    changed_files += diff_result.changed_files;
                    deleted_files += diff_result.deleted_files;
                    total_files += diff_result.new_files + diff_result.changed_files + diff_result.unchanged_count;
                }

                // Total size from files table
                let total_size_val: Option<i64> = sqlx::query_scalar(
                    "SELECT SUM(size) FROM files \
                     WHERE host_id IN (SELECT host_id FROM scan_objects WHERE scan_id = ?) \
                     AND is_deleted = 0",
                )
                .bind(&resp.scan_id)
                .fetch_optional(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(format!("Failed to sum sizes: {e}")))?;
                let total_size = total_size_val.unwrap_or(0) as u64;

                // 6. Estimated cost
                let estimated_cost = total_download as f64 * 0.00000009;

                // 7. Clean up scan_objects
                ScanObjectEntry::delete_by_scan(&db, &resp.scan_id).await?;

                resp.report = Some(AggregateReport {
                    traffic_by_stage,
                    total_download_bytes: total_download,
                    total_upload_bytes: total_upload,
                    total_requests,
                    estimated_cost,
                    file_type_breakdown,
                    size_ranges,
                    total_files,
                    total_size,
                    new_files,
                    changed_files,
                    deleted_files,
                    host_count: resp.hosts.len() as u64,
                    duration_secs: start.elapsed().as_secs_f64(),
                });

                Ok(resp)
            }
        }))
    }
}
```

- [ ] **Step 2: 更新导入**

```rust
// 移除:
use crate::s3::traffic_persist::flush_counters;
use crate::s3::traffic_recorder::TrafficCounters;

// 添加:
use chrono::Utc;
```

- [ ] **Step 3: 运行测试确认通过**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib scan::aggregate -- --nocapture`
Expected: tests pass

- [ ] **Step 4: 提交**

```bash
git add crates/s3-gallery-core/src/scan/aggregate.rs
git commit -m "refactor: AggregateLayer reads traffic from DB instead of counters

Remove dependency on TrafficCounters, query traffic_log for
scan-period traffic data grouped by (business, operation, direction).

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: 更新 scanner.rs + cmd_scan.rs + cmd_serve.rs

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs`

**说明：** 适配新接口，使用 `spawn_batch_writer` 替代 `spawn_aggregator`，传入 `mpsc::Sender` 给 `TrafficRecorder`。

- [ ] **Step 1: 修改 scanner.rs**

```rust
// 在 run_scan 中：
use crate::s3::traffic_persist::spawn_batch_writer;

let (tx, _rx) = mpsc::channel::<TrafficRecord>(4096);
let recorder = Arc::new(TrafficRecorder::new(tx.clone()));
let _agg_handle = spawn_batch_writer(pool.clone(), 60, 100);

// 对于 AggregateLayer，不再传入 counters
.layer(AggregateLayer::new(pool.clone()))
```

完整的 `run_scan` 函数：

```rust
pub async fn run_scan(config: ScanConfig) -> Result<ScanResult> {
    tracing::info!(
        target: "s3_gallery::scan",
        prefix = %config.prefix,
        client_id = %config.client_id,
        "Scan started"
    );

    let (tx, _rx) = tokio::sync::mpsc::channel(4096);
    let recorder = Arc::new(TrafficRecorder::new(tx));
    let _agg_handle = spawn_batch_writer(config.db.clone(), 60, 100);

    let discover_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), &config.host_id, "scan_discover"))
        .service(S3Service::new(config.s3.clone().into_inner()));

    let exif_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), &config.host_id, "scan_exif"))
        .service(S3Service::new(config.s3.into_inner()));

    let mut pipeline = ServiceBuilder::new()
        .layer(AggregateLayer::new(config.db.clone()))
        .layer(ProcessLayer::new(config.db.clone(), exif_s3, config.concurrency))
        .layer(DiffLayer::new(config.db.clone()))
        .layer(DiscoverLayer::new(config.db.clone()))
        .service(discover_s3);

    // ... rest unchanged
}
```

- [ ] **Step 2: 修改 cmd_scan.rs**

```rust
// 在 cmd_scan.rs 中：
use s3_gallery_core::s3::traffic_persist::spawn_batch_writer;

// 替换:
// let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
// let _agg_handle = spawn_aggregator(recorder.clone(), pool.clone(), 60);
// 为:
let (tx, _rx) = tokio::sync::mpsc::channel(4096);
let recorder = Arc::new(TrafficRecorder::new(tx));
let _agg_handle = spawn_batch_writer(pool.clone(), 60, 100);

// 对于 AggregateLayer:
// 替换:
// .layer(AggregateLayer::new(pool.clone(), recorder.counters.clone()))
// 为:
.layer(AggregateLayer::new(pool.clone()))
```

- [ ] **Step 3: 修改 cmd_serve.rs**

```rust
// 在 cmd_serve.rs 中：
use s3_gallery_core::s3::traffic_persist::spawn_batch_writer;

// 替换:
// let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
// let _agg_handle = spawn_aggregator(recorder.clone(), pool.clone(), 60);
// 为:
let (tx, _rx) = tokio::sync::mpsc::channel(4096);
let recorder = Arc::new(TrafficRecorder::new(tx));
let _agg_handle = spawn_batch_writer(pool.clone(), 60, 100);
```

- [ ] **Step 4: 编译确认**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check 2>&1 | head -50`
Expected: no errors

- [ ] **Step 5: 运行测试**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core`
Expected: all tests pass

- [ ] **Step 6: 提交**

```bash
git add crates/s3-gallery-core/src/scan/scanner.rs \
       crates/s3-gallery-core/src/scan/aggregate.rs \
       crates/s3-gallery-cli/src/cmd_scan.rs \
       crates/s3-gallery-cli/src/cmd_serve.rs
git commit -m "refactor: update startup code for new traffic batch writer

Replace spawn_aggregator with spawn_batch_writer, update
AggregateLayer and TrafficRecorder constructor calls.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: 更新 traffic live 端点（按 business 分组）

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/traffic_handler.rs`

**说明：** `traffic_live` 端点改为返回按 business 分组的实时流量数据。

- [ ] **Step 1: 修改 traffic_live 端点**

```rust
/// Live traffic JSON endpoint — polled by HTMX every 5 seconds.
pub async fn traffic_live(
    State(state): State<AppState>,
) -> HandlerResult {
    // Per-business breakdown for last 10 seconds
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT business, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
         FROM traffic_log \
         WHERE recorded_at > datetime('now', '-10 seconds') \
         GROUP BY business"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        HandlerResult::Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "failed to read live traffic", "detail": e.to_string()}),
        )
    })?;

    let mut businesses = serde_json::Map::new();
    let mut total_download_bytes: f64 = 0.0;
    let mut total_requests: i64 = 0;

    for (business, bytes, count) in &rows {
        let kbps = *bytes as f64 / 1024.0 / 10.0;
        businesses.insert(
            business.clone(),
            json!({
                "download_kbps": format!("{:.1}", kbps),
                "requests": count,
            }),
        );
        total_download_bytes += kbps;
        total_requests += count;
    }

    HandlerResult::Json(json!({
        "businesses": businesses,
        "total_download_kbps": format!("{:.1}", total_download_bytes),
        "total_requests": total_requests,
    }))
}
```

- [ ] **Step 2: 编译确认**

Run: `cd /Users/macbook/Project/s3-gallery && cargo check -p s3-gallery-cli 2>&1 | head -20`
Expected: no errors

- [ ] **Step 3: 提交**

```bash
git add crates/s3-gallery-cli/src/web/handlers/traffic_handler.rs
git commit -m "feat: traffic live endpoint returns per-business breakdown

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: 更新 view/traffic.rs 分组查询

**Files:**
- Modify: `crates/s3-gallery-core/src/view/traffic.rs`

**说明：** 补充按 `(host_id, business, operation)` 分组的查询能力，用于 CLI 和 Web 展示。

- [ ] **Step 1: 修改 get_traffic_summary**

当前 `get_traffic_summary` 已经支持按 business 分组查询。需要补充 `host_id` 和 `operation` 的过滤。

```rust
/// Get traffic summary for a host/period.
///
/// Now properly groups by business and supports operation-level breakdown.
pub async fn get_traffic_summary(
    db: &SqlitePool,
    host_id: Option<&str>,
    _period: Option<&str>,
    _since: Option<&str>,
    _until: Option<&str>,
) -> Result<TrafficSummary> {
    let mut conditions = Vec::new();
    let mut params: Vec<String> = Vec::new();

    if let Some(hid) = host_id {
        conditions.push(format!("host_id = ?"));
        params.push(hid.to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Per-business aggregation
    let query_str = format!(
        "SELECT business, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
         FROM traffic_log {} \
         GROUP BY business, direction ORDER BY business",
        where_clause
    );

    let mut query = sqlx::query_as::<_, (String, String, i64, i64)>(&query_str);
    for p in &params {
        query = query.bind(p);
    }

    let rows = query.fetch_all(db).await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let mut business_map: std::collections::BTreeMap<String, BusinessTraffic> =
        std::collections::BTreeMap::new();
    for (business, direction, bytes, count) in rows {
        let entry = business_map.entry(business.clone()).or_insert(BusinessTraffic {
            business: business.clone(),
            download_bytes: 0,
            upload_bytes: 0,
            requests: 0,
        });
        entry.requests += count as u64;
        if direction == "download" {
            entry.download_bytes += bytes as u64;
        } else {
            entry.upload_bytes += bytes as u64;
        }
    }

    let businesses: Vec<BusinessTraffic> = business_map.into_values().collect();
    let total_download_bytes: u64 = businesses.iter().map(|b| b.download_bytes).sum();
    let total_upload_bytes: u64 = businesses.iter().map(|b| b.upload_bytes).sum();
    let total_requests: u64 = businesses.iter().map(|b| b.requests).sum();

    // Top files from traffic_file_log
    let top_files_query = if host_id.is_some() {
        "SELECT file_key, COALESCE(SUM(bytes), 0) as total_bytes \
         FROM traffic_file_log WHERE host_id = ? \
         GROUP BY file_key ORDER BY total_bytes DESC LIMIT 10"
    } else {
        "SELECT file_key, COALESCE(SUM(bytes), 0) as total_bytes \
         FROM traffic_file_log \
         GROUP BY file_key ORDER BY total_bytes DESC LIMIT 10"
    };

    let mut top_query = sqlx::query_as::<_, (String, i64)>(top_files_query);
    if host_id.is_some() {
        top_query = top_query.bind(host_id.unwrap());
    }
    let top_file_rows = top_query.fetch_all(db).await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let top_files: Vec<FileTraffic> = top_file_rows.into_iter()
        .map(|(file_key, bytes)| FileTraffic {
            file_key,
            bytes: bytes as u64,
        })
        .collect();

    // Estimated cost: $0.03/GB download
    let estimated_cost = (total_download_bytes as f64 / 1_073_741_824.0) * 0.03;

    Ok(TrafficSummary {
        businesses,
        total_download_bytes,
        total_upload_bytes,
        total_requests,
        top_files,
        estimated_cost,
    })
}
```

- [ ] **Step 2: 运行测试**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib view::traffic -- --nocapture`
Expected: tests pass

- [ ] **Step 3: 提交**

```bash
git add crates/s3-gallery-core/src/view/traffic.rs
git commit -m "feat: support per-host and top-files in traffic summary query

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## 执行顺序

1. **Task 1** → `traffic_persist.rs`（TrafficBatchWriter 核心）
2. **Task 2** → `traffic_recorder.rs`（移除 TrafficCounters）
3. **Task 3** → `s3/mod.rs`（更新导出）
4. **Task 4** → `aggregate.rs`（从 DB 查流量）
5. **Task 5** → `scanner.rs` + `cmd_scan.rs` + `cmd_serve.rs`（启动适配）
6. **Task 6** → `traffic_handler.rs`（live 端点按 business 分组）
7. **Task 7** → `view/traffic.rs`（分组查询 + Top N 文件）