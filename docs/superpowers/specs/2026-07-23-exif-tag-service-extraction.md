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

ExifService 由两个层组合而成，从内到外：

**ChunkedDownloadLayer（内层）：** 接受 `ExifRequest`，调用 `S3Service.get_object_range` 下载 64KB，返回 `Vec<u8>`。

```rust
struct ChunkedDownloadLayer {
    s3: S3Service,
}

impl Layer<I> for ChunkedDownloadLayer {
    type Service = BoxService<ExifRequest, Vec<u8>, S3GalleryError>;
    // 从 ExifRequest 中提取 (bucket, key)，调用 s3.get_object_range
}
```

**ExtractionLayer（外层）：** 接受 `Vec<u8>`，运行 `ExtractorRegistry.extract_all`，存储 `MetadataEntry`，返回 `ExifResult`。

```rust
struct ExtractionLayer {
    db: SqlitePool,
}

impl Layer<I> for ExtractionLayer {
    type Service = BoxService<Vec<u8>, ExifResult, S3GalleryError>;
    // 解析 bytes → MetadataItem[]，存储到 DB，返回 ExifData
}
```

**组合：**

```rust
let exif_service = ServiceBuilder::new()
    .layer(ExtractionLayer::new(db))
    .layer(ChunkedDownloadLayer::new(exif_s3))
    .service(/* 起始点——但 ExifService 不是从 scan pipeline 来的 */);
```

等一下——这里有个问题。`ChunkedDownloadLayer` 接受 `ExifRequest`，`ExtractionLayer` 接受 `Vec<u8>`。但 `ServiceBuilder` 组合时，内层 Service 的 Request 类型由外层决定。`ExtractionLayer` 是 `Layer<I>`，它的 `I` 需要是 `Service<Vec<u8>>`，但 `ChunkedDownloadLayer` 的 output 是 `Vec<u8>`。

所以 `ExtractionLayer` 应该包装在 `ChunkedDownloadLayer` 外面：

```rust
// ExtractionLayer 接受 Vec<u8>，返回 ExifResult
// ChunkedDownloadLayer 接受 ExifRequest，返回 Vec<u8>
// 组合后：ExifRequest → ExifResult

let exif_service = ServiceBuilder::new()
    .layer(ExtractionLayer::new(db))      // 外层：Vec<u8> → ExifResult
    .layer(ChunkedDownloadLayer::new(s3)) // 内层：ExifRequest → Vec<u8>
    .service(/* 不需要，因为 ChunkedDownloadLayer 自己就是终点 */);
```

不对，`ServiceBuilder` 需要 `.service(inner)` 作为最内层。但 `ChunkedDownloadLayer` 是最内层，它不需要 inner——它直接做 S3 调用。

所以 `ChunkedDownloadLayer` 不是 `Layer`，而是一个 `Service<ExifRequest>`：

```rust
// ChunkedDownloadService 是基础 Service（不是 Layer）
struct ChunkedDownloadService {
    s3: S3Service,
}

impl Service<ExifRequest> for ChunkedDownloadService {
    type Response = Vec<u8>;
    // 下载 64KB
}

// ExtractionLayer 是 Layer，包装下载结果
struct ExtractionLayer {
    db: SqlitePool,
}

impl<I> Layer<I> for ExtractionLayer
where I: Service<ExifRequest, Response = Vec<u8>>
{
    type Service = BoxService<ExifRequest, ExifResult, S3GalleryError>;
    // 解析 bytes → 存储 metadata → 返回 ExifResult
}

// 组合：
let exif_service = ServiceBuilder::new()
    .layer(ExtractionLayer::new(db))
    .service(ChunkedDownloadService::new(s3));
// 类型：BoxService<ExifRequest, ExifResult, S3GalleryError>
```

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

### 2.5 ProcessLayer 简化后

ProcessLayer 不再需要 `exif_s3` 字段，改为持有 `exif_service` 和 `tag_service`：

```rust
struct ProcessLayer {
    db: SqlitePool,
    exif_service: BoxService<ExifRequest, ExifResult, S3GalleryError>,
    tag_service: TagService,
}
```

for 循环简化为：

```rust
for entry in &pending {
    let exif_result = exif_service.call(ExifRequest {
        bucket: bucket.clone(),
        key: ObjectKey::new(entry.key.clone())?,
        host_id: host.host_id.clone(),
        file_type: ...,  // 从 entry 或扩展名计算
        ext: ...,
    }).await?;

    if let ExifResult::Some(exif_data) = exif_result {
        tag_service.call(TagRequest {
            host_id: host.host_id.clone(),
            key: entry.key.clone(),
            exif_data,
            file_type: ...,
        }).await?;
        processed += 1;
    } else {
        // 标记为 extracted，无 EXIF
        // ...
    }
}
```

---

## 3. 文件变更清单

| 文件 | 变更 | 说明 |
|------|------|------|
| `scan/process.rs` | 修改 | 移除内联的 EXIF 下载/提取/标签逻辑，改用 ExifService + TagService |
| `scan/exif_service.rs` | 新建 | `ChunkedDownloadService` + `ExtractionLayer` + `ExifService` 组合 |
| `scan/tag_service.rs` | 新建 | `TagService` 实现 |
| `scan/mod.rs` | 修改 | 添加 `pub mod exif_service; pub mod tag_service;` |
| `scan/pipeline.rs` | 修改 | 添加 `ExifRequest`、`ExifResult`、`ExifData`、`TagRequest`、`TagResponse` 类型 |
| `cmd_scan.rs` / `scanner.rs` | 修改 | 构建时创建 ExifService + TagService 并传入 ProcessLayer |

---

## 4. 测试

- `ChunkedDownloadService` 可用 MockS3Client 测试
- `ExtractionLayer` 可用已知 bytes 测试（纯解析逻辑）
- `TagService` 可用已知 MetadataItem[] 测试
- `ProcessLayer` 集成测试保持不变
- 现有 276 个测试全部通过