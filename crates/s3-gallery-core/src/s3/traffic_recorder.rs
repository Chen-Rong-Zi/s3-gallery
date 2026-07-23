//! Traffic tracking types and real-time counters.
//!
//! TrafficRecord, TrafficCounters, S3Operation, and BusinessS3Client.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::s3::client::{ObjectMetadata, ObjectSummary, S3Client};
use crate::s3::s3_service::S3Request;
use crate::types::{BucketName, ObjectKey};

/// A single traffic record — created by BusinessS3Client on successful S3 operations.
#[derive(Debug, Clone)]
pub struct TrafficRecord {
    pub host_id: String,
    /// Empty for list/head/delete operations that don't target a single file.
    pub file_key: String,
    pub business: String,
    pub operation: S3Operation,
    pub direction: String,
    pub bytes: u64,
    pub count: u64,
}

/// S3Client operations — used as index into `per_operation` array.
/// Must match the order of S3Request variants.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Operation {
    GetObject = 0,
    GetObjectRange = 1,
    PutObject = 2,
    PutObjectIfNoneMatch = 3,
    ListObjects = 4,
    HeadObject = 5,
    DeleteObject = 6,
    ObjectExists = 7,
}

impl S3Operation {
    pub fn from_request(req: &S3Request) -> Self {
        match req {
            S3Request::GetObject(..) => Self::GetObject,
            S3Request::GetObjectRange(..) => Self::GetObjectRange,
            S3Request::PutObject(..) => Self::PutObject,
            S3Request::PutObjectIfNoneMatch(..) => Self::PutObjectIfNoneMatch,
            S3Request::ListObjects(..) => Self::ListObjects,
            S3Request::HeadObject(..) => Self::HeadObject,
            S3Request::DeleteObject(..) => Self::DeleteObject,
            S3Request::ObjectExists(..) => Self::ObjectExists,
        }
    }
}

impl std::fmt::Display for S3Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GetObject => write!(f, "GetObject"),
            Self::GetObjectRange => write!(f, "GetObjectRange"),
            Self::PutObject => write!(f, "PutObject"),
            Self::PutObjectIfNoneMatch => write!(f, "PutObjectIfNoneMatch"),
            Self::ListObjects => write!(f, "ListObjects"),
            Self::HeadObject => write!(f, "HeadObject"),
            Self::DeleteObject => write!(f, "DeleteObject"),
            Self::ObjectExists => write!(f, "ObjectExists"),
        }
    }
}

/// Real-time traffic counters using AtomicU64.
pub struct TrafficCounters {
    pub download_bytes: AtomicU64,
    pub upload_bytes: AtomicU64,
    pub request_count: AtomicU64,
    pub per_operation: [AtomicU64; 8],
}

impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            download_bytes: AtomicU64::new(0),
            upload_bytes: AtomicU64::new(0),
            request_count: AtomicU64::new(0),
            per_operation: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub fn record(&self, record: &TrafficRecord) {
        self.request_count.fetch_add(record.count, Ordering::Relaxed);
        if record.direction == "download" {
            self.download_bytes
                .fetch_add(record.bytes, Ordering::Relaxed);
        } else {
            self.upload_bytes
                .fetch_add(record.bytes, Ordering::Relaxed);
        }
        self.per_operation[record.operation as usize]
            .fetch_add(record.bytes, Ordering::Relaxed);
    }
}

/// TrafficRecorder — fire-and-forget traffic recording via atomic counters.
///
/// Records traffic to AtomicU64 counters for real-time access. The background
/// aggregator (spawn_aggregator in traffic_persist.rs) periodically reads and
/// resets these counters, writing aggregated records to the database.
pub struct TrafficRecorder {
    pub counters: Arc<TrafficCounters>,
}

impl TrafficRecorder {
    /// Create a new TrafficRecorder.
    pub fn new(_pool: sqlx::SqlitePool) -> Self {
        Self {
            counters: Arc::new(TrafficCounters::new()),
        }
    }

    /// Record a traffic event. Updates atomic counters (non-blocking).
    pub fn record(&self, record: TrafficRecord) {
        self.counters.record(&record);
    }
}

/// BusinessS3Client wraps an S3Client and records traffic on success.
///
/// Carries a `business` label (e.g. "web_download", "exif_extraction")
/// and a `host_id` so traffic can be attributed per-business-layer.
pub struct BusinessS3Client {
    inner: Arc<dyn S3Client>,
    recorder: Arc<TrafficRecorder>,
    host_id: String,
    business: String,
}

impl BusinessS3Client {
    pub fn new(
        inner: Arc<dyn S3Client>,
        host_id: &str,
        business: &str,
        recorder: Arc<TrafficRecorder>,
    ) -> Self {
        Self {
            inner,
            recorder,
            host_id: host_id.to_string(),
            business: business.to_string(),
        }
    }

    fn record_traffic(&self, operation: S3Operation, bytes: u64, file_key: &str) {
        let direction = match operation {
            S3Operation::GetObject | S3Operation::GetObjectRange | S3Operation::HeadObject => {
                "download"
            }
            S3Operation::PutObject | S3Operation::PutObjectIfNoneMatch => "upload",
            S3Operation::ListObjects | S3Operation::DeleteObject | S3Operation::ObjectExists => {
                "download"
            }
        };
        self.recorder.record(TrafficRecord {
            host_id: self.host_id.clone(),
            file_key: file_key.to_string(),
            business: self.business.clone(),
            operation,
            direction: direction.to_string(),
            bytes,
            count: 1,
        });
    }
}

#[async_trait]
impl S3Client for BusinessS3Client {
    async fn list_objects(
        &self,
        bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ObjectSummary>> {
        let result = self.inner.list_objects(bucket, prefix).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::ListObjects, 0, "");
        }
        result
    }

    async fn head_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<ObjectMetadata> {
        let result = self.inner.head_object(bucket, key).await;
        if let Ok(ref meta) = result {
            self.record_traffic(S3Operation::HeadObject, meta.size.as_u64(), key.as_str());
        }
        result
    }

    async fn get_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>> {
        let result = self.inner.get_object(bucket, key).await;
        if let Ok(ref data) = result {
            self.record_traffic(S3Operation::GetObject, data.len() as u64, key.as_str());
        }
        result
    }

    async fn get_object_range(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let result = self.inner.get_object_range(bucket, key, start, end).await;
        if let Ok(ref data) = result {
            self.record_traffic(
                S3Operation::GetObjectRange,
                data.len() as u64,
                key.as_str(),
            );
        }
        result
    }

    async fn put_object(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<()> {
        let result = self.inner.put_object(bucket, key, body).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::PutObject, body.len() as u64, key.as_str());
        }
        result
    }

    async fn put_object_if_none_match(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool> {
        let result = self.inner.put_object_if_none_match(bucket, key, body).await;
        if let Ok(true) = result {
            self.record_traffic(
                S3Operation::PutObjectIfNoneMatch,
                body.len() as u64,
                key.as_str(),
            );
        }
        result
    }

    async fn delete_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<()> {
        let result = self.inner.delete_object(bucket, key).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::DeleteObject, 0, key.as_str());
        }
        result
    }

    async fn object_exists(&self, bucket: &BucketName, key: &ObjectKey) -> Result<bool> {
        let result = self.inner.object_exists(bucket, key).await;
        if result.is_ok() {
            self.record_traffic(S3Operation::ObjectExists, 0, key.as_str());
        }
        result
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn test_traffic_counters_new() {
        let c = TrafficCounters::new();
        assert_eq!(c.download_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(c.upload_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(c.request_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_traffic_counters_record() {
        let c = TrafficCounters::new();
        let record = TrafficRecord {
            host_id: "test".into(),
            file_key: "f.txt".into(),
            business: "test".into(),
            operation: S3Operation::GetObject,
            direction: "download".into(),
            bytes: 100,
            count: 1,
        };
        c.record(&record);
        assert_eq!(c.download_bytes.load(Ordering::Relaxed), 100);
        assert_eq!(c.request_count.load(Ordering::Relaxed), 1);
        assert_eq!(
            c.per_operation[S3Operation::GetObject as usize]
                .load(Ordering::Relaxed),
            100
        );
    }

    #[test]
    fn test_s3_operation_from_request() -> crate::error::Result<()> {
        use crate::s3::s3_service::S3Request;
        use crate::types::{BucketName, ObjectKey};
        let b = BucketName::new("my-bucket")?;
        let k = ObjectKey::new("k")?;

        assert!(matches!(
            S3Operation::from_request(&S3Request::GetObject(b.clone(), k.clone())),
            S3Operation::GetObject
        ));
        assert!(matches!(
            S3Operation::from_request(&S3Request::ListObjects(b.clone(), k.clone())),
            S3Operation::ListObjects
        ));
        assert!(matches!(
            S3Operation::from_request(&S3Request::ObjectExists(b.clone(), k.clone())),
            S3Operation::ObjectExists
        ));
        assert!(matches!(
            S3Operation::from_request(&S3Request::DeleteObject(b.clone(), k.clone())),
            S3Operation::DeleteObject
        ));
        Ok(())
    }

    use std::sync::Arc;

    use crate::s3::mock::MockS3Client;

    #[tokio::test]
    async fn test_traffic_recorder_record() {
        let counters = Arc::new(TrafficCounters::new());
        let recorder = TrafficRecorder {
            counters: counters.clone(),
        };

        let record = TrafficRecord {
            host_id: "test".into(),
            file_key: "f.txt".into(),
            business: "test".into(),
            operation: S3Operation::GetObject,
            direction: "download".into(),
            bytes: 100,
            count: 1,
        };
        recorder.record(record);

        // Should be in counters immediately
        assert_eq!(counters.download_bytes.load(Ordering::Relaxed), 100);
    }

    #[tokio::test]
    async fn test_business_s3_client_records_traffic() -> crate::error::Result<()> {
        let counters = Arc::new(TrafficCounters::new());
        let recorder = Arc::new(TrafficRecorder {
            counters: counters.clone(),
        });
        let inner = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);

        let client =
            BusinessS3Client::new(inner.clone(), "h1", "test_biz", recorder);
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        // GetObject should record download traffic
        let data = client.get_object(&bucket, &key).await?;
        assert_eq!(data, b"hello");
        assert!(counters.download_bytes.load(Ordering::Relaxed) > 0);
        assert_eq!(counters.request_count.load(Ordering::Relaxed), 1);

        Ok(())
    }
}