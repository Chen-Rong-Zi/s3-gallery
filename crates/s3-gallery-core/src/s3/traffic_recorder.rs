//! Traffic tracking types and real-time counters.
//!
//! TrafficRecord, TrafficCounters, S3Operation, and BusinessS3Client.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::s3::s3_service::S3Request;

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
}