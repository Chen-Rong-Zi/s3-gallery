//! Traffic query logic — summary, history, and live data for the dashboard and CLI.
//!
//! Now supports per-business breakdown, per-host filtering, and top files.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

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
    db: &DatabaseConnection,
    host_id: Option<&str>,
    _period: Option<&str>,
    _since: Option<&str>,
    _until: Option<&str>,
) -> Result<TrafficSummary> {
    // Per-business aggregation
    let (query_str, values): (String, Vec<sea_orm::Value>) = if let Some(hid) = host_id {
        (
            "SELECT business, direction, COALESCE(SUM(bytes), 0) as total_bytes, COALESCE(SUM(count), 0) as total_count \
             FROM traffic_log WHERE host_id = ? \
             GROUP BY business, direction ORDER BY business"
                .to_string(),
            vec![hid.into()],
        )
    } else {
        (
            "SELECT business, direction, COALESCE(SUM(bytes), 0) as total_bytes, COALESCE(SUM(count), 0) as total_count \
             FROM traffic_log \
             GROUP BY business, direction ORDER BY business"
                .to_string(),
            Vec::new(),
        )
    };

    let stmt = if values.is_empty() {
        Statement::from_string(DbBackend::Sqlite, query_str)
    } else {
        Statement::from_sql_and_values(DbBackend::Sqlite, query_str, values)
    };

    let rows = db
        .query_all(stmt)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let mut business_map: std::collections::BTreeMap<String, BusinessTraffic> =
        std::collections::BTreeMap::new();
    for row in &rows {
        let business: String = row
            .try_get("", "business")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let direction: String = row
            .try_get("", "direction")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let bytes: i64 = row
            .try_get("", "total_bytes")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let count: i64 = row
            .try_get("", "total_count")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let entry = business_map
            .entry(business.clone())
            .or_insert(BusinessTraffic {
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
async fn get_top_files(db: &DatabaseConnection, host_id: Option<&str>) -> Result<Vec<FileTraffic>> {
    let (query_str, values): (String, Vec<sea_orm::Value>) = if let Some(hid) = host_id {
        (
            "SELECT file_key, COALESCE(SUM(bytes), 0) as total_bytes \
             FROM traffic_file_log WHERE host_id = ? \
             GROUP BY file_key ORDER BY total_bytes DESC LIMIT 10"
                .to_string(),
            vec![hid.into()],
        )
    } else {
        (
            "SELECT file_key, COALESCE(SUM(bytes), 0) as total_bytes \
             FROM traffic_file_log \
             GROUP BY file_key ORDER BY total_bytes DESC LIMIT 10"
                .to_string(),
            Vec::new(),
        )
    };

    let stmt = if values.is_empty() {
        Statement::from_string(DbBackend::Sqlite, query_str)
    } else {
        Statement::from_sql_and_values(DbBackend::Sqlite, query_str, values)
    };

    let rows = db
        .query_all(stmt)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let mut result = Vec::new();
    for row in &rows {
        let file_key: String = row
            .try_get("", "file_key")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let bytes: i64 = row
            .try_get("", "total_bytes")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        result.push(FileTraffic {
            file_key,
            bytes: bytes as u64,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::run_full_migration;
    use crate::db::pool::create_pool;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_get_traffic_summary_empty() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        run_full_migration(&db).await?;

        let summary = get_traffic_summary(&db, None, None, None, None).await?;
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
        let db = create_pool(&db_path).await?;
        run_full_migration(&db).await?;

        // Insert some test traffic data using raw SQL via SeaORM
        let now = chrono::Utc::now().to_rfc3339();
        let insert_sql = "INSERT INTO traffic_log (host_id, operation, business, direction, bytes, count, recorded_at) \
                          VALUES (?, ?, ?, ?, ?, ?, ?)";
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            insert_sql,
            vec![
                "host1".into(),
                "GetObject".into(),
                "web_download".into(),
                "download".into(),
                1000i64.into(),
                1i64.into(),
                now.clone().into(),
            ],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            insert_sql,
            vec![
                "host1".into(),
                "GetObject".into(),
                "web_download".into(),
                "download".into(),
                2000i64.into(),
                1i64.into(),
                now.clone().into(),
            ],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            insert_sql,
            vec![
                "host1".into(),
                "ListObjects".into(),
                "scan_discover".into(),
                "download".into(),
                0i64.into(),
                1i64.into(),
                now.clone().into(),
            ],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        // Insert file-level traffic
        let file_insert_sql = "INSERT INTO traffic_file_log (host_id, file_key, business, bytes, count, recorded_at) \
                               VALUES (?, ?, ?, ?, ?, ?)";
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            file_insert_sql,
            vec![
                "host1".into(),
                "bigfile.mp4".into(),
                "web_download".into(),
                500_000_000i64.into(),
                1i64.into(),
                now.into(),
            ],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let summary = get_traffic_summary(&db, None, None, None, None).await?;

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
