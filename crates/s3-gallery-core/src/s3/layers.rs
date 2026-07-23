//! Tower Layer implementations for S3Service.
//!
//! LogLayer wraps S3Service with logging via LoggedS3Client.
//! TrafficLayer will be added in a later task.

use std::sync::Arc;

use tower::layer::Layer;

use crate::s3::client::S3Client;
use crate::s3::logged::LoggedS3Client;
use crate::s3::s3_service::S3Service;

/// LogLayer wraps S3Service with logging via LoggedS3Client.
///
/// Must be placed inside the Tower stack because it needs access to the
/// inner Arc<dyn S3Client> via into_inner().
pub struct LogLayer;

impl Layer<S3Service> for LogLayer {
    type Service = S3Service;

    fn layer(&self, inner: S3Service) -> Self::Service {
        S3Service::new(Arc::new(LoggedS3Client::new(inner.into_inner())) as Arc<dyn S3Client>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use crate::s3::s3_service::{S3Request, S3Response, S3Service};
    use std::sync::Arc;
    use tower::layer::Layer;
    use tower::Service;

    #[tokio::test]
    async fn test_log_layer_composes() -> crate::error::Result<()> {
        let mock = Arc::new(MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?);
        let core = S3Service::new(mock);
        let mut svc = LogLayer.layer(core);
        let req = S3Request::GetObject(
            crate::types::BucketName::new("test-bucket")?,
            crate::types::ObjectKey::new("test.txt")?,
        );
        let resp = svc.call(req).await?;
        match resp {
            S3Response::GetObject(data) => assert_eq!(data, b"hello"),
            _ => panic!("expected GetObject"),
        }
        Ok(())
    }
}