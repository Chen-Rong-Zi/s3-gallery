# ExifService + TagService 提取设计文档

> **Goal:** 将 ProcessLayer 中的 EXIF 下载、提取和标签解析拆分为两个独立的 tower::Service，由 ProcessLayer 内部调用，使每层职责单一、可独立测试。

**Architecture:** ExifService 内部组合 ChunkedDownloadLayer 和 ExtractionLayer，输出 `Result<Option<ExifData>>`；TagService 接收 ExifData 解析标签并存储。ProcessLayer 仍然是编排者，遍历 pending 文件依次调用两个 Service。

**Tech Stack:** Rust, `tower` 0.5 (Service, Layer, service_fn, boxed), `sqlx`/SQLite

---

## 1. 问题分析

当前 ProcessLayer 在一个 for 循环中做了三件事：

1. 下载 64KB EXIF（`S3Service.get_object_range`）
2. 提取元数据（`ExtractorRegistry.extract_all` → `MetadataEntry::insert`）
3. 标签解析（`evaluate_all` → `FileTagEntry::insert` → 更新 `effective_date`）

这违反了单一职责原则——下载、解析、标签是三个独立的关注点，混在一起难以独立测试，也难以复用。

## 2. 设计

### 2.1 整体架构

```
ProcessLayer
  for each pending file:
    ┌─ ExifService ───────────────────────────────┐
    │  ExtractionLayer  (纯解析，无 IO)             │
    │  ┌─ ChunkedDownloadLayer ──────────────────┐│
    │  │  下载 64KB (S3 get_object_range)         ││
    │  └─────────────────────────────────────────┘│
    └──→ Result<Option<ExifData>>──────────────────┘
              │
              ↓ 如果有 EXIF 数据
    ┌─ TagService ─────────────────────────────────┐
    │  解析标签 → 存储 FileTagEntry                 │
    │  更新 effective_date                          │
    └──────────────────────────────────────────────┘
```

### 2.2 类型定义

```rust
/// EXIF 提取请求
struct ExifRequest {
    pub bucket: BucketName,
    pub key: ObjectKey,
    pub host_id: String,
    pub file_type: String,
    pub ext: String,
}

/// EXIF 提取结果
enum ExifResult {
    Some(ExifData),
    None,  // 该文件无 EXIF 数据
}

/// 成功提取的 EXIF 数据
struct ExifData {
    pub items: Vec<MetadataItem>,
    pub effective_date: Option<String>,
}

/// 标签解析请求
struct TagRequest {
    pub host_id: String,
    pub key: String,
    pub exif_data: ExifData,
    pub file_type: String,
}

/// 标签解析结果
struct TagResponse {
    pub tags: Vec<String>,
}
```

### 2.3 ExifService 内部组合

ExifService 由 ChunkedDownloadService（基础 Service）和 ExtractionLayer（包装层）组合而成：

**ChunkedDownloadService（基础 Service，不是 Layer）：** 接受 `ExifRequest`，调用 `S3Service.get_object_range` 下载 64KB，返回 `Vec<u8>`。

```rust
struct ChunkedDownloadService {
    s3: S3Service,
}

impl Service<ExifRequest> for ChunkedDownloadService {
    type Response = Vec<u8>;
    type Error = S3GalleryError;
    // 从 ExifRequest 中提取 (bucket, key)，调用 s3.get_object_range
}
```

**ExtractionLayer（Layer，包装 ChunkedDownloadService）：** 接受 `Vec<u8>`，运行 `ExtractorRegistry.extract_all`，存储 `MetadataEntry`，返回 `ExifResult`。

```rust
struct ExtractionLayer {
    db: SqlitePool,
}

impl<I> Layer<I> for ExtractionLayer
where I: Service<ExifRequest, Response = Vec<u8>, Error = S3GalleryError>
{
    type Service = BoxService<ExifRequest, ExifResult, S3GalleryError>;
    // 解析 bytes → MetadataItem[]，存储到 DB，返回 ExifData
}
```

**组合：**

```rust
let exif_service: BoxService<ExifRequest, ExifResult, S3GalleryError> = ServiceBuilder::new()
    .layer(ExtractionLayer::new(db))   // 外层：Vec<u8> → ExifResult
    .service(ChunkedDownloadService::new(s3));  // 内层：ExifRequest → Vec<u8>
```

`ChunkedDownloadService` 是最内层，直接实现 `Service<ExifRequest>`（不是 Layer）。`ExtractionLayer` 包装它，类型链为 `ExifRequest → ExifResult`。

### 2.4 TagService

TagService 更简单，它不需要组合——逻辑就是"解析 EXIF → 生成标签 → 存储"：

```rust
struct TagService {
    db: SqlitePool,
}

impl Service<TagRequest> for TagService {
    type Response = TagResponse;
    // 1. evaluate_all(tag_rules, exif_data.items, file_type)
    // 2. TagEntry::ensure_exists + FileTagEntry::insert
    // 3. 更新 effective_date
    // 4. 返回 TagResponse
}
```

### 2.5 BatchService — 通用批量并发 Service

ProcessLayer 当前用 for 循环逐个 await。为消除循环并实现并发，引入 `BatchService<I, Req, Res>`——一个泛型 Service，把 `Vec<Req>` 或任何 `IntoIterator<Item = Req>` 转换为 `Vec<Result<Res, Error>>`，内部用 `Semaphore` 控制并发度。

```rust
/// 通用批量 Service：包装任意 Service<Req, Res>，
/// 接受 IntoIterator<Item = Req>，并发执行，返回 Vec<Result<Res, Error>>。
pub struct BatchService<I, Req, Res> {
    inner: I,
    max_concurrency: usize,
}

impl<I, Req, Res, Iter> Service<Iter> for BatchService<I, Req, Res>
where
    I: Service<Req, Response = Res> + Clone + Send + 'static,
    I::Future: Send,
    Req: Send + 'static,
    Res: Send + 'static,
    Iter: IntoIterator<Item = Req>,
    Iter::IntoIter: Send,
{
    type Response = Vec<Result<Res, I::Error>>;
    type Error = I::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, reqs: Iter) -> Self::Future {
        let inner = self.inner.clone();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));

        Box::pin(async move {
            use futures::stream::FuturesUnordered;
            use futures::StreamExt;

            let mut tasks = FuturesUnordered::new();
            for req in reqs.into_iter() {
                let mut inner = inner.clone();
                let permit = semaphore.clone().acquire_owned();
                tasks.push(async move {
                    let _permit = permit.await.unwrap();
                    inner.call(req).await
                });
            }

            tasks.collect::<Vec<_>>().await
        })
    }
}
```

**核心设计：**
- `IntoIterator` 泛型：接受 `Vec<Req>`、`Vec<Req>.into_iter()`、`iter.map(...)` 等任何惰性迭代器
- `FuturesUnordered`：流式消费，任务完成一个即可返回，不等全部收集
- `Semaphore`：限制最大并发数，防止打满 S3

### 2.6 ProcessLayer 简化后

ProcessLayer 不再持有 `exif_s3`，改为持有 `BatchService<ExifService, ExifRequest, ExifResult>` 和 `BatchService<TagService, TagRequest, TagResponse>`：

```rust
struct ProcessLayer {
    db: SqlitePool,
    batch_exif: BatchService<BoxService<ExifRequest, ExifResult, S3GalleryError>, ExifRequest, ExifResult>,
    batch_tag: BatchService<TagService, TagRequest, TagResponse>,
}
```

for 循环完全消除，变为两阶段处理：

```rust
// 1. 构造所有 ExifRequest（惰性迭代，不分配）
let exif_reqs = pending.iter().filter_map(|entry| {
    let ext = parse_extension(entry.key.rsplit('/').next()?)?;
    let file_type = classify_extension(&ext);
    // 检查是否有 extractor 支持
    if registry.find(&file_type, ext.as_str()).is_empty() {
        return None;
    }
    Some(ExifRequest {
        bucket: bucket.clone(),
        key: ObjectKey::new(entry.key.clone()).ok()?,
        host_id: host.host_id.clone(),
        file_type: file_type.to_string(),
        ext: ext.to_string(),
    })
});

// 2. 批量并发下载 + 提取 EXIF
let exif_results = batch_exif.call(exif_reqs).await?;

// 3. 构造 TagRequest（只取成功的 EXIF 结果）
let tag_reqs = exif_results.into_iter().filter_map(|r| {
    r.ok()?.into_some()?.let(|exif_data| TagRequest {
        host_id: host.host_id.clone(),
        key: exif_data.key.clone(),
        exif_data,
        file_type: ...,
    })
});

// 4. 批量并发标签解析
let tag_results = batch_tag.call(tag_reqs).await?;
```

---

## 3. 文件变更清单

| 文件 | 变更 | 说明 |
|------|------|------|
| `scan/process.rs` | 修改 | 移除内联的 EXIF 下载/提取/标签逻辑，改用 BatchService<ExifService> + BatchService<TagService> |
| `scan/exif_service.rs` | 新建 | `ChunkedDownloadService` + `ExtractionLayer` 组合 |
| `scan/tag_service.rs` | 新建 | `TagService` 实现 |
| `scan/batch_service.rs` | 新建 | `BatchService<I, Req, Res>` 泛型批量并发 Service |
| `scan/mod.rs` | 修改 | 添加 `pub mod exif_service; pub mod tag_service; pub mod batch_service;` |
| `scan/pipeline.rs` | 修改 | 添加 `ExifRequest`、`ExifResult`、`ExifData`、`TagRequest`、`TagResponse` 类型 |
| `cmd_scan.rs` / `scanner.rs` | 修改 | 构建时创建 ExifService + TagService + BatchService 并传入 ProcessLayer |

---

## 4. 测试

- `ChunkedDownloadService` 可用 MockS3Client 测试
- `ExtractionLayer` 可用已知 bytes 测试（纯解析逻辑）
- `TagService` 可用已知 MetadataItem[] 测试
- `ProcessLayer` 集成测试保持不变
- 现有 276 个测试全部通过