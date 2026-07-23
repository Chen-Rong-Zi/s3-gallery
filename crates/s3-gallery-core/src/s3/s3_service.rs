//! S3Service — tower::Service wrapper around Arc<dyn S3Client>.
//!
//! Provides a unified S3 request/response type system and implements
//! `tower::Service` so that Tower's built-in layers (BufferLayer,
//! TimeoutLayer, ConcurrencyLimitLayer) can be composed with it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tower::Service;

use crate::error::{Result, S3GalleryError};
use crate::s3::client::{ObjectMetadata, ObjectSummary, S3Client};
use crate::types::{BucketName, ObjectKey};

/// Unified S3 request type — each variant corresponds to one S3Client method.
#[derive(Debug, Clone)]
pub enum S3Request {
    GetObject(BucketName, ObjectKey),
    GetObjectRange(BucketName, ObjectKey, u64, u64),
    ListObjects(BucketName, ObjectKey),
    HeadObject(BucketName, ObjectKey),
    PutObject(BucketName, ObjectKey, Vec<u8>),
    PutObjectIfNoneMatch(BucketName, ObjectKey, Vec<u8>),
    DeleteObject(BucketName, ObjectKey),
    ObjectExists(BucketName, ObjectKey),
}

/// Unified S3 response type — each variant corresponds to one S3Request variant.
#[derive(Debug)]
pub enum S3Response {
    GetObject(Vec<u8>),
    GetObjectRange(Vec<u8>),
    ListObjects(Vec<ObjectSummary>),
    HeadObject(ObjectMetadata),
    PutObject(()),
    PutObjectIfNoneMatch(bool),
    DeleteObject(()),
    ObjectExists(bool),
}

impl S3Response {
    pub fn try_into_get_object(self) -> Result<Vec<u8>> {
        match self {
            Self::GetObject(data) => Ok(data),
            _ => Err(S3GalleryError::Internal("expected GetObject response".into())),
        }
    }

    pub fn try_into_get_object_range(self) -> Result<Vec<u8>> {
        match self {
            Self::GetObjectRange(data) => Ok(data),
            _ => Err(S3GalleryError::Internal("expected GetObjectRange response".into())),
        }
    }

    pub fn try_into_list_objects(self) -> Result<Vec<ObjectSummary>> {
        match self {
            Self::ListObjects(objs) => Ok(objs),
            _ => Err(S3GalleryError::Internal("expected ListObjects response".into())),
        }
    }

    pub fn try_into_head_object(self) -> Result<ObjectMetadata> {
        match self {
            Self::HeadObject(meta) => Ok(meta),
            _ => Err(S3GalleryError::Internal("expected HeadObject response".into())),
        }
    }

    pub fn try_into_put_object(self) -> Result<()> {
        match self {
            Self::PutObject(()) => Ok(()),
            _ => Err(S3GalleryError::Internal("expected PutObject response".into())),
        }
    }

    pub fn try_into_put_object_if_none_match(self) -> Result<bool> {
        match self {
            Self::PutObjectIfNoneMatch(b) => Ok(b),
            _ => Err(S3GalleryError::Internal("expected PutObjectIfNoneMatch response".into())),
        }
    }

    pub fn try_into_delete_object(self) -> Result<()> {
        match self {
            Self::DeleteObject(()) => Ok(()),
            _ => Err(S3GalleryError::Internal("expected DeleteObject response".into())),
        }
    }

    pub fn try_into_object_exists(self) -> Result<bool> {
        match self {
            Self::ObjectExists(b) => Ok(b),
            _ => Err(S3GalleryError::Internal("expected ObjectExists response".into())),
        }
    }
}

/// S3Service wraps Arc<dyn S3Client> and implements tower::Service<S3Request>.
///
/// This allows Tower's built-in layers (BufferLayer, TimeoutLayer,
/// ConcurrencyLimitLayer) to be composed with it.
#[derive(Clone)]
pub struct S3Service {
    inner: Arc<dyn S3Client>,
}

impl S3Service {
    pub fn new(inner: Arc<dyn S3Client>) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> Arc<dyn S3Client> {
        self.inner
    }
}

impl Service<S3Request> for S3Service {
    type Response = S3Response;
    type Error = S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: S3Request) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let result = match req {
                S3Request::GetObject(b, k) => {
                    inner.get_object(&b, &k).await.map(S3Response::GetObject)
                }
                S3Request::GetObjectRange(b, k, s, e) => {
                    inner.get_object_range(&b, &k, s, e).await.map(S3Response::GetObjectRange)
                }
                S3Request::ListObjects(b, p) => {
                    inner.list_objects(&b, &p).await.map(S3Response::ListObjects)
                }
                S3Request::HeadObject(b, k) => {
                    inner.head_object(&b, &k).await.map(S3Response::HeadObject)
                }
                S3Request::PutObject(b, k, body) => {
                    inner.put_object(&b, &k, &body).await.map(S3Response::PutObject)
                }
                S3Request::PutObjectIfNoneMatch(b, k, body) => {
                    inner.put_object_if_none_match(&b, &k, &body).await.map(S3Response::PutObjectIfNoneMatch)
                }
                S3Request::DeleteObject(b, k) => {
                    inner.delete_object(&b, &k).await.map(S3Response::DeleteObject)
                }
                S3Request::ObjectExists(b, k) => {
                    inner.object_exists(&b, &k).await.map(S3Response::ObjectExists)
                }
            };
            result.map_err(|e| e)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_s3_service_get_object() -> crate::error::Result<()> {
        let mock = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);
        let mut svc = S3Service::new(mock);
        let req = S3Request::GetObject(
            crate::types::BucketName::new("test-bucket")?,
            crate::types::ObjectKey::new("test.txt")?,
        );
        let resp = svc.call(req).await?;
        match resp {
            S3Response::GetObject(data) => assert_eq!(data, b"hello"),
            _ => panic!("expected GetObject response"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_s3_service_list_objects() -> crate::error::Result<()> {
        let mock = Arc::new(MockS3Client::with_fixtures(vec![
            ("a.jpg", b"img1"),
            ("b.jpg", b"img2"),
        ])?);
        let mut svc = S3Service::new(mock);
        let req = S3Request::ListObjects(
            crate::types::BucketName::new("test-bucket")?,
            crate::types::ObjectKey::new("")?,
        );
        let resp = svc.call(req).await?;
        match resp {
            S3Response::ListObjects(objs) => assert_eq!(objs.len(), 2),
            _ => panic!("expected ListObjects response"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_s3_service_clone() {
        let mock = Arc::new(MockS3Client::new());
        let svc = S3Service::new(mock);
        let svc2 = svc.clone();
        drop(svc);
        drop(svc2);
    }

    #[tokio::test]
    async fn test_s3_service_into_inner() {
        let mock = Arc::new(MockS3Client::new());
        let svc = S3Service::new(mock.clone());
        let inner = svc.into_inner();
        // Verify that the returned Arc points to the same allocation by
        // checking that the strong count is consistent (2 references: mock + inner).
        assert_eq!(Arc::strong_count(&mock), 2);
        assert_eq!(Arc::strong_count(&inner), 2);
        drop(mock);
        assert_eq!(Arc::strong_count(&inner), 1);
    }
}