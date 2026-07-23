use crate::error::{Result, S3GalleryError};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// 并发控制，所有 S3 请求通过此结构体执行
#[derive(Clone)]
pub struct ConcurrencyLimiter {
    semaphore: Arc<Semaphore>,
}

impl ConcurrencyLimiter {
    /// 创建并发限制器
    /// max_concurrency: 最大并发 S3 请求数
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
        }
    }

    /// 执行 S3 请求，受并发限制
    /// 自动获取 Semaphore 许可，执行完毕释放
    ///
    /// # Errors
    ///
    /// Returns an error if acquiring the semaphore permit fails, or if the wrapped future returns an error.
    pub async fn execute<F, T>(&self, f: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let _permit = self.semaphore.acquire().await.map_err(|e| {
            S3GalleryError::Internal(format!("Failed to acquire semaphore permit: {}", e))
        })?;

        f.await
    }

    /// 返回当前可用的许可数
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_execute_returns_result() -> Result<()> {
        let limiter = ConcurrencyLimiter::new(10);
        let result = limiter
            .execute(async { Ok::<_, S3GalleryError>(42) })
            .await?;
        assert_eq!(result, 42);
        Ok(())
    }

    #[tokio::test]
    async fn test_concurrency_limit_respected() -> Result<()> {
        let limiter = ConcurrencyLimiter::new(2);
        assert_eq!(limiter.available_permits(), 2);

        // Execute a task that holds the permit
        let limiter_clone = limiter.clone();
        let handle = tokio::spawn(async move {
            limiter_clone
                .execute(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok::<_, S3GalleryError>(())
                })
                .await
        });

        // Give the task time to acquire
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(limiter.available_permits(), 1);

        handle
            .await
            .map_err(|e| S3GalleryError::Internal(e.to_string()))??;
        assert_eq!(limiter.available_permits(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_shares_semaphore() -> Result<()> {
        let limiter = ConcurrencyLimiter::new(1);
        assert_eq!(limiter.available_permits(), 1);

        let limiter_clone = limiter.clone();
        assert_eq!(limiter_clone.available_permits(), 1);

        // Execute a task that holds the permit
        let handle = tokio::spawn(async move {
            limiter_clone
                .execute(async {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok::<_, S3GalleryError>(())
                })
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(limiter.available_permits(), 0);

        handle
            .await
            .map_err(|e| S3GalleryError::Internal(e.to_string()))??;
        assert_eq!(limiter.available_permits(), 1);
        Ok(())
    }
}
