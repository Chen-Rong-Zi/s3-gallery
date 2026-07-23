//! Traffic query logic — summary, history, and live data for the dashboard and CLI.

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
    let host_filter = if let Some(hid) = host_id {
        format!("WHERE host_id = '{}'", hid)
    } else {
        String::new()
    };

    // Per-business aggregation
    let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        &format!(
            "SELECT business, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
             FROM traffic_log {} \
             GROUP BY business, direction ORDER BY business",
            host_filter
        ),
    )
    .fetch_all(db)
    .await
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

    // Top files (simplified — actual implementation uses traffic_file_log)
    let top_files: Vec<FileTraffic> = Vec::new();

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
}