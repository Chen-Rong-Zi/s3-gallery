# Tower 迁移清理设计文档

> **Goal:** 修复上一次 Tower 迁移遗留的问题——部分 scan 层过度设计（完整 Service trait 实现可用 `service_fn` 替代），view 层迁移未完成，以及所有业务代码直接使用 `Arc<dyn S3Client>` 的问题。

**Architecture:** 三个部分：(A) 4 个 scan 层改用 `BoxService::new(service_fn(...))` 去掉手写 `Service` trait，(B) 完成 view 层的 `Option<host_id>` 分支消除，(C) 所有业务代码改用 `S3Service`，禁止直接使用 `Arc<dyn S3Client>`。

**Tech Stack:** Rust, `tower` 0.5 (Service, Layer, service_fn, boxed)

---

## 1. 问题分析

### 1.1 过度设计：4 个 scan 层

当前 `DiscoverLayer`、`DiffService<I>`、`ProcessService<I>`、`AggregateService<I>` 都实现了完整的 `Service<ScanRequest>` trait，包含 `Poll::Ready`、`type Future`、`Pin<Box<dyn Future>>` 等样板代码。它们都是"call inner → 处理结果 → 返回"的纯数据流模式，可以用 `tower::service_fn` 替代。

### 1.2 迁移未完成：view 层

`2026-07-23-traffic-tracking-and-tower-refactoring.md` 的 Task 10 要求用 `fetch_all_opt` 消除 `Option<host_id>` 分支，但只完成了部分：

| 文件 | 总分支 | 已简化 | 剩余 |
|------|--------|--------|------|
| `view/stat.rs` | 6 | 1 | 5 处 scalar 查询 |
| `view/duplicates.rs` | 2 | 1 | 1 处（3 参数无法简化） |
| `view/timeline_gallery.rs` | 4 | 0 | 4 处 |

### 1.3 业务代码直接使用 `Arc<dyn S3Client>`

当前有多个模块绕过 `S3Service` 直接调用 `Arc<dyn S3Client>`，导致这些操作没有流量统计、没有日志记录，也不符合 Tower 的组合模式：

| 文件 | 直接 S3 操作 | 说明 |
|------|-------------|------|
| `cmd_init.rs` | `put_object` | 写 host.config.json |
| `cmd_db.rs` | `get_object`, `put_object`, `delete_object`, `check_lock` | DB 拉取/推送/锁检查 |
| `cmd_scan.rs` | `get_object`, `put_object` | DB 拉取/推送（扫描后上传 DB） |
| `web/state.rs` | `s3_clients: HashMap<String, Arc<dyn S3Client>>` | 存储原始客户端 |
| `web/handlers/download.rs` | 通过 `get_s3_client()` 获取原始客户端 | 每次请求构建 S3Service |
| `web/handlers/thumbnail.rs` | 通过 `get_s3_client()` 获取原始客户端 | 每次请求构建 S3Service |

另外存在死代码：
- `s3/lock.rs` — `acquire_lock`、`LockGuard` 未被业务代码使用（仅 `check_lock` 在 `cmd_db.rs` 中使用）
- `view/remote.rs` — `RemoteView` 定义但未被使用

---

## 2. 设计

### 2.1 Part A：Scan 层用 `service_fn` 简化

**核心思路：** 保留 `Layer` struct（持有 `db`、`counters`、`exif_s3` 等配置），去掉手写的 `*Service` struct。`Layer::layer()` 直接返回 `BoxService<ScanRequest, ScanResponse, S3GalleryError>`：

```rust
// 之前：DiffLayer + DiffService<I>，共 ~45 行
// 之后：仅 DiffLayer，~20 行
pub struct DiffLayer { db: SqlitePool }

impl<I> Layer<I> for DiffLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone + Send + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        BoxService::new(service_fn(move |req: ScanRequest| {
            let mut inner = inner.clone();
            let db = db.clone();
            async move {
                let mut resp = inner.call(req).await?;
                // ... diff logic ...
                Ok(resp)
            }
        }))
    }
}
```

**变更文件：**

| 文件 | 当前行数 | 简化后行数 | 节省 |
|------|---------|-----------|------|
| `scan/diff_layer.rs` | ~60 | ~30 | 50% |
| `scan/process.rs` | ~110 | ~80 | 30% |
| `scan/aggregate.rs` | ~100 | ~70 | 30% |
| `scan/discover.rs` | ~120 | ~90 | 25% |

### 2.2 Part B：完成 view 层迁移

#### stat.rs —— 6 处分支，已完成 1 处

5 处剩余分支都是 `query_scalar` 返回 `i64`。需添加 `fetch_scalar_opt` 辅助函数：

```rust
/// Execute a scalar query with an optional host_id binding.
pub async fn fetch_scalar_opt<T>(
    db: &SqlitePool,
    sql: &str,
    host_id: Option<&str>,
) -> Result<T>
where
    T: sqlx::Decode<'_, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    if let Some(hid) = host_id {
        sqlx::query_scalar(sql)
            .bind(hid)
            .fetch_one(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    } else {
        sqlx::query_scalar(sql)
            .fetch_one(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    }
}
```

使用后 5 处 scalar 分支变为：

```rust
let total_files: i64 = fetch_scalar_opt(
    db,
    "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0",
    host_id,
).await?;
```

#### timeline_gallery.rs —— 4 处分支，已完成 0 处

其中 1 处无 tag 筛选的 `FileEntry` 查询可以用 `fetch_all_opt` 替换：

```rust
// 之前：
if let Some(hid) = host_id {
    sqlx::query_as("SELECT * FROM files WHERE host_id = ? AND ...")
        .bind(hid).fetch_all(db).await...
} else {
    sqlx::query_as("SELECT * FROM files WHERE ...").fetch_all(db).await...
};

// 之后：
let files: Vec<FileEntry> = fetch_all_opt(
    db,
    "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
     ORDER BY effective_date DESC, last_modified DESC",
    host_id,
).await?;
```

#### 总结

| 文件 | 总分支 | 可简化 | 已完成 | 本次处理 |
|------|--------|--------|--------|---------|
| `view/stat.rs` | 6 | 6 | 1 | 5（用 `fetch_scalar_opt`） |
| `view/duplicates.rs` | 2 | 1 | 1 | 0（剩余 1 个 3 参数无法简化） |
| `view/timeline_gallery.rs` | 4 | 1 | 0 | 1（用 `fetch_all_opt`） |
| **合计** | **12** | **8** | **2** | **6** |

### 2.3 Part C：所有业务代码改用 `S3Service`

**原则：** `Arc<dyn S3Client>` 只允许存在于 `S3Service` 和 Tower 层的内部实现中。所有业务代码必须通过 `S3Service` 访问 S3。

#### C1. cmd_init.rs

改为使用 `S3Service` 替代 `Arc<dyn S3Client>`：

```rust
// 之前：
let s3 = Arc::new(RealS3Client::from_config(&config)) as Arc<dyn S3Client>;
s3.put_object(&bucket, config_key, &json_bytes).await?;

// 之后：
let mut s3 = S3Service::new(Arc::new(RealS3Client::from_config(&config)));
s3.put_object(&bucket, &config_key, &json_bytes).await?;
```

#### C2. cmd_db.rs

改为使用 `S3Service`。`create_s3_client()` 返回 `S3Service` 而非 `Arc<dyn S3Client>`。`check_lock` 调用同样改为接受 `S3Service`。

#### C3. cmd_scan.rs

DB 拉取/推送操作（`get_object` 读取远程 DB、`put_object` 上传本地 DB）改为使用 `S3Service` 替代 `Arc<dyn S3Client>`。

#### C4. web/state.rs + web/handlers

**问题：** 当前 `AppState` 同时存储 `s3_clients: HashMap<String, Arc<dyn S3Client>>` 和 `s3_stack: S3Service`。handlers 通过 `get_s3_client()` 获取原始客户端，每次请求构建新的 S3Service。

**修改：**
1. 从 `AppState` 移除 `s3_clients` 字段
2. 保留 `s3_stack: S3Service`（不带业务 TrafficLayer 的基础服务）
3. 添加 `s3_with_traffic()` 方法：

```rust
impl AppState {
    /// Get an S3Service with per-business traffic recording.
    pub fn s3_with_traffic(&self, host_id: &str, business: &str) -> S3Service {
        if let Some(ref recorder) = self.traffic_recorder {
            ServiceBuilder::new()
                .layer(TrafficLayer::new(recorder.clone(), host_id, business))
                .service(self.s3_stack.clone())
        } else {
            self.s3_stack.clone()
        }
    }
}
```

4. `cmd_serve.rs` 中构建 `s3_stack` 时，只加 `LogLayer`，不加 `TrafficLayer`（每个 handler 自己加业务标签）：

```rust
// 之前：
let s3_stack = LogLayer.layer(
    TrafficLayer::new(recorder.clone(), "serve", "s3_api").layer(core_s3),
);

// 之后：
let s3_stack = LogLayer.layer(core_s3);
```

5. handlers 改为：

```rust
// 之前：
let raw_client = state.get_s3_client(host).unwrap().clone();
let mut s3 = if let Some(ref recorder) = state.traffic_recorder {
    let core = S3Service::new(raw_client);
    ServiceBuilder::new()
        .layer(TrafficLayer::new(recorder.clone(), host_id, "web_download"))
        .service(core)
} else {
    S3Service::new(raw_client)
};

// 之后：
let mut s3 = state.s3_with_traffic(host_id, "web_download");
```

#### C5. 死代码清理

- `view/remote.rs` — `RemoteView` 未被使用，可以删除
- `s3/lock.rs` — `acquire_lock`、`LockGuard` 未被使用，保留 `check_lock`（在 cmd_db.rs 中使用）

---

## 3. 文件变更清单

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `scan/diff_layer.rs` | 修改 | 去掉 `DiffService`，`Layer::layer()` 返回 `BoxService` |
| `scan/process.rs` | 修改 | 去掉 `ProcessService`，`Layer::layer()` 返回 `BoxService` |
| `scan/aggregate.rs` | 修改 | 去掉 `AggregateService`，`Layer::layer()` 返回 `BoxService` |
| `scan/discover.rs` | 修改 | 去掉 `DiscoverService`，`Layer::layer()` 返回 `BoxService` |
| `util/db_helpers.rs` | 修改 | 添加 `fetch_scalar_opt` |
| `view/stat.rs` | 修改 | 5 处 scalar 查询改用 `fetch_scalar_opt` |
| `view/timeline_gallery.rs` | 修改 | 1 处 `FileEntry` 查询改用 `fetch_all_opt` |
| `cmd_init.rs` | 修改 | 改用 `S3Service` 替代 `Arc<dyn S3Client>` |
| `cmd_db.rs` | 修改 | 改用 `S3Service`，`create_s3_client` 返回 `S3Service` |
| `cmd_scan.rs` | 修改 | DB 拉取/推送改用 `S3Service` |
| `web/state.rs` | 修改 | 移除 `s3_clients`，添加 `s3_with_traffic()` |
| `web/handlers/download.rs` | 修改 | 使用 `state.s3_with_traffic()` |
| `web/handlers/thumbnail.rs` | 修改 | 使用 `state.s3_with_traffic()` |
| `cmd_serve.rs` | 修改 | `s3_stack` 只加 `LogLayer`，不加 `TrafficLayer` |
| `view/remote.rs` | 删除 | 死代码，未被使用 |
| `view/mod.rs` | 修改 | 移除 `pub use remote::RemoteView` |

---

## 4. 不在此次范围内的变更

- `s3/lock.rs` 的 `acquire_lock`/`LockGuard` 保持存在（虽然是死代码，删除可能影响后续需求）
- `LogLayer`、`TrafficLayer` 为 `Layer<S3Service>` 非通用 —— 暂不改为泛型

---

## 5. 测试

- 现有 278 个 core 测试全部通过
- `service_fn` 简化后，`cmd_scan.rs` 和 `scanner.rs` 的管道构建代码不变
- view 层迁移后，查询行为不变
- `S3Service` 迁移后，`cmd_init`、`cmd_db`、`cmd_scan` 的 DB 操作行为不变
- web handlers 改用 `s3_with_traffic()` 后，S3 访问行为不变，仅流量标签可能变化