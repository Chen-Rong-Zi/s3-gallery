//! Background traffic aggregator — batches TrafficRecords from the mpsc channel
//! and writes them to the database every 60 seconds.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;

use super::traffic_recorder::TrafficRecorder;

/// Default aggregation interval in seconds.
const AGGREGATION_INTERVAL_SECS: u64 = 60;

/// Retention: traffic_log and traffic_file_log kept for 90 days.
const TRAFFIC_LOG_RETENTION_DAYS: i64 = 90;
/// Retention: traffic_stats kept for 12 months.
const TRAFFIC_STATS_RETENTION_DAYS: i64 = 365;

/// Spawn the background aggregator task.
///
/// Reads from the TrafficRecorder's counters every `interval_secs` seconds,
/// batches records, and writes to the traffic_log, traffic_file_log, and
/// traffic_stats tables. Also performs periodic cleanup of old data.
pub fn spawn_aggregator(
    recorder: Arc<TrafficRecorder>,
    pool: SqlitePool,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    let interval = if interval_secs == 0 {
        AGGREGATION_INTERVAL_SECS
    } else {
        interval_secs
    };

    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(Duration::from_secs(interval));
        let counters = recorder.counters.clone();

        loop {
            interval_timer.tick().await;

            // Read and reset counters atomically
            let download = counters.download_bytes.swap(0, Ordering::AcqRel);
            let upload = counters.upload_bytes.swap(0, Ordering::AcqRel);
            let count = counters.request_count.swap(0, Ordering::AcqRel);

            if download == 0 && upload == 0 && count == 0 {
                // No traffic this interval — still run cleanup
                if let Err(e) = cleanup_old_data(&pool).await {
                    tracing::warn!(target: "s3_gallery::traffic", error = %e, "traffic cleanup failed");
                }
                continue;
            }

            // Write a summary record to traffic_log
            let now = Utc::now().to_rfc3339();
            let result = sqlx::query(
                "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
                 VALUES ('__aggregated', '__batch', '__aggregated', 'download', ?, ?, ?)",
            )
            .bind(download as i64)
            .bind(count as i64)
            .bind(&now)
            .execute(&pool)
            .await;

            if let Err(e) = result {
                tracing::warn!(target: "s3_gallery::traffic", error = %e, "failed to write aggregated traffic");
            }

            // Cleanup old data
            if let Err(e) = cleanup_old_data(&pool).await {
                tracing::warn!(target: "s3_gallery::traffic", error = %e, "traffic cleanup failed");
            }
        }
    })
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