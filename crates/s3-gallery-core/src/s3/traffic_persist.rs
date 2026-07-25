//! Background traffic batch writer — receives TrafficRecords via mpsc channel,
//! batches them, and writes grouped records to traffic_log and traffic_file_log.
//!
//! Spawned by `spawn_batch_writer()`, which returns the mpsc::Sender side.
//! The sender is passed to TrafficRecorder, and BusinessS3Client sends records
//! through it on every successful S3 operation.
//!
//! Every 5 seconds or every 100 records, the batch is flushed to the database:
//! - traffic_log: grouped by (host_id, business, operation, direction) with aggregated bytes/count
//! - traffic_file_log: one row per record with non-empty file_key

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use super::traffic_recorder::TrafficRecord;

const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 5;
const DEFAULT_BATCH_SIZE: usize = 100;
const CHANNEL_CAPACITY: usize = 4096;
const TRAFFIC_LOG_RETENTION_DAYS: i64 = 90;
const TRAFFIC_STATS_RETENTION_DAYS: i64 = 365;

/// Spawn the batch writer background task.
///
/// Returns the mpsc::Sender that TrafficRecorder uses to send records.
/// When `flush_interval_secs` or `batch_size` is 0, defaults are used.
pub fn spawn_batch_writer(
    pool: SqlitePool,
    flush_interval_secs: u64,
    batch_size: usize,
) -> mpsc::Sender<TrafficRecord> {
    let (tx, rx) = mpsc::channel::<TrafficRecord>(CHANNEL_CAPACITY);
    let interval = if flush_interval_secs == 0 {
        DEFAULT_FLUSH_INTERVAL_SECS
    } else {
        flush_interval_secs
    };
    let batch_size = if batch_size == 0 {
        DEFAULT_BATCH_SIZE
    } else {
        batch_size
    };

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

/// Internal batch writer that receives and persists traffic records.
struct TrafficBatchWriter {
    receiver: mpsc::Receiver<TrafficRecord>,
    pool: SqlitePool,
    buffer: Vec<TrafficRecord>,
    batch_size: usize,
    flush_interval: Duration,
    last_cleanup: chrono::DateTime<Utc>,
}

impl TrafficBatchWriter {
    /// Main loop: receive records and flush on timer or batch size.
    async fn run(&mut self) {
        let mut interval = tokio::time::interval(self.flush_interval);
        // Skip the immediate first tick so the first flush is timer-driven
        interval.tick().await;

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

            // Channel closed — flush remaining and exit
            if self.receiver.is_closed() && !self.buffer.is_empty() {
                self.flush().await;
                break;
            }
            if self.receiver.is_closed() {
                break;
            }

            // Cleanup old data once per hour
            let elapsed = (Utc::now() - self.last_cleanup).num_minutes();
            if elapsed >= 60 {
                if let Err(e) = cleanup_old_data(&self.pool).await {
                    tracing::warn!(target: "s3_gallery::traffic", error = %e, "traffic cleanup failed");
                }
                self.last_cleanup = Utc::now();
            }
        }
    }

    /// Drain the buffer and write all records to the database.
    async fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let records = std::mem::take(&mut self.buffer);
        flush_batch(&self.pool, &records).await;
    }
}

/// Write a batch of traffic records to the database.
///
/// Groups records by (host_id, business, operation, direction) for traffic_log
/// aggregation, and writes per-file records to traffic_file_log.
async fn flush_batch(pool: &SqlitePool, records: &[TrafficRecord]) {
    // Group by (host_id, business, operation, direction) for traffic_log
    #[derive(Hash, PartialEq, Eq)]
    struct LogGroupKey {
        host_id: String,
        business: String,
        operation: String,
        direction: String,
    }

    let mut log_groups: HashMap<LogGroupKey, (u64, u64)> = HashMap::new();

    for record in records {
        let key = LogGroupKey {
            host_id: record.host_id.clone(),
            business: record.business.clone(),
            operation: record.operation.to_string(),
            direction: record.direction.clone(),
        };
        let entry = log_groups.entry(key).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(record.bytes);
        entry.1 = entry.1.saturating_add(record.count);
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

    // Insert traffic_log rows (grouped)
    for (key, (bytes, count)) in &log_groups {
        if let Err(e) = sqlx::query(
            "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&key.host_id)
        .bind(&key.operation)
        .bind(&key.business)
        .bind(&key.direction)
        .bind(*bytes as i64)
        .bind(*count as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await
        {
            tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to insert traffic_log row");
        }
    }

    // Insert traffic_file_log rows (one per record with non-empty file_key)
    for record in records {
        if record.file_key.is_empty() {
            continue;
        }
        if let Err(e) = sqlx::query(
            "INSERT INTO traffic_file_log (host_id, file_key, business, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
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
///
/// # Errors
///
/// Returns `sqlx::Error` if any of the DELETE queries fail.
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::traffic_recorder::{S3Operation, TrafficRecord};
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

        // Verify traffic_log rows (should be 2 grouped rows)
        let log_rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT business, operation, bytes, count FROM traffic_log ORDER BY business",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;

        assert_eq!(log_rows.len(), 2, "should have 2 grouped traffic_log rows");
        assert_eq!(log_rows[0].0, "scan_exif");
        assert_eq!(log_rows[0].1, "ListObjects");
        assert_eq!(log_rows[1].0, "web_download");
        assert_eq!(log_rows[1].1, "GetObject");
        // web_download should have aggregated bytes: 1000 + 2000 = 3000
        assert_eq!(log_rows[1].2, 3000, "web_download bytes should be aggregated");
        assert_eq!(log_rows[1].3, 2, "web_download count should be aggregated");

        // Verify traffic_file_log rows (2 records with non-empty file_key)
        let file_rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT file_key, bytes FROM traffic_file_log ORDER BY file_key",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;

        assert_eq!(file_rows.len(), 2, "should have 2 file-level rows");
        assert_eq!(file_rows[0].0, "file1.jpg");
        assert_eq!(file_rows[0].1, 1000);
        assert_eq!(file_rows[1].0, "file2.jpg");
        assert_eq!(file_rows[1].1, 2000);

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
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM traffic_log WHERE business = 'test_biz'",
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;
        assert_eq!(count, 1, "record should be persisted after flush");

        Ok(())
    }
}