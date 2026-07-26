# 代码标准合规整改报告

> 基于 2026-07-26 的全面代码审核结果，汇总所有不符合 CLAUDE.md 标准的违规项。
> 每个问题附带原始 CLAUDE.md 标准引用，用于后续 brainstorming 重构方案。

---

## 一、clippy deny 违规（10 处）

### 1.1 unwrap() 在非测试代码中（2 处）

| # | 文件 | 行 | 代码 | 标准引用 |
|---|------|:--:|------|---------|
| U1 | `crates/s3-gallery-core/src/view/traffic.rs` | 69 | `host_id.unwrap()` — 用 `if host_id.is_some()` 保护后 unwrap | `#![deny(clippy::unwrap_used)]` |
| U2 | `crates/s3-gallery-core/src/view/traffic.rs` | 129 | 同上模式 | `#![deny(clippy::unwrap_used)]` |

### 1.2 expect() 在非测试代码中（1 处）

| # | 文件 | 行 | 代码 | 标准引用 |
|---|------|:--:|------|---------|
| E1 | `crates/s3-gallery-core/src/scan/batch_service.rs` | 74 | `permit.await.expect("semaphore closed")` | `#![deny(clippy::expect_used)]` |

### 1.3 数组索引访问（6 处）

| # | 文件 | 行 | 代码 | 标准引用 |
|---|------|:--:|------|---------|
| I1 | `crates/s3-gallery-cli/src/web/handlers/file_detail.rs` | 34 | `UNITS[unit_idx]` | `#![deny(clippy::indexing_slicing)]` |
| I2 | `crates/s3-gallery-cli/src/web/handlers/file_detail.rs` | 36 | `UNITS[unit_idx]` | 同上 |
| I3 | `crates/s3-gallery-cli/src/web/handlers/file_detail.rs` | 62 | `key[..slash]` | 同上 |
| I4 | `crates/s3-gallery-cli/src/web/handlers/download.rs` | 19 | `key[..slash]` | 同上 |
| I5 | `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs` | 19 | `key[..slash]` | 同上 |
| I6 | `crates/s3-gallery-cli/src/web/handlers/browse.rs` | 193 | `path[..slash]` | 同上 |

### 1.4 panic!() 在测试代码中（1 处）

| # | 文件 | 行 | 代码 | 标准引用 |
|---|------|:--:|------|---------|
| P1 | `tests/lock_test.rs` | 80 | `panic!("expected Err, got Ok")` | `#![deny(clippy::panic)]` |

### 1.5 let _ = 丢弃 must_use 值（1 处生产代码）

| # | 文件 | 行 | 代码 | 标准引用 |
|---|------|:--:|------|---------|
| L1 | `crates/s3-gallery-cli/src/web/handlers/thumbnail.rs` | 129 | `let _ = ThumbnailEntry::insert(...)` | `#![deny(clippy::let_underscore_must_use)]` |

---

## 二、类型安全违规（23 项）

### 2.1 高优先级（12 项）

| # | 文件 | 行 | 问题 | 当前类型 | 应为类型 |
|---|------|:--:|------|:--------:|:--------:|
| T1 | `db/models.rs` | 181 | `FileEntry.file_type` 是 String | `String` | `FileType` 枚举 |
| T2 | `db/models.rs` | 183 | `FileEntry.metadata_state` 是 String | `String` | `MetadataState` 枚举 |
| T3 | `db/models.rs` | 346 | `MetadataEntry.namespace` 是 String | `String` | `MetadataNamespace` 枚举 |
| T4 | `db/models.rs` | 536 | `TagEntry.tag_type` 是 String | `String` | `TagType` 枚举 |
| T5 | `s3/config.rs` | 106 | `HostIdentifier.host_type` 是 String | `String` | `HostType` 枚举 |
| T6 | `db/models.rs` | 23 | `HostConfigEntry.host_type` 是 String | `String` | `HostType` 枚举 |
| T7 | `s3/traffic_recorder.rs` | 26 | `TrafficRecord.direction` 是 String | `String` | `Direction` 枚举 |
| T8 | `s3/traffic_recorder.rs` | 24 | `TrafficRecord.business` 是 String | `String` | `BusinessLabel` 枚举 |
| T9 | `db/models.rs` | 838 | `TrafficLogEntry.operation` 是 String | `String` | `S3Operation` 枚举 |
| T10 | `db/models.rs` | 839 | `TrafficLogEntry/TrafficStatsEntry.direction` 是 String | `String` | `Direction` 枚举 |
| T11 | `scan/pipeline.rs` | 114 | `ExifRequest.file_type` 是 String | `String` | `FileType` 枚举 |
| T12 | `scan/pipeline.rs` | 137 | `TagRequest.file_type` 是 String | `String` | `FileType` 枚举 |

### 2.2 中优先级（5 项）

| # | 文件 | 行 | 问题 | 建议 |
|---|------|:--:|------|------|
| T13 | `extractor/registry.rs` | 17 | `MetadataExtractor::supports()` 参数 `file_type: &str` | 改为 `&FileType` |
| T14 | `scan/pipeline.rs` | 55 | `HostDiffResult.file_type_counts: HashMap<String, u64>` | 改为 `HashMap<FileType, u64>` |
| T15 | `scan/pipeline.rs` | 94 | `AggregateReport.file_type_breakdown: HashMap<String, u64>` | 改为 `HashMap<FileType, u64>` |
| T16 | `view/stat.rs` | 21 | `FileStats.by_category: HashMap<String, u64>` | 改为 `HashMap<FileCategory, u64>` |
| T17 | `view/stat.rs` | 23 | `FileStats.by_file_type: HashMap<String, u64>` | 改为 `HashMap<FileType, u64>` |

### 2.3 低优先级（6 项）

| # | 文件 | 行 | 问题 | 建议 |
|---|------|:--:|------|------|
| T18 | `db/models.rs` | 440 | `ThumbnailEntry.format` 是 String | 改为 `ThumbnailFormat` 枚举 |
| T19 | `db/models.rs` | 169 | `FileEntry.key` 是 String | 改为 `ObjectKey` 类型 |
| T20 | `db/models.rs` | 173 | `FileEntry.etag` 是 String | 改为 `Etag` 类型 |
| T21 | `db/models.rs` | 172 | `FileEntry.size` 是 i64 | 改为 `FileSize` 类型 |
| T22 | `s3/traffic_recorder.rs` | 128 | 字符串字面量 "download"/"upload" 匹配 | 改为 `Direction` 枚举 |
| T23 | `scan/pipeline.rs` | 113 | `ExifRequest.ext` 是 String | 改为 `FileExtension` 类型 |

---

## 三、副作用管理违规

### 3.1 Web handler 命名违规（2 处）

| # | 文件 | 行 | Handler | 调用 S3 操作 | 应加前缀 |
|---|------|:--:|:-------:|:------------:|:--------:|
| S1 | `web/handlers/download.rs` | 44 | `download` | `get_object` | `fetch_` |
| S2 | `web/handlers/thumbnail.rs` | 30 | `thumbnail` | `get_object` | `fetch_` |

### 3.2 S3 操作函数命名违规（4 处）

| # | 文件 | 行 | 函数 | 调用 S3 操作 | 应加前缀 |
|---|------|:--:|:-----:|:------------:|:--------:|
| N1 | `s3/lock.rs` | 69 | `LockGuard::release()` | `delete_object` | `upload_` 或 `delete_` |
| N2 | `s3/lock.rs` | 87 | `LockGuard::renew()` | `put_object` | `upload_` |
| N3 | `s3/lock.rs` | 113 | `acquire_lock()` | `put_object_if_none_match` | `upload_` |
| N4 | `s3/lock.rs` | 153 | `check_lock()` | `head_object` (via `object_exists`) | `fetch_` |

### 3.3 RemoteView 模式缺失

| # | 问题 | 说明 |
|---|------|------|
| R1 | `RemoteView` 结构体不存在 | 原始设计用 `RemoteView` 持有 S3 引用，通过类型系统隔离 IO 操作。当前代码已废弃此模式，web handler 直接调用 `state.s3_with_traffic()` 获取 S3 客户端，无类型层面约束。 |

---

## 四、公开 API 表面泄漏（9 项）

### 4.1 高优先级（3 项）

| # | 模块 | 泄漏项 | 当前可见性 | 建议 |
|---|------|--------|:----------:|------|
| A1 | `scan/mod.rs` | 10 个内部模块公开（pipeline, discover, diff_layer, process, aggregate, batch_service, exif_service, tag_service, scan_objects, diff） | `pub mod` | 改为 `pub(crate) mod` |
| A2 | `s3/mock.rs` | `MockS3Client` 整个结构体 | `pub` | 加 `#[cfg(feature = "test-utils")]` |
| A3 | `s3/real.rs` | `pub client: aws_sdk_s3::Client` 字段 | `pub` | 改为私有，提供 accessor |

### 4.2 中优先级（4 项）

| # | 模块 | 泄漏项 | 建议 |
|---|------|--------|------|
| A4 | `s3/traffic_persist.rs` | `pub sender: mpsc::Sender<TrafficRecord>` | 改为 `pub(crate)` |
| A5 | `s3/logged.rs` | `LoggedS3Client` 公开 | 改为 `pub(crate)` |
| A6 | `s3/traffic_recorder.rs` | `BusinessS3Client` 公开 | 改为 `pub(crate)` |
| A7 | `s3/lock.rs` | `LockLease` 公开 | 改为 `pub(crate)` |

### 4.3 低优先级（2 项）

| # | 模块 | 泄漏项 | 建议 |
|---|------|--------|------|
| A8 | `s3/s3_service.rs` | `into_inner()` 公开 | 改为 `pub(crate)` |
| A9 | `db/mod.rs` | `pub use self::status::*` 通配符 | 显式导出 |

---

## 五、测试覆盖不足（9 项）

| # | 模块 | 行数 | 现状 |
|---|------|:----:|------|
| C1 | `RealS3Client` (`s3/real.rs`) | 293 | 8 个公开方法，仅被间接测试 |
| C2 | `ProcessLayer` (`scan/process.rs`) | 182 | 无测试模块 |
| C3 | `ExifService` (`scan/exif_service.rs`) | 130 | 无测试 |
| C4 | `TagService` (`scan/tag_service.rs`) | 85 | 无测试 |
| C5 | `DiffLayer` (`scan/diff_layer.rs`) | 140 | 无测试 |
| C6 | `BatchService` (`scan/batch_service.rs`) | 74 | 含 `expect` 但无测试 |
| C7 | `ScanPipeline` types (`scan/pipeline.rs`) | 165 | 无测试 |
| C8 | `view/traffic.rs` | 45-161 | 仅 2 个测试函数，`unwrap()` 边缘情况未覆盖 |
| C9 | `s3/lock.rs` | 172-179 | 测试辅助函数用 `loop {}` 做 fallback，失败时永远挂起 |

---

## 六、锁管理关注点（3 项）

| # | 文件 | 行 | 问题 |
|---|------|:--:|------|
| L1 | `s3/lock.rs` | 172 | `test_key()` 用 `loop {}` 做 fallback，构造失败时测试永远挂起 |
| L2 | `s3/lock.rs` | 177 | `test_bucket()` 同上 |
| L3 | `s3/lock.rs` | 39 | `#[must_use]` 不能防止绑定变量后超出作用域静默丢弃，锁可能泄漏 |