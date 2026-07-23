//! BatchService — 泛型批量并发 Service。
//!
//! 包装任意 `Service<Req, Response = Res>`，接受 `IntoIterator<Item = Req>`，
//! 内部用 FuturesOrdered + Semaphore 并发执行，返回 `Vec<Result<Res, Error>>`。
//! FuturesOrdered 保证结果按插入顺序返回，不受完成顺序影响。

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::stream::{FuturesOrdered, StreamExt};
use tokio::sync::Semaphore;
use tower::Service;

/// 包装 inner Service，支持批量并发请求。
pub struct BatchService<I, Req, Res> {
    inner: I,
    max_concurrency: usize,
    _phantom: PhantomData<(Req, Res)>,
}

impl<I, Req, Res> BatchService<I, Req, Res> {
    pub fn new(inner: I, max_concurrency: usize) -> Self {
        Self {
            inner,
            max_concurrency,
            _phantom: PhantomData,
        }
    }
}

impl<I: Clone, Req, Res> Clone for BatchService<I, Req, Res> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            max_concurrency: self.max_concurrency,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<I, Req, Res, Iter> Service<Iter> for BatchService<I, Req, Res>
where
    I: Service<Req, Response = Res> + Clone + Send + 'static,
    I::Error: Send,
    I::Future: Send,
    Req: Send + 'static,
    Res: Send + 'static,
    Iter: IntoIterator<Item = Req> + Send + 'static,
    Iter::IntoIter: Send,
{
    type Response = Vec<std::result::Result<Res, I::Error>>;
    type Error = crate::error::S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, reqs: Iter) -> Self::Future {
        let inner = self.inner.clone();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));

        Box::pin(async move {
            let mut tasks = FuturesOrdered::new();
            for req in reqs.into_iter() {
                let mut inner = inner.clone();
                let permit = semaphore.clone().acquire_owned();
                tasks.push_back(async move {
                    // SAFETY: semaphore is created locally and never closed
                    #[allow(clippy::expect_used)]
                    let _permit = permit.await.expect("semaphore closed");
                    inner.call(req).await
                });
            }

            let results: Vec<_> = tasks.collect().await;
            Ok(results)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::service_fn;

    #[tokio::test]
    async fn test_batch_service_empty() {
        let svc = service_fn(|req: i32| async move { Ok::<_, String>(req * 2) });
        let mut batch = BatchService::new(svc, 10);
        let results: Vec<std::result::Result<i32, String>> = batch.call(vec![]).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_batch_service_concurrent() {
        let svc = service_fn(|req: i32| async move { Ok::<_, String>(req * 2) });
        let mut batch = BatchService::new(svc, 10);
        let results: Vec<std::result::Result<i32, String>> = batch.call(vec![1, 2, 3]).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), &2);
        assert_eq!(results[1].as_ref().unwrap(), &4);
        assert_eq!(results[2].as_ref().unwrap(), &6);
    }

    #[tokio::test]
    async fn test_batch_service_semaphore_limits() {
        let svc = service_fn(|req: i32| async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            Ok::<_, String>(req * 2)
        });
        let mut batch = BatchService::new(svc, 2);
        let results: Vec<std::result::Result<i32, String>> = batch.call(1..=5).await.unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_batch_service_preserves_order() {
        // Tasks that complete out of order (smaller input = slower)
        let svc = service_fn(|req: u64| async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(req)).await;
            Ok::<_, String>(req)
        });
        let mut batch = BatchService::new(svc, 10);
        // Input: [100, 5, 1]  — completion order: [1, 5, 100]
        let results: Vec<std::result::Result<u64, String>> =
            batch.call(vec![100, 5, 1]).await.unwrap();
        // Must be in insertion order: [100, 5, 1]
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), &100);
        assert_eq!(results[1].as_ref().unwrap(), &5);
        assert_eq!(results[2].as_ref().unwrap(), &1);
    }
}
