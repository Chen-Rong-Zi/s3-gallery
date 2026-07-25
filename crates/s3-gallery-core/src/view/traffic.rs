//! Traffic query logic — summary, history, and live data for the dashboard and CLI.
//!
//! Now supports per-business breakdown, per-host filtering, and top files.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};

/// Traffic summary — per-business breakdown and top files.
#[derive(Debug, Clone, Serialize)]
pub struct TrafficSummary {
    pub businesses: Vec<BusinessTraffic>,
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub total_requests: u64,
    pub top_files: Vec<FileTraffic>,
    pub estimated_cost: f64,
}

/// Traffic per business layer.
#[derive(Debug, Clone, Serialize)]
pub struct BusinessTraffic {
    pub business: String,
    pub download_bytes: u64,
    pub upload_bytes: u64,
    pub requests: u64,
}

/// Traffic for a single file.
#[derive(Debug, Clone, Serialize)]
pub struct FileTraffic {
    pub file_key: String,
    pub bytes: u64,
}

/// Get traffic summary for a host/period.
///
/// Groups by business, supports per-host filtering, and returns top files
/// by total bytes from traffic_file_log.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_traffic_summary(
    db: &SqlitePool,
    host_id: Option<&str>,
    _period: Option<&str>,
    _since: Option<&str>,
    _until: Option<&str>,
) -> Result<TrafficSummary> {
    // Build WHERE clause for host_id filter
    let (host_filter, param_used) = if let Some(_hid) = host_id {
        (format!("WHERE host_id = ?"), true)
    } else {
        (String::new(), false)
    };

    // Per-business aggregation
    let query_str = format!(
        "SELECT business, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
         FROM traffic_log {} \
         GROUP BY business, direction ORDER BY business",
        host_filter
    );

    let rows: Vec<(String, String, i64, i64)> = if param_used {
        sqlx::query_as(&query_str)
            .bind(host_id.unwrap())
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        sqlx::query_as(&query_str)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    };

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
    let top_files = get_top_files(db, host_id).await?;

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

/// Query the top 10 files by total bytes transferred.
async fn get_top_files(
    db: &SqlitePool,
    host_id: Option<&str>,
) -> Result<Vec<FileTraffic>> {
    if host_id.is_some() {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT file_key, COALESCE(SUM(bytes), 0) as total_bytes \
             FROM traffic_file_log WHERE host_id = ? \
             GROUP BY file_key ORDER BY total_bytes DESC LIMIT 10",
        )
        .bind(host_id.unwrap())
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|(file_key, bytes)| FileTraffic {
                file_key,
                bytes: bytes as u64,
            })
            .collect())
    } else {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT file_key, COALESCE(SUM(bytes), 0) as total_bytes \
             FROM traffic_file_log \
             GROUP BY file_key ORDER BY total_bytes DESC LIMIT 10",
        )
        .fetch_all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|(file_key, bytes)| FileTraffic {
                file_key,
                bytes: bytes as u64,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_get_traffic_summary_empty() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let summary = get_traffic_summary(&pool, None, None, None, None).await?;
        assert!(summary.businesses.is_empty());
        assert!(summary.top_files.is_empty());
        assert_eq!(summary.total_download_bytes, 0);
        assert_eq!(summary.total_upload_bytes, 0);
        assert_eq!(summary.total_requests, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_traffic_summary_with_data() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        // Insert some test traffic data
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("host1")
        .bind("GetObject")
        .bind("web_download")
        .bind("download")
        .bind(1000i64)
        .bind(1i64)
        .bind(&now)
        .execute(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("host1")
        .bind("GetObject")
        .bind("web_download")
        .bind("download")
        .bind(2000i64)
        .bind(1i64)
        .bind(&now)
        .execute(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("host1")
        .bind("ListObjects")
        .bind("scan_discover")
        .bind("download")
        .bind(0i64)
        .bind(1i64)
        .bind(&now)
        .execute(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        // Insert file-level traffic
        sqlx::query(
            "INSERT INTO traffic_file_log (host_id, file_key, business, bytes, count, recorded_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("host1")
        .bind("bigfile.mp4")
        .bind("web_download")
        .bind(500_000_000i64)
        .bind(1i64)
        .bind(&now)
        .execute(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let summary = get_traffic_summary(&pool, None, None, None, None).await?;

        // Should have 2 businesses
        assert_eq!(summary.businesses.len(), 2);
        assert_eq!(summary.businesses[0].business, "scan_discover");
        assert_eq!(summary.businesses[1].business, "web_download");

        // web_download should have 3000 bytes total
        let web_download = &summary.businesses[1];
        assert_eq!(web_download.download_bytes, 3000);
        assert_eq!(web_download.requests, 2);

        // Should have top files
        assert_eq!(summary.top_files.len(), 1);
        assert_eq!(summary.top_files[0].file_key, "bigfile.mp4");
        assert_eq!(summary.top_files[0].bytes, 500_000_000);

        Ok(())
    }
}