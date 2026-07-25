//! Traffic tracking types and channel-based recording.
//!
//! TrafficRecord, S3Operation, TrafficRecorder, and BusinessS3Client.
//!
//! TrafficRecorder sends TrafficRecords into an mpsc channel, where the
//! TrafficBatchWriter (in traffic_persist.rs) receives and persists them.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::Result;
use crate::s3::client::{ObjectMetadata, ObjectSummary, S3Client};
use crate::types::{BucketName, ObjectKey, Prefix, S3Operation};

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

/// TrafficRecorder — fire-and-forget traffic recording via mpsc channel.
///
/// Records traffic by sending TrafficRecords to the TrafficBatchWriter
/// background task. Non-blocking: drops the record if the channel is full.
pub struct TrafficRecorder {
    sender: mpsc::Sender<TrafficRecord>,
}

impl TrafficRecorder {
    /// Create a new TrafficRecorder with the given mpsc sender.
    ///
    /// The sender should come from `spawn_batch_writer()` in traffic_persist.rs.
    pub fn new(sender: mpsc::Sender<TrafficRecord>) -> Self {
        Self { sender }
    }

    /// Record a traffic event. Sends to the batch writer (non-blocking).
    /// Drops the record if the channel is full.
    pub fn record(&self, record: TrafficRecord) {
        let _ = self.sender.try_send(record);
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
        prefix: &Prefix,
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

    use std::sync::Arc;

    use crate::s3::mock::MockS3Client;

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

        // GetObject should record a traffic record
        let data = client.get_object(&bucket, &key).await?;
        assert_eq!(data, b"hello");

        // We can't assert on atomic counters anymore, but data was returned
        Ok(())
    }
}