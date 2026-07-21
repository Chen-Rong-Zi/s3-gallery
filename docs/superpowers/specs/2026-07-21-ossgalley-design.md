# ossgalley — 跨平台 OSS 媒体文件浏览 Gallery

## 概述

ossgalley 是一个使用 Rust 构建的跨平台工具，用于浏览 OSS（S3 兼容）上的媒体文件。它通过 SQLite 数据库缓存文件元数据和缩略图，支持离线浏览和增量扫描，并提供 CLI 和 Web 两种交互方式。

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                     Users                                   │
├──────────────┬──────────────────────┬──────────────────────┤
│   Terminal   │    Web Browser       │   Android (future)   │
├──────────────┼──────────────────────┼──────────────────────┤
│ ossgalley-cli│   ossgalley-web      │   ossgalley-android   │
│ (clap)       │ (axum + HTML/HTMX)   │   (TBD)              │
├──────────────┴──────────────────────┴──────────────────────┤
│                  ossgalley-core                            │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │ s3       │  │ db       │  │ scan     │  │ view      │  │
│  │ client   │  │ engine   │  │ scanner  │  │ queries   │  │
│  └──────────┘  └──────────┘  └──────────┘  └───────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                 │
│  │ lock     │  │extractor │  │thumbnail │                 │
│  │ manager  │  │registry  │  │generator │                 │
│  └──────────┘  └──────────┘  └──────────┘                 │
├─────────────────────────────────────────────────────────────┤
│                 OSS (S3 Compatible)                         │
│  my-bucket/host-alpha/                                      │
│  └── .ossgallery/{ossgallery.db, db.lock, host.config.json} │
│  └── photos/…                                               │
└─────────────────────────────────────────────────────────────┘
```

### 核心原则

- **全异步架构**：所有 crate 统一使用 `tokio` 运行时，核心库对外暴露 async API
- **分层解耦**：`core` 不依赖任何展示层，CLI 和 Web 是薄包装
- **扫描者/消费者分离**：支持定时扫描 + 消费者只读浏览，降低 OSS 请求成本
- **增量扫描**：基于 `start_after` 和 ETag 对比，避免全量 ListObjects

### 扫描者 vs 消费者

```
扫描者角色（writer）：
  - acquire lock → 全量/增量扫描 → 更新 DB → 上传 DB 到 OSS → release lock
  - 适合 crontab/systemd timer 定时执行

消费者角色（reader）：
  - 下载 DB → 本地查询（零 OSS 请求）→ 按需懒加载缩略图/EXIF
  - `--readonly` 模式完全不写 OSS

回退策略：
  - 启动时检查：本地缓存 → OSS 上 DB → 自动扫描
  - 全程自动决策，用户无感
```

## 目录结构

### OSS 主机目录结构

```
my-bucket/
└── <host-name>/
    ├── .ossgallery/
    │   ├── host.config.json    # 主机配置
    │   ├── ossgallery.db       # SQLite 数据库
    │   └── db.lock             # 租约锁
    ├── photos/
    ├── videos/
    └── ...
```

### host.config.json

```json
{
    "host_id": "a1b2c3d4-...",
    "host_name": "我的相机",
    "host_type": "camera",
    "description": "旅行照片备份",
    "created_at": "2026-07-21T10:00:00Z",
    "version": 1
}
```

### 租约锁 (db.lock)

```json
{
    "client_id": "uuid-v4",
    "acquired_at": "2026-07-21T10:00:00Z",
    "expires_at": "2026-07-21T12:00:00Z"
}
```

- 锁租期 2 小时，客户端可续约
- 过期后其他客户端可抢占
- `scan --force` 忽略锁
- `db unlock` 强制释放（危险操作）

## 项目结构

```
ossgalley/
├── Cargo.toml                          # [workspace]
├── crates/
│   ├── ossgalley-core/                 # 核心库
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── s3/                     # S3Client trait + 实现
│   │       │   ├── client.rs
│   │       │   ├── config.rs
│   │       │   └── lock.rs
│   │       ├── db/                     # sqlx 数据库
│   │       │   ├── pool.rs
│   │       │   ├── schema.rs
│   │       │   └── models.rs
│   │       ├── scan/                   # 扫描引擎
│   │       │   ├── scanner.rs
│   │       │   └── diff.rs
│   │       ├── view/                   # 查询引擎
│   │       │   ├── ls.rs, tree.rs, stat.rs
│   │       │   ├── search.rs, timeline.rs, tags.rs
│   │       │   ├── duplicates.rs, export.rs
│   │       │   └── mod.rs
│   │       ├── extractor/              # 元数据提取器
│   │       │   ├── registry.rs, exif.rs, mp4.rs, audio.rs
│   │       ├── classify/classifier.rs
│   │       ├── thumbnail/generator.rs
│   │       └── error.rs
│   ├── ossgalley-cli/                  # CLI 二进制
│   │   └── src/
│   │       ├── main.rs, cli.rs
│   │       ├── cmd_init.rs, cmd_scan.rs
│   │       ├── cmd_view.rs, cmd_db.rs
│   │       └── cmd_serve.rs
│   ├── ossgalley-web/                  # Web 二进制
│   │   └── src/
│   │       ├── main.rs, router.rs
│   │       ├── handlers/ (browse, gallery, search, files, ...)
│   │       └── templates/ (HTML + HTMX 片段)
│   └── ossgalley-macros/               # 可选自定义宏
├── tests/                              # 集成测试
├── scripts/                            # MinIO 辅助脚本
└── docs/
    └── superpowers/specs/
```

## 数据库 Schema

```sql
-- 主机配置表
CREATE TABLE host_config (
    host_id TEXT PRIMARY KEY,
    host_name TEXT NOT NULL,
    host_type TEXT NOT NULL DEFAULT 'unknown',
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

-- 文件清单（核心表）
CREATE TABLE files (
    key TEXT PRIMARY KEY,
    etag TEXT NOT NULL,
    size INTEGER NOT NULL,
    last_modified TEXT NOT NULL,
    content_type TEXT,                       -- 从扩展名派生，非来自 S3 ListObjects
    file_type TEXT NOT NULL,
    metadata_state TEXT NOT NULL DEFAULT 'pending',  -- pending / complete / partial / failed
    is_deleted INTEGER NOT NULL DEFAULT 0
);

-- 聚类规则表（用户可扩展）
CREATE TABLE classification_rules (
    extension TEXT PRIMARY KEY,
    file_type TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    description TEXT
);

-- 提取器规则表（多对多）
CREATE TABLE extractor_rules (
    extension TEXT NOT NULL,
    extractor_name TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (extension, extractor_name)
);

-- 通用元数据表（namespace + key-value 结构）
CREATE TABLE metadata (
    file_key TEXT NOT NULL,
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    extracted_at TEXT NOT NULL,
    partial INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (file_key, namespace, key),
    FOREIGN KEY (file_key) REFERENCES files(key) ON DELETE CASCADE
);

-- 缩略图表
CREATE TABLE thumbnails (
    file_key TEXT PRIMARY KEY,
    data BLOB NOT NULL,
    format TEXT NOT NULL DEFAULT 'jpeg',
    width INTEGER,
    height INTEGER,
    cached_at TEXT NOT NULL,
    FOREIGN KEY (file_key) REFERENCES files(key) ON DELETE CASCADE
);

-- 标签表
CREATE TABLE tags (
    tag_id INTEGER PRIMARY KEY AUTOINCREMENT,
    tag_name TEXT NOT NULL UNIQUE,
    tag_type TEXT NOT NULL DEFAULT 'auto'
);

-- 文件-标签关联
CREATE TABLE file_tags (
    file_key TEXT NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (file_key, tag_id),
    FOREIGN KEY (file_key) REFERENCES files(key) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(tag_id) ON DELETE CASCADE
);

-- 扫描元数据
CREATE TABLE scan_metadata (
    last_scanned_key TEXT,
    last_scanned_at TEXT,
    total_files INTEGER,
    total_size INTEGER,
    db_schema_version INTEGER NOT NULL DEFAULT 1
);

-- 索引
CREATE INDEX idx_files_file_type ON files(file_type);
CREATE INDEX idx_files_last_modified ON files(last_modified);
CREATE INDEX idx_metadata_namespace ON metadata(namespace);
CREATE INDEX idx_metadata_file_key ON metadata(file_key);
CREATE INDEX idx_metadata_key_value ON metadata(key, value);
CREATE INDEX idx_tags_tag_type ON tags(tag_type);
CREATE INDEX idx_thumbnails_cached_at ON thumbnails(cached_at);
```

## OSS 请求优化策略

1. **ListObjectsV2 一次性获取**：`aws-sdk-s3` 的 `ListObjectsV2` 返回的 `Object` 结构体直接包含 `key`、`e_tag`、`size`、`last_modified`，**无需额外的 HEAD 请求**。扫描时一次分页遍历即可完成全部文件的元数据采集。
2. **增量扫描**：`ListObjectsV2(prefix, start_after=last_key)`，避免全量遍历
3. **ETag 变更检测**：ListObjects 拿到的 ETag 与 DB 中记录的对比，仅 ETag 变化的文件才需要下载内容
4. **Range 请求**：获取 EXIF 仅下载文件头部 64KB（JPEG）/ 128KB（RAW），不下载完整文件
5. **缩略图懒加载**：仅当用户浏览图片视图时按需下载并缓存到 DB，扫描阶段不生成缩略图
6. **DB 共享**：扫描者上传 DB，消费者直接下载使用，避免重复扫描

> **注意**：`content_type` 不在 ListObjectsV2 返回值中，但本项目按扩展名映射 content_type（如 `.jpg` → `image/jpeg`），不需要从 S3 获取。`files.content_type` 列在扫描时由分类器根据扩展名填充。HEAD 请求仅在元数据提取前的 ETag 二次确认等场景使用。

## 元数据提取器系统

```rust
#[async_trait]
pub trait MetadataExtractor: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, file_type: &str, extension: &str) -> bool;
    async fn extract(&self, data: &[u8], extension: &str) -> Result<Vec<MetadataItem>>;
}

pub struct MetadataItem {
    pub key: String,
    pub value: String,       // JSON 编码
}

// 注册和使用
let mut registry = ExtractorRegistry::new();
registry.register(Box::new(ExifExtractor::new()));
registry.register(Box::new(Mp4Extractor::new()));
```

- 提取器通过 `extractor_rules` 表与扩展名关联（多对多）
- 所有提取器输出统一写入 `metadata` 表（namespace + key-value）
- 新增提取器只需实现 trait + 注册，无需修改 Schema

## CLI 命令设计

```
ossgalley
├── init <bucket> <host-dir>         # 初始化主机
├── scan <host>                      # 扫描/更新 db
│   ├── --incremental                 # 增量扫描（从上次断点继续）
│   ├── --schedule                    # 定时模式（锁被占有时静默跳过）
│   ├── --force                       # 忽略 lock，强制扫描
│   ├── --no-metadata                 # 跳过元数据提取，仅扫描文件清单
│   ├── --with-thumbnails             # 额外生成缩略图（昂贵，需完整下载文件）
│   └── --concurrency <n>             # 并发请求数（默认 10，控制 ListObjects 和元数据提取并发，缩略图生成固定 3）
├── view <host>                      # 查询 db（文本输出）
│   ├── ls [path]                    # 列出目录
│   ├── tree [path]                  # 目录树
│   ├── stat [path]                  # 统计信息
│   ├── files <key>                  # 文件详情
│   ├── search <query>               # 搜索
│   ├── metadata
│   │   ├── ls <namespace>           # 列出命名空间键
│   │   └── query <ns:key>           # 元数据查询
│   ├── types                        # 类型分布
│   ├── duplicates                   # 重复文件
│   ├── largest [n]                  # 最大文件
│   ├── recent [n]                   # 最近文件
│   ├── oldest [n]                   # 最旧文件
│   ├── timeline                     # 时间轴
│   ├── tags
│   │   ├── list                     # 标签列表
│   │   ├── files <tag>              # 标签下文件
│   │   └── stats                    # 标签统计
│   ├── diff
│   │   ├── added                    # 新增文件
│   │   ├── removed                  # 已删除文件
│   │   └── changed                  # 变更文件
│   ├── export <format>              # 导出 csv/json
│   ├── schema                       # db 元信息
│   ├── locations                    # GPS 文件
│   ├── orphan                       # 已删除记录
│   └── health                       # 完整性检查
├── db                               # 管理 db 文件
│   ├── pull <host>                  # 下载 db
│   ├── push <host>                  # 上传 db
│   ├── status <host>                # db 状态
│   ├── lock <host>                  # 查看锁
│   └── unlock <host>                # 释放锁
└── serve <host>                     # 启动 Web 服务
    ├── --port <port>                # 默认 8080
    └── --readonly                   # 只读模式
```

### 认证配置优先级

CLI 参数 > 环境变量 > 配置文件 > 默认值

- 环境变量：`OSSGALLEY_ENDPOINT`, `OSSGALLEY_ACCESS_KEY`, `OSSGALLEY_SECRET_KEY`, `OSSGALLEY_REGION`
- 配置文件：`~/.config/ossgalley/config.toml`
- 默认值：`http://localhost:9000`, `s3oss`, `s3oss1234`, `us-east-1`

## Web 前端

### 技术栈

- **后端**：axum (Rust HTTP 框架)
- **模板**：minijinja（Jinja2 语法）
- **前端**：HTML + HTMX（无 Node.js 依赖）
- **数据通道**：Web 后端调用 `core::view` 输出 JSON → 模板渲染 HTML

### 页面路由

| 路由 | 功能 | 数据来源 |
|------|------|---------|
| `/` | 仪表盘 | `stat`, `types` |
| `/browse?path=...` | 文件浏览 | `ls`, `tree` |
| `/gallery` | 图片画廊 | `search --type image`, `timeline` |
| `/search?q=...` | 搜索 | `search` |
| `/timeline` | 时间轴 | `timeline` |
| `/tags` | 标签管理 | `tags list`, `tags stats` |
| `/files/<key>` | 文件详情 | `files`, `metadata` |
| `/duplicates` | 重复文件 | `duplicates` |
| `/stats` | 详细统计 | `stat`, `largest`, `recent` |
| `/settings` | 主机设置 | `schema` |

### 核心交互模式

- 初始加载返回完整 HTML
- HTMX 驱动后续交互，只返回 HTML 片段
- 排序、过滤、分页通过 HTMX 局部刷新
- 缩略图懒加载 (`loading="lazy"`)
- 图片画廊按时间轴分组，自动加载更多

## 开发环境

- **本地 OSS 测试**：MinIO (localhost:9000, s3oss/s3oss1234)
- **测试数据**：`scripts/seed-test-data.sh` 填充测试文件
- **测试策略**：`S3Client` 定义为 trait，提供 `MockS3Client` 实现
- **集成测试**：通过 MinIO 实例运行真实 S3 交互测试

## 错误处理

- 统一错误类型 `OssgalleyError`（基于 `thiserror`）
- 所有 async 函数返回 `Result<T, OssgalleyError>`
- CLI 错误输出到 stderr
- Web 错误返回 JSON 格式 `{"error": "...", "code": "..."}`

## 类型安全纪律（代码强制要求）

### 原则

**用类型系统将非法状态编码为编译错误，让运行时 bug 不可能发生。**

### 1. Newtype 模式：区分语义相同的原始类型

```rust
// ✅ 每个业务概念有独立类型，不同概念传反时编译错误
pub struct BucketName(String);
pub struct ObjectKey(String);
pub struct HostId(Uuid);
pub struct Etag(String);
pub struct FileSize(i64);
```

- `BucketName` 构造时验证 S3 命名规则（3-63 字符，小写字母数字连字符）
- `ObjectKey` 构造时验证不能为空、不以 `/` 开头
- 函数签名使用 Newtype 而非原始 `String`/`i64`

### 2. 枚举替代字符串

```rust
// ✅ 所有业务分类使用枚举，不是 String
pub enum FileType { Image, RawPhoto, Video, Audio, Archive, Text, Document, Other }
pub enum MetadataNamespace { Exif, Mp4, Audio, Xmp, Icc, Custom(String) }
pub enum HostType { Camera, Phone, Server, Project, Custom(String) }
pub enum LockState { Released, Acquired, Expired }
pub enum ViewSortField { Name, Size, Date, Type }
pub enum TimelineGranularity { Year, Month, Day }
```

- 禁止 `String` 作为分类字段的类型
- 枚举配合 `serde` 序列化/反序列化，与 DB 字符串互转
- 用户可扩展部分使用 `Custom(String)` 变体，不破坏枚举封闭性

### 3. 状态机模式：非法状态转换编译不通过

```rust
// ✅ 锁操作在类型层面体现状态
// 无锁状态 → 只有 acquire() 方法
// 持锁状态 → 只有 release() 和 renew() 方法
// 编译时保证：不会在无锁时释放锁，不会在持锁时重复获取

// 状态标记类型（零大小，编译时状态）
pub struct LockReleased;
pub struct LockAcquired;

// 最终实现使用 LockGuard（见副作用管理章节）
// 此处的状态机模式是概念说明，实际 API 以 LockGuard 为准
```

### 4. 枚举化错误类型

```rust
// ✅ 每个错误分支有明确类型，调用方必须处理所有分支
#[derive(Debug, thiserror::Error)]
pub enum OssgalleyError {
    #[error("Invalid bucket name: {0}")]        InvalidBucket(String),
    #[error("S3 operation failed: {0}")]        S3Error(#[from] aws_sdk_s3::Error),
    #[error("Lock held by {client_id}")]        LockHeld { client_id: Uuid, expires_at: DateTime<Utc> },
    #[error("Object not found: {key}")]         ObjectNotFound { key: String },
    #[error("Database error: {0}")]             Database(#[from] sqlx::Error),
    #[error("No database found.")]              NoDatabase(String),
    #[error("Host '{0}' already initialized")]  HostAlreadyInitialized(String),
    #[error("Host not found: {0}")]             HostNotFound(String),
    #[error("Metadata extraction failed")]      MetadataExtractionFailed { key: String, detail: String },
}
```

### 5. Result 类型表达业务语义

```rust
// ✅ 函数返回值用枚举表达多种结果，而非 Option 或 bool
pub enum AcquireLockResult {
    Acquired(LeaseLock<LockAcquired>),
    HeldByOther { client_id: Uuid, expires_at: DateTime<Utc> },
    Expired(LeaseLock<LockAcquired>),  // 可强制覆盖
}

pub enum EnsureDbResult {
    LocalCache(DbPool),     // 零 OSS 请求
    Downloaded(DbPool),     // 从 OSS 下载
    AutoScanned(DbPool),   // 自动扫描生成
}

// ✅ 扫描结果结构体所有字段必有值，无 Option
pub struct ScanResult {
    pub total_files: u64,
    pub total_size: u64,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub metadata_extracted: u64,
    pub duration: Duration,
}
```

### 6. DB 查询数据返回类型化结构体

```rust
// ✅ 从 DB 读出的数据强制绑定到类型化结构体，不暴露原始 Row
#[derive(sqlx::FromRow)]
pub struct FileEntry {
    pub key: ObjectKey,
    pub etag: Etag,
    pub size: FileSize,           // FileSize 实现 Display 显示 "12.3 MB"
    pub last_modified: DateTime<Utc>,
    pub file_type: FileType,      // 枚举，非 String
}

#[derive(sqlx::FromRow)]
pub struct MetadataEntry {
    pub file_key: ObjectKey,
    pub namespace: MetadataNamespace,
    pub key: String,
    pub value: serde_json::Value,  // JSON 类型自描述
    pub partial: bool,             // bool，非 i32 0/1
}
```

### 7. 测试策略

#### 测试层级

```
           ┌──────────┐
           │  E2E 测试 │  ← 真实 MinIO + 真实 CLI 命令
           │   (3-5)  │     耗时最长，覆盖关键路径
           └────┬─────┘
                │
           ┌────▼─────┐
           │ 集成测试  │  ← MockS3Client + 真实 sqlite DB
           │ (20-30)  │     模块间交互、扫描→DB→view 流程
           └────┬─────┘
                │
           ┌────▼─────┐
           │ 单元测试  │  ← 纯内存，零 IO
           │ (80-120) │     每个函数、每个边界条件
           └────┬─────┘
                │
           ┌────▼─────┐
           │ 属性测试  │  ← proptest 随机输入
           │ (10-15)  │     发现边界情况
           └────┬─────┘
                │
           ┌────▼─────┐
           │ 模糊测试  │  ← libfuzzer 随机字节
           │  (2-3)   │     提取器对损坏数据不 panic
           └──────────┘
```

#### 1. 单元测试（80-120 个）

每个核心函数都有测试，覆盖正常路径 + 所有错误路径。

```rust
// ── 测试文件组织结构（与源码一一对应） ──
src/
├── s3/
│   ├── mod.rs
│   ├── client.rs
│   ├── config.rs
│   └── config_test.rs          // OssConfig 验证测试
├── db/
│   ├── mod.rs
│   ├── schema.rs
│   ├── schema_test.rs          // 建表迁移测试
│   └── models_test.rs          // 类型序列化/反序列化测试
├── classify/
│   ├── classifier.rs
│   └── classifier_test.rs      // 文件类型分类测试
├── extractor/
│   ├── mod.rs
│   ├── exif.rs
│   ├── exif_test.rs            // EXIF 提取测试
│   └── registry_test.rs        // 提取器注册查找测试
├── view/
│   ├── ls_test.rs              // 目录列表查询测试
│   ├── search_test.rs          // 搜索查询测试
│   ├── timeline_test.rs        // 时间轴查询测试
│   ├── duplicates_test.rs      // 重复文件查找测试
│   └── tags_test.rs            // 标签查询测试
├── thumbnail/
│   ├── generator.rs
│   └── generator_test.rs       // 缩略图生成测试
├── lock/
│   ├── lock_test.rs            // 锁状态机测试
│   └── concurrency_test.rs     // 并发锁竞争测试
└── error.rs
```

**单元测试覆盖的边界条件（每个函数至少测试 3 种路径）：**

| 类别 | 测试案例 | 预期 |
|------|---------|------|
| Newtype | `BucketName::new("")` | `Err(InvalidBucket)` |
| Newtype | `BucketName::new("ab")` | `Err(InvalidBucket)`（< 3 字符） |
| Newtype | `BucketName::new("a-b-c")` | `Ok(BucketName)` |
| Newtype | `ObjectKey::new("a/../b")` | `Err(PathTraversal)` |
| Newtype | `ObjectKey::new("/abs")` | `Err(InvalidKey)` |
| 枚举 | `FileType::from_extension("jpg")` | `Some(FileType::Image)` |
| 枚举 | `FileType::from_extension("unknown")` | `None` |
| 枚举 | 序列化/反序列化 roundtrip | 所有变体守恒 |
| 分类 | 大小写不敏感 | `"JPG"` 和 `"jpg"` 结果相同 |
| 分类 | 自定义规则覆盖内置规则 | 自定义优先 |
| 锁 | `LockReleased` 上调用 `release()` | 编译错误 |
| 锁 | `LockAcquired` 上调用 `release()` | `Ok(())` |
| 锁 | 两次 `release()` | 编译错误（所有权） |
| Result | 所有 Result 函数 | 不会 panic |

#### 2. 属性测试（10-15 个，使用 proptest）

```rust
#[cfg(test)]
mod proptest {
    use proptest::prelude::*;

    proptest! {
        // 任何随机字符串都不应导致 panic
        #[test]
        fn object_key_rejects_path_traversal(key in ".*\\\\.\\\\.*") {
            prop_assert!(ObjectKey::new(&key).is_err());
        }

        #[test]
        fn bucket_name_length_validation(name in "[a-z0-9-]{1,100}") {
            let result = BucketName::new(&name);
            if name.len() < 3 || name.len() > 63 {
                prop_assert!(result.is_err());
            } else {
                prop_assert!(result.is_ok());
            }
        }

        #[test]
        fn file_type_roundtrip(value in any::<FileType>()) {
            let json = serde_json::to_string(&value).unwrap();
            let deserialized: FileType = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(value, deserialized);
        }

        #[test]
        fn file_classification_never_panics(ext in "\\w{0,20}") {
            let _ = FileType::from_extension(&ext);
        }
    }
}
```

#### 3. 集成测试（20-30 个）

使用 `MockS3Client` + 真实 sqlite DB，测试模块间交互。

```rust
// tests/common/mod.rs
pub struct TestEnv {
    pub s3: MockS3Client,
    pub db: DbPool,
    pub tmp_dir: TempDir,
    pub host: HostIdentifier,
}

impl TestEnv {
    /// 创建测试环境
    /// - MockS3Client 预置测试数据
    /// - 临时目录中的 sqlite DB
    pub async fn new() -> Self {
        let tmp_dir = TempDir::new().unwrap();
        let db_path = tmp_dir.path().join("test.db");
        let db = DbPool::open(&db_path).await.unwrap();
        db.run_migrations().await.unwrap();
        Self {
            s3: MockS3Client::with_fixtures(vec![
                "photos/IMG_001.jpg",  // 12MB, ETag: "abc"
                "photos/IMG_002.jpg",  // 8MB,  ETag: "def"
                "videos/clip.mp4",      // 50MB, ETag: "ghi"
                "docs/notes.txt",       // 1KB,  ETag: "jkl"
            ]),
            db,
            tmp_dir,
            host: HostIdentifier::new("test-bucket", "test-host").unwrap(),
        }
    }
}

// ── 集成测试案例列表 ──

// 1. 扫描流程
#[tokio::test]
async fn test_full_scan_populates_db() { ... }
#[tokio::test]
async fn test_incremental_scan_only_new_files() { ... }
#[tokio::test]
async fn test_scan_detects_deleted_files() { ... }
#[tokio::test]
async fn test_scan_detects_changed_etag() { ... }

// 2. 锁机制
#[tokio::test]
async fn test_lock_prevents_concurrent_scans() { ... }
#[tokio::test]
async fn test_lock_expiry_allows_override() { ... }
#[tokio::test]
async fn test_lock_release_after_scan() { ... }

// 3. 查询流程
#[tokio::test]
async fn test_ls_root_directory() { ... }
#[tokio::test]
async fn test_ls_with_sort_and_filter() { ... }
#[tokio::test]
async fn test_search_by_filename() { ... }
#[tokio::test]
async fn test_search_by_metadata() { ... }
#[tokio::test]
async fn test_timeline_by_month() { ... }
#[tokio::test]
async fn test_duplicates_detection() { ... }

// 4. 元数据提取
#[tokio::test]
async fn test_exif_extraction_range_request() { ... }
#[tokio::test]
async fn test_metadata_cache_hit() { ... }
#[tokio::test]
async fn test_metadata_cache_miss_triggers_fetch() { ... }

// 5. DB 生命周期
#[tokio::test]
async fn test_ensure_db_local_cache_hit() { ... }
#[tokio::test]
async fn test_ensure_db_download_from_oss() { ... }
#[tokio::test]
async fn test_ensure_db_auto_scan() { ... }
#[tokio::test]
async fn test_ensure_db_readonly_no_db() { ... }
```

#### 4. E2E 测试（3-5 个）

使用真实 MinIO 实例，测试完整的 CLI 命令流程。

```rust
// tests/e2e_test.rs
// 标记为 #[ignore]，仅在 CI 中运行（需要 MinIO 实例）

/// 场景：初始化 → 扫描 → 查询 → 再次扫描（增量）
#[tokio::test]
#[ignore]
async fn test_e2e_init_scan_query() {
    let s3 = create_real_s3_client();
    let bucket = "e2e-test-bucket";

    // 1. 创建测试 bucket 和文件
    s3.create_bucket(bucket).await.unwrap();
    s3.put_object(bucket, "test-host/photo.jpg", ...).await.unwrap();

    // 2. CLI init
    let output = Command::new("ossgalley")
        .args(["init", bucket, "test-host", "--endpoint", "http://localhost:9000"])
        .output().unwrap();
    assert!(output.status.success());

    // 3. CLI scan
    let output = Command::new("ossgalley")
        .args(["scan", "test-bucket/test-host", "--endpoint", "http://localhost:9000"])
        .output().unwrap();
    assert!(output.status.success());

    // 4. CLI view ls
    let output = Command::new("ossgalley")
        .args(["view", "test-bucket/test-host", "ls", "--endpoint", "http://localhost:9000"])
        .output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("photo.jpg"));

    // 5. 清理
    s3.delete_bucket(bucket).await.unwrap();
}
```

#### 5. 模糊测试（2-3 个）

```rust
// tests/fuzz_targets/exif_extractor.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // 任何随机字节都不应导致 panic
    let extractor = ExifExtractor::new();
    let _ = extractor.extract(data, "jpg");
});

// tests/fuzz_targets/object_key.rs
fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(&data);
    let _ = ObjectKey::new(&s);
});
```

#### 6. 文档测试

```rust
/// 获取 S3 对象的 ETag。
///
/// # 示例
///
/// ```rust
/// # use ossgalley_core::*;
/// # async fn example() -> Result<()> {
/// let client = MockS3Client::new();
/// let bucket = BucketName::new("my-bucket")?;
/// let key = ObjectKey::new("photo.jpg")?;
/// let etag = client.get_etag(&bucket, &key).await?;
/// assert!(etag.as_str().starts_with('"'));
/// # Ok(())
/// # }
/// ```
pub async fn get_etag(&self, bucket: &BucketName, key: &ObjectKey) -> Result<Etag>;

// cargo test 自动编译并运行 doc test
// 确保文档中的示例代码始终可用
```

#### 测试在 CI 中的位置

```yaml
# 每次 push/PR 运行（< 5 分钟）：
cargo test --lib                    # 单元测试
cargo test --doc                     # 文档测试
cargo test --test proptest          # 属性测试（快速模式）

# 每晚运行（< 15 分钟）：
cargo test --test integration       # 集成测试（需 MinIO）
cargo test --test e2e               # E2E 测试（需 MinIO）
cargo fuzz run exif_extractor -- -max_total_time=60  # 模糊测试
cargo tarpaulin --out xml           # 覆盖率报告

# PR 合并前必须通过：
cargo test --lib                     # 100% 通过
cargo test --doc                     # 100% 通过
cargo clippy -D warnings             # 零警告
```

#### 测试覆盖率目标

| 层级 | 目标覆盖率 | 检查时机 |
|------|-----------|---------|
| 核心类型（Newtype、枚举） | 100% | 每次 PR |
| 分类器 | 100% | 每次 PR |
| 提取器注册表 | 100% | 每次 PR |
| 查询引擎（view） | > 90% | 每次 PR |
| 扫描引擎 | > 85% | 每晚 |
| 锁管理 | 100% | 每次 PR |
| CLI 命令 | > 70% | 每晚 |
| Web handler | > 60% | 每晚 |

#### 测试数据管理

```rust
// 测试辅助函数，所有测试共享

// tests/common/mod.rs
pub struct TestFixture {
    pub s3: MockS3Client,
    pub db: DbPool,
    pub host: HostIdentifier,
}

impl TestFixture {
    /// 创建空测试环境
    pub async fn new() -> Self { ... }

    /// 创建预置数据的测试环境
    pub async fn with_files(files: &[FileSpec]) -> Self { ... }

    /// 创建包含扫描结果的测试环境
    pub async fn with_scanned_host(files: &[FileSpec]) -> Self { ... }

    /// 清理临时文件
    pub fn cleanup(self) { ... }
}

pub struct FileSpec {
    pub key: &'static str,
    pub etag: &'static str,
    pub size: u64,
    pub file_type: FileType,
    pub metadata: Option<Vec<MetadataItem>>,
}
```

#### 测试类型对比总表

| 测试类型 | 数量 | 运行时间 | 依赖 | CI 触发 | 门禁 |
|---------|------|---------|------|---------|------|
| 单元测试 | 80-120 | < 30s | 无 | 每次 PR | 阻塞 |
| 文档测试 | 10-20 | < 10s | 无 | 每次 PR | 阻塞 |
| 属性测试 | 10-15 | < 30s | 无 | 每次 PR | 阻塞 |
| 集成测试 | 20-30 | < 2min | MockS3 + sqlite | 每次 PR | 阻塞 |
| E2E 测试 | 3-5 | < 5min | MinIO 实例 | 每晚 | 不阻塞 |
| 真实 OSS 测试 | 2-3 | < 10min | 真实 S3 兼容 OSS | 手动触发 | 不阻塞（需凭证） |
| 模糊测试 | 2-3 | < 1min | 无 | 每晚 | 不阻塞 |
| 覆盖率 | - | < 2min | 无 | 每晚 | 不阻塞（> 80% 警告） |

#### 测试指标详解

##### 单元测试指标

```
通过条件:
  - 所有测试函数返回 Ok(()) 或不 panic
  - 每个 #[test] 函数独立运行，互不依赖
  - 每个函数至少覆盖 3 条路径：正常路径、边界路径、错误路径

量化指标:
  - 数量: 80-120 个
  - 密度: 每个公开函数至少 1 个测试，核心函数至少 3 个
  - 覆盖率: 核心类型（Newtype、枚举）100% 行覆盖
  - 执行时间: 全部 < 30s

失败后果:
  - 任何单元测试失败 → PR 阻塞
  - 不允许使用 #[should_panic] —— 错误必须通过 Result 返回
  - 必须使用 assert!(result.is_err()) 而非 catch_unwind

示例:
  #[test]
  fn bucket_name_too_short_is_rejected() {
      let result = BucketName::new("ab");
      assert!(result.is_err());
      assert!(matches!(result.unwrap_err(), OssgalleyError::InvalidBucket(_)));
  }
```

##### 文档测试指标

```
通过条件:
  - 文档中所有 ```rust 代码块被 cargo test --doc 编译并执行通过
  - 每个公开 API 必须有文档示例（clippy::missing_docs_in_private_items 控制）

量化指标:
  - 数量: 10-20 个
  - 覆盖: 每个公开函数至少 1 个可运行的示例
  - 执行时间: 全部 < 10s

失败后果:
  - 编译失败或运行失败 → PR 阻塞
  - 文档与代码不同步 → 编译失败（示例代码过时）

示例:
  /// 获取 BucketName 的字符串表示。
  ///
  /// ```rust
  /// # use ossgalley_core::BucketName;
  /// let bucket = BucketName::new("my-bucket").unwrap();
  /// assert_eq!(bucket.as_str(), "my-bucket");
  /// ```
  pub fn as_str(&self) -> &str;
```

##### 属性测试指标

```
通过条件:
  - 随机生成 100 组输入（CI 快速模式）或 10000 组（本地完整模式）
  - 所有输入不导致 panic
  - 所有断言通过

量化指标:
  - 数量: 10-15 个 proptest 函数
  - 覆盖: 输入验证（ObjectKey、BucketName）、枚举序列化、分类器
  - 随机种子: 固定种子可复现，失败时输出最小化案例（shrink）
  - 执行时间: 全部 < 30s（CI 快速模式）

失败后果:
  - 任何输入导致 panic → PR 阻塞
  - 失败时输出最小复现案例，可直接作为单元测试用例

示例:
  proptest! {
      #[test]
      fn bucket_name_validates_length(name in "[a-z0-9-]{1,100}") {
          let result = BucketName::new(&name);
          if name.len() < 3 || name.len() > 63 {
              prop_assert!(result.is_err());
          } else {
              prop_assert!(result.is_ok());
          }
      }
  }
  // 失败时输出类似：
  // test failed: minimal failing input: name = "a"
  // 可直接复制为固定测试用例
```

##### 集成测试指标

```
通过条件:
  - 模块间交互按预期工作
  - MockS3Client 模拟各种 S3 响应（正常、404、304、超时）
  - 真实 sqlite DB 验证 SQL 查询和 schema 迁移

量化指标:
  - 数量: 20-30 个
  - 覆盖场景:
    - 扫描：全量、增量、ETag 变化、文件删除
    - 锁：获取、释放、续约、竞争、过期
    - 查询：ls、tree、search、timeline、tags、duplicates
    - DB 生命周期：本地缓存、下载、自动扫描、只读报错
    - 元数据：缓存命中、缓存未命中、提取失败恢复
  - 执行时间: 全部 < 2min

失败后果:
  - 任何集成测试失败 → PR 阻塞
  - 失败时输出 mock 调用记录和 DB 状态，方便定位

示例:
  #[tokio::test]
  async fn test_scan_detects_changed_etag() {
      let env = TestEnv::with_files(&[FileSpec::new("photo.jpg", "etag-v1")]);
      env.s3.update_etag("photo.jpg", "etag-v2");  // 模拟 ETag 变更
      let result = env.scan().await.unwrap();
      assert_eq!(result.changed_files, 1);  // 检测到 1 个变更
      assert_eq!(result.db.get_etag("photo.jpg").unwrap(), "etag-v2");
  }
```

##### E2E 测试指标

```
通过条件:
  - 通过真实 CLI 命令（std::process::Command）执行完整场景
  - 输出符合预期（stdout 包含关键信息，exit code 为 0）
  - 测试结束后清理所有资源（bucket、文件、临时目录）

量化指标:
  - 数量: 3-5 个（每个场景一个）
  - 覆盖场景:
    - init → scan → view ls（完整生命周期）
    - scan → scan --incremental（增量扫描）
    - serve → curl API（Web 接口）
  - 执行时间: 全部 < 5min
  - 标记为 #[ignore]，仅在 CI 每晚运行或手动触发

失败后果:
  - 失败 → 不阻塞 PR，但发送报告（Slack/Email 通知）
  - 连续 3 次失败 → 自动创建 GitHub Issue

示例:
  #[tokio::test]
  #[ignore]
  async fn test_e2e_full_lifecycle() {
      // 使用真实 MinIO
      let output = Command::new("ossgalley")
          .args(["init", "e2e-bucket", "test-host"])
          .env("OSSGALLEY_ENDPOINT", "http://localhost:9000")
          .output().unwrap();
      assert!(output.status.success());
      // ... 后续步骤
  }
```

##### 真实 OSS 测试指标

```
通过条件:
  - 通过真实 S3 兼容 OSS（AWS S3、Cloudflare R2、Backblaze B2、MinIO 等）执行完整场景
  - 测试开始前验证凭证有效性，无效则跳过（不失败）
  - 测试结束后清理所有资源（bucket、文件、锁）

量化指标:
  - 数量: 2-3 个
  - 覆盖场景:
    - 真实 S3 上 init → scan → view ls（完整生命周期）
    - 真实 S3 上并发锁竞争测试（两个客户端同时扫描）
  - 执行时间: 全部 < 10min
  - 标记为 #[ignore] 且 #[cfg(feature = "real-s3-test")]，仅在 CI 手动触发时运行

使用的凭证（通过 GitHub Actions secrets 传入）:
  - OSSGALLEY_ENDPOINT — OSS endpoint URL
  - OSSGALLEY_REGION — OSS region（如 us-east-1）
  - OSSGALLEY_ACCESS_KEY — Access key
  - OSSGALLEY_SECRET_KEY — Secret key
  - OSSGALLEY_TEST_BUCKET — 测试用 bucket 名称（自动创建和清理）

安全要求:
  - 测试使用独立 bucket（OSSGALLEY_TEST_BUCKET），测试结束后强制清理
  - 测试结束后不论成功失败都执行清理（使用 Drop 或 finally 模式）
  - 凭证不写入日志，不输出到 stdout
  - 测试 bucket 名称包含随机后缀，避免冲突
  - 推荐使用专门的测试 OSS 账号，而非生产账号

失败后果:
  - 失败 → 输出诊断信息（不包含凭证）
  - 不阻塞 PR，不发送通知
  - 人工检查结果后决定是否修复

示例:
  #[tokio::test]
  #[ignore]
  #[cfg(feature = "real-s3-test")]
  async fn test_real_s3_init_scan_query() {
      let endpoint = std::env::var("OSSGALLEY_ENDPOINT")
          .expect("OSSGALLEY_ENDPOINT must be set");
      let bucket = std::env::var("OSSGALLEY_TEST_BUCKET")
              .unwrap_or_else(|_| format!("ossgalley-test-{}", Uuid::new_v4()));

      // 创建独立测试 bucket
      let s3 = create_s3_client_from_env().await;
      s3.create_bucket(&bucket).await.unwrap();

      // 使用 defer 模式确保清理
      let _cleanup = Defer::new(|| {
          let s3 = s3.clone();
          async move {
              // 清空并删除 bucket
              s3.delete_objects(&bucket, list_all_keys(&s3, &bucket).await).await.ok();
              s3.delete_bucket(&bucket).await.ok();
          }
      });

      // 执行测试
      let output = Command::new("ossgalley")
          .args(["init", &bucket, "test-host"])
          .env("OSSGALLEY_ENDPOINT", &endpoint)
          .env("OSSGALLEY_ACCESS_KEY", &std::env::var("OSSGALLEY_ACCESS_KEY").unwrap())
          .env("OSSGALLEY_SECRET_KEY", &std::env::var("OSSGALLEY_SECRET_KEY").unwrap())
          .output().unwrap();
      assert!(output.status.success());
      // ... 后续步骤
      // _cleanup 在此 drop，自动清理
  }
```

##### 模糊测试指标

```
通过条件:
  - 随机字节输入不导致任何 panic
  - 即使输入是损坏的 JPEG、空字节、极长字符串、Unicode 乱码

量化指标:
  - 数量: 2-3 个 fuzz target
  - 覆盖:
    - EXIF 提取器：任意字节不 panic
    - ObjectKey 构造：任意字符串不 panic
  - 执行时间: 每个 target 至少 60 秒（CI 每晚）
  - 覆盖率: 使用 SanitizerCoverage 追踪代码路径

失败后果:
  - 发现 panic → 记录崩溃输入（.crash 文件）
  - 开发人员必须修复并添加对应的单元测试
  - 不阻塞 PR，但次日必须修复

示例:
  // 发现崩溃后：
  // $ ls fuzz/artifacts/exif_extractor/
  // crash-0x1a2b3c4d  (导致 panic 的输入)
  //
  // 修复步骤：
  // 1. 将崩溃输入作为单元测试用例
  // 2. 修复提取器逻辑
  // 3. 验证新测试通过
  // 4. 重新运行 fuzz 确认无新崩溃
```

##### 覆盖率指标

```
通过条件:
  - 代码行覆盖率 > 80%（整体项目）
  - 核心模块覆盖率 > 90%（core 库）
  - CLI/Web 层 > 60%（薄包装层，逻辑少）

量化指标:
  - 工具: cargo tarpaulin
  - 报告: Cobertura XML 格式，上传到 Codecov
  - 阈值:
    - 整体: > 80%（警告）/ > 90%（目标）
    - core/src/types: 100%
    - core/src/classify: 100%
    - core/src/view: > 90%
    - core/src/scan: > 85%
    - core/src/lock: 100%
    - cli/src: > 70%
    - web/src: > 60%

失败后果:
  - < 80% → 警告，不阻塞 PR
  - < 60% → 阻塞 PR（严重不足）
  - 连续 3 次下降 → 自动创建 GitHub Issue

不应被覆盖率误导的代码:
  - 模板代码（HTML 模板不计算覆盖率）
  - 测试辅助代码（tests/common 不计算）
  - 宏展开代码
```

### 8. 例外：用户可扩展的部分使用 String

```rust
// 分类规则扩展名        → String（用户可加自定义扩展名）
// 标签名称              → String（用户可创建任意标签）
// 元数据键名            → String（提取器返回的键名）
// 自定义命名空间        → MetadataNamespace::Custom(String)

// 原则：DB 中用 String（可扩展），业务逻辑层用枚举（编译保证）
// 从 DB 读出 String 后在数据边界转换为枚举，转换失败返回明确错误
```

### 违反类型系统的后果

| 违反 | 结果 | 发现时机 |
|------|------|---------|
| bucket 和 key 传反 | 编译错误 | 编译时 |
| 无锁时释放锁 | 编译错误 | 编译时 |
| file_type 拼写错误 | 编译错误 | 编译时 |
| 未处理 Result | 编译警告（可配置为错误） | 编译时 |
| 无效 bucket 名称 | Err(InvalidBucket) | 运行时，构造时 |
| 空 key 做 S3 请求 | Err(InvalidKey) | 运行时，构造时 |
| DB 中未知枚举值 | Err(Database) | 运行时，反序列化时 |

## 副作用管理：类型系统必须暴露所有副作用

### 原则

**所有副作用必须在类型签名中可见。不可能从类型签名中隐藏一个 IO 操作。**

### 1. 纯查询与可能 IO 通过类型隔离

```rust
// ── LocalView：类型系统保证不持有 S3 客户端 ──
// 没有 S3 字段 → 编译器保证不可能触发远程调用
pub struct LocalView {
    db: DbPool,
    // 没有 Arc<dyn S3Client>！
}

impl LocalView {
    // 纯本地 DB 查询，零 OSS 请求
    pub async fn ls(&self, path: &Path, opts: &LsOptions) -> Result<LsResult>;
    pub async fn tree(&self, path: &Path, depth: u32) -> Result<TreeNode>;
    pub async fn stat(&self, path: &Path) -> Result<StatResult>;
    pub async fn search(&self, query: &str, opts: &SearchOptions) -> Result<Vec<FileEntry>>;
    pub async fn timeline(&self, opts: &TimelineOptions) -> Result<Vec<TimelineBucket>>;
    pub async fn tags(&self) -> Result<Vec<TagEntry>>;
    pub async fn duplicates(&self, opts: &DuplicatesOptions) -> Result<Vec<DuplicateGroup>>;
    pub async fn file_info(&self, key: &ObjectKey) -> Result<FileInfo>;
    // 没有 thumbnail() — 它需要 S3，在此类型上不存在
    // 没有 metadata() — 懒加载需要 S3，在此类型上不存在
}

// ── RemoteView：显式持有 S3，函数名以 fetch_ 前缀编码副作用 ──
// 调用者看到 fetch_ 就意识到：这个函数可能慢，可能触发远程 IO
pub struct RemoteView {
    db: DbPool,
    s3: Arc<dyn S3Client>,
    bucket: BucketName,
}

impl RemoteView {
    /// ⚠️ 可能从 OSS 下载完整文件（如果本地缓存未命中）
    /// 可能触发: 1 次 GET（完整文件）
    pub async fn fetch_thumbnail(&self, key: &ObjectKey) -> Result<Option<Vec<u8>>>;

    /// ⚠️ 可能从 OSS Range 请求（如果本地缓存未命中）
    /// 可能触发: 1 次 GET（Range 64KB）
    pub async fn fetch_metadata(&self, key: &ObjectKey, ns: &MetadataNamespace) -> Result<Vec<MetadataItem>>;

    /// ⚠️ 从 OSS 下载完整文件，流式返回
    /// 可能触发: 1 次 GET（完整文件）
    pub async fn fetch_file(&self, key: &ObjectKey) -> Result<impl Stream<Item = Result<Bytes, SdkError>>>;

    /// 检查本地缓存中是否有缩略图（纯查询，不触发远程）
    /// 注：这是纯查询，放这里是因为和 fetch_thumbnail 逻辑相关
    pub fn has_thumbnail_cached(&self, key: &ObjectKey) -> bool;
}

// ── 在 Web handler 中，类型系统区分两种操作 ──
async fn handler_browse(local: &LocalView) -> Result<Html> {
    let files = local.ls(path).await?;          // ✅ 编译器知道纯查询
    // local.fetch_thumbnail(key).await?;       // ❌ 编译错误！LocalView 没有这个方法
}

async fn handler_thumbnail(remote: &RemoteView) -> Result<impl IntoResponse> {
    let thumb = remote.fetch_thumbnail(key).await?;  // ✅ 显式表明可能 IO
}
```

### 2. ensure_db 拆分为检查 + 执行，编译器保证调用者看到所有副作用

```rust
// ── 第一步：纯检查，最多 1 次 HEAD ──
pub async fn check_db_status(
    cache_path: &Path,
    s3: &dyn S3Client,
    host: &HostIdentifier,
) -> Result<DbStatus>;

/// 检查结果，不带任何执行意图
pub enum DbStatus {
    /// 本地缓存与 OSS 版本一致，可直接使用
    LocalCacheUpToDate,
    /// OSS 上有更新版本，需要下载
    RemoteNewer { remote_modified: DateTime<Utc> },
    /// OSS 上有 DB，本地无缓存，需要下载
    RemoteOnly,
    /// OSS 和本地都无 DB
    None,
}

// ── 第二步：调用者根据 status 选择 action ──
// 每个 action 的副作用都在类型中用文档显式标注

/// 根据 DbStatus 选择执行的操作，每种操作有明确的副作用清单
pub enum DbAction {
    /// 纯本地，零 OSS 请求
    UseLocal,
    /// 1 次 GET 请求，下载 DB 文件
    DownloadFromOss,
    /// 触发全量扫描：acquire lock + N List pages + N Range requests + PUT DB + release lock
    /// 可能写 OSS，可能耗时数分钟
    FullScanAndUpload,
    /// 报错退出，不触发任何 OSS 操作
    Abort(String),
}

// 调用者必须显式 match 所有分支：
fn decide_action(status: DbStatus, readonly: bool) -> DbAction {
    match status {
        DbStatus::LocalCacheUpToDate => DbAction::UseLocal,
        DbStatus::RemoteNewer { .. } => DbAction::DownloadFromOss,
        DbStatus::RemoteOnly => DbAction::DownloadFromOss,
        DbStatus::None if !readonly => DbAction::FullScanAndUpload,
        DbStatus::None => DbAction::Abort("No database found. Run 'ossgalley scan <host>' first.".into()),
    }
    // ↑ 编译器保证：如果未来新增 DbStatus 变体，此处编译错误
}
```

### 3. 锁必须显式释放，通过类型系统强制

```rust
// ── LockGuard：#[must_use] 保证不会被忽略 ──
#[must_use = "LockGuard must be explicitly released via .release()"]
pub struct LockGuard {
    s3: Arc<dyn S3Client>,
    bucket: BucketName,
    lock_key: ObjectKey,
    // 没有 Drop 中释放锁的逻辑！
    // 锁的释放必须由调用者显式调用 .release() 完成
}

impl LockGuard {
    /// 释放锁并消耗 guard
    /// 此函数消耗 self，调用后 guard 不可再用
    pub async fn release(self) -> Result<()> {
        self.s3.delete_object(&self.bucket, &self.lock_key).await?;
        Ok(())
    }

    /// 续约锁（需要 &mut 引用，表明可能修改锁状态）
    pub async fn renew(&mut self) -> Result<()> {
        // PUT 更新租约时间（重置 expires_at 为当前时间 + 租期）
        self.s3.put_object(&self.bucket, &self.lock_key, &self.serialize_lease()).await?;
        Ok(())
    }
}

// 使用：编译器检查所有路径
async fn scan_with_lock(...) -> Result<ScanResult> {
    let guard = acquire_lock(&s3, &bucket, &key).await?;
    // ↑ 如果忘记处理 guard，编译器警告：
    // warning: unused return value of `LockGuard` that must be used

    // ... 扫描过程 ...

    // 必须显式释放：
    guard.release().await?;
    // 释放后 guard 不可再用（所有权转移）
    // guard.release().await?;  // ❌ 编译错误：use of moved value

    Ok(result)
}
```

### 4. 禁止 panic，所有错误路径必须通过 Result 传递

```rust
// ── 每个 crate 的 lib.rs 顶部必须包含以下配置 ──
#![forbid(unsafe_code)]
#![deny(unreachable_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::let_underscore_must_use)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]

// 上述配置的含义：
// forbit(unsafe_code)           — 禁止 unsafe 代码
// deny(unreachable_code)        — 不可达代码视为错误
// deny(unused_must_use)         — #[must_use] 的值必须被使用
// deny(clippy::unwrap_used)     — 禁止 .unwrap()，必须用 ?
// deny(clippy::expect_used)     — 禁止 .expect()，必须用 ?
// deny(clippy::indexing_slicing) — 禁止 arr[i] 索引，必须用 .first()/.get()
// deny(clippy::panic)            — 禁止 panic!() 宏
// deny(clippy::todo)             — 禁止 todo!() 宏
// deny(clippy::unimplemented)    — 禁止 unimplemented!() 宏
// deny(clippy::let_underscore_must_use) — 禁止 let _ = must_use_value
// deny(clippy::await_holding_lock)     — 禁止跨 await 持有锁
// deny(clippy::missing_errors_doc)     — 要求文档标注错误条件
// deny(clippy::missing_panics_doc)     — 要求文档标注 panic 条件
```

### 5. 所有可能失败的操作必须显式处理

```rust
// ❌ 禁止的写法：
let key = obj.key().unwrap();                    // clippy::unwrap_used
let first = arr[0];                              // clippy::indexing_slicing
let val = s.parse::<i32>().unwrap();             // clippy::unwrap_used
let _ = guard.release();                         // clippy::let_underscore_must_use

// ✅ 必须的写法：
let key = obj.key().ok_or(OssgalleyError::MissingField("key"))?;
let first = arr.first().ok_or(OssgalleyError::EmptyCollection)?;
let val = s.parse::<i32>().map_err(|e| OssgalleyError::ParseError(e))?;
guard.release().await?;  // Result 必须处理
```

### 6. DB 操作两阶段写入，确保元数据提取失败不留下空洞

```rust
// ── files 表增加 metadata_state 列 ──
pub enum MetadataState {
    /// 文件刚记录，尚未尝试提取元数据
    Pending,
    /// 元数据提取成功完成
    Complete,
    /// 部分提取器成功，部分失败
    Partial,
    /// 提取失败，下次扫描重试
    Failed,
}

// ── 扫描时的两阶段写入 ──
async fn scan_file(s3: &dyn S3Client, db: &DbPool, key: &str, etag: &str) -> Result<()> {
    // 阶段 1：先写入基础信息，标记 metadata_state = Pending
    db.upsert_file_basic(key, etag, size, MetadataState::Pending).await?;

    // 阶段 2：尝试提取元数据
    match extract_metadata(s3, key, etag).await {
        Ok(items) => {
            db.insert_metadata(key, &items).await?;
            db.update_metadata_state(key, MetadataState::Complete).await?;
        }
        Err(e) => {
            // 记录失败，下次扫描根据 ETag 变化判断是否重试
            db.update_metadata_state(key, MetadataState::Failed).await?;
            tracing::warn!("metadata extraction failed for {key}: {e}");
            // 不返回错误！扫描继续，不会因一个文件失败而中断全部
        }
    }
    Ok(())
}
```

### 7. 命名约定：函数名编码副作用

```rust
// 前缀约定表：
// 无前缀  — 纯本地操作，零 OSS 请求，零写入
// fetch_  — 可能触发远程 GET/HEAD，但不会写入 OSS
// write_  — 可能写入本地 DB
// upload_ — 可能 PUT 到 OSS
// async fn thumbnail(...)           // ❌ 不清晰
// async fn fetch_thumbnail(...)     // ✅ 可能远程 GET
// async fn write_metadata(...)      // ✅ 可能写入本地 DB
// async fn upload_db(...)           // ✅ 可能 PUT 到 OSS
```

### 副作用类型系统违反后果

| 违反 | 结果 | 发现时机 |
|------|------|---------|
| LocalView 中持有 S3 引用 | 编译错误（类型定义限制） | 编译时 |
| 未处理 LockGuard | 编译警告（deny 后为错误） | 编译时 |
| 使用 `.unwrap()` | 编译错误（clippy deny） | 编译时 |
| 使用 `arr[i]` 索引 | 编译错误（clippy deny） | 编译时 |
| 使用 `panic!()` | 编译错误（clippy deny） | 编译时 |
| 跨 await 持有锁 | 编译错误（clippy deny） | 编译时 |
| 忽略 Result 返回值 | 编译警告（deny 后为错误） | 编译时 |
| 函数缺少错误文档 | 编译警告（clippy deny） | 编译时 |

## 设计中采用的约束

- 聚类可扩展：`classification_rules` 表 + `extractor_rules` 表支持用户自定义
- 元数据解耦：`metadata` 表采用 namespace + key-value 结构，支持任意提取器
- UI 不泄漏到数据层：`tags` 表不包含颜色、图标等展示属性
- 版本统一：`scan_metadata.db_schema_version` 管理整个 DB 的 Schema 版本
- 部分下载标记：`metadata.partial` 标记来自 Range 请求的不完整数据

## SDK 优化后的关键流程

### 扫描流程（优化后）

基于 `aws-sdk-s3` v1.138.1 的 SDK 能力，优化后的扫描流程将 ListObjects 和 HEAD 请求合并，并利用 SDK 内置 Paginator 简化代码：

```
扫描流程（全量）：
  1. acquire lock (PUT with If-None-Match: *)
  2. 创建临时 DB
  3. ListObjectsV2 分页遍历（使用 Paginator）
     ├─ 每页返回 key + e_tag + size + last_modified
     └─ 无需额外 HEAD 请求
  4. 与 DB 中已有 ETag 对比
     ├─ 新文件 → INSERT
     ├─ ETag 变化 → UPDATE
     └─ 缺失文件 → 标记 is_deleted=1
  5. 按需提取元数据（Range 请求）
  6. 更新 scan_metadata
  7. PUT ossgallery.db → OSS
  8. release lock
```

### 核心代码模式

```rust
// ── 扫描循环 ──
async fn scan_host(
    client: &Client,
    db: &DbPool,
    bucket: &BucketName,
    prefix: &ObjectKey,
    concurrency: &ConcurrencyLimiter,
) -> Result<ScanResult> {
    let mut paginator = client
        .list_objects_v2()
        .bucket(bucket.as_str())
        .prefix(prefix.as_str())
        .into_paginator()
        .page_size(1000);

    let mut pages = paginator.send();
    while let Some(page) = pages.next().await {
        let page = page?;
        let contents = page.contents().ok_or(OssgalleyError::MissingField("contents"))?;
        for obj in contents {
            let key = obj.key().ok_or(OssgalleyError::MissingField("key"))?;
            let etag = obj.e_tag().ok_or(OssgalleyError::MissingField("etag"))?;
            let size = obj.size().ok_or(OssgalleyError::MissingField("size"))?;
            // last_modified 可选跳过

            if db.is_new_or_changed(key, etag).await? {
                concurrency.execute(async {
                    db.upsert_file(key, etag, size, MetadataState::Pending).await
                }).await?;
            }
        }
    }
    Ok(ScanResult { ... })
}

// ── 租约锁（原子操作） ──
async fn acquire_lock(
    client: &Client,
    bucket: &str,
    lock_key: &str,
    lease: &LockLease,
) -> Result<bool> {
    let body = serde_json::to_string(lease)?;
    let result = client
        .put_object()
        .bucket(bucket)
        .key(lock_key)
        .body(body.into())
        .if_none_match("*")  // 仅当锁不存在时创建
        .send()
        .await;

    match result {
        Ok(_) => Ok(true),          // 锁获取成功
        Err(SdkError::ServiceError(e))
            if e.err().is_precondition_failed() => Ok(false), // 锁被占用
        Err(e) => Err(e.into()),
    }
}

// ── EXIF 提取（Range + Conditional GET） ──
async fn extract_exif(
    client: &Client,
    bucket: &str,
    key: &str,
    cached_etag: &str,
) -> Result<Vec<MetadataItem>> {
    let response = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .range("bytes=0-65536")    // 仅下载前 64KB
        .if_none_match(cached_etag) // ETag 未变则跳过
        .send()
        .await;

    let data = match response {
        Ok(resp) => resp.body.collect().await?.to_vec(),
        Err(SdkError::ServiceError(e))
            if e.err().is_not_modified() => return Ok(vec![]), // 304 未修改
        Err(e) => return Err(e.into()),
    };

    // 解析 EXIF
    let reader = exif::Reader::new();
    let exif = reader.read_from_container(&mut std::io::Cursor::new(&data))?;
    // 提取字段...
}

// ── 批量删除（可选场景） ──
async fn batch_delete(
    client: &Client,
    bucket: &str,
    keys: &[String],
) -> Result<()> {
    let objects = keys.iter()
        .map(|k| ObjectIdentifier::builder().key(k).build())
        .collect::<Result<Vec<_>>>()?;
    let delete = Delete::builder()
        .set_objects(Some(objects))
        .build();

    client.delete_objects()
        .bucket(bucket)
        .delete(delete)
        .send()
        .await?;
    Ok(())
}
```

### 优化对比

| 操作 | 优化前 | 优化后 | 节省 |
|------|--------|--------|------|
| 扫描 1000 文件 | 1000 List + 1000 HEAD | 1-2 次 List 分页 | 1000 次 S3 请求 |
| 扫描时 ETag 获取 | 需要额外 HEAD | ListObjectsV2 直接返回 | 无额外请求 |
| 锁创建 | 手动 GET + 条件 PUT | 单次 PUT If-None-Match | 1 次请求 |
| EXIF 提取 | 全量下载 10MB | Range 64KB + 条件 GET | 99%+ 流量 |
| 批量删除 | N 次 DELETE | 1 次 DeleteObjects | N-1 次请求 |
| 分页逻辑 | 手动 ContinuationToken | `into_paginator()` | 代码量减少 50% |

### 关键发现

- `ListObjectsV2` 返回的 `Object` 结构体包含 `key`、`e_tag`、`size`、`last_modified`，**足以完成变更检测，无需 HEAD 请求**
- `PutObjectInput` 支持 `if_none_match("*")` 实现原子锁创建，**无需先 GET 再 PUT**
- `GetObjectInput` 支持 `if_none_match(etag)` 条件 GET，**ETag 未变时返回 304 Not Modified，零流量消耗**
- SDK 内置 Paginator，**无需手动处理 ContinuationToken**

## 元数据提取策略（混合策略）

采用混合策略，不同元数据按不同策略提取，平衡扫描速度和数据可用性：

| 元数据类型 | 提取时机 | 成本 | 存储位置 | 理由 |
|-----------|---------|------|---------|------|
| 文件类型分类 | 扫描时，本地 | 零（扩展名匹配） | files.file_type | 无 OSS 请求 |
| EXIF 元数据 | 扫描时提取 | 低（64KB Range） | metadata 表 | 性价比高，Range 请求仅 0.6% 流量 |
| MP4 元数据 | 扫描时提取 | 低（128KB Range） | metadata 表 | 同上 |
| 音频元数据 | 扫描时提取 | 低（128KB Range） | metadata 表 | 同上 |
| 缩略图 | 消费者按需 | 高（完整下载） | thumbnails 表 | 懒加载，仅查看时生成 |
| 完整文件 | 用户主动下载 | 高（完整下载） | 无缓存 | 仅在用户点击下载时获取 |

### 扫描阶段的元数据提取

```
scan 阶段 4: 元数据提取
  ├─ 参数控制:
  │   ├─ 默认: 提取 EXIF + MP4 + 音频元数据（Range 请求）
  │   ├─ --no-metadata: 跳过全部元数据提取
  │   └─ --with-thumbnails: 额外生成缩略图（昂贵，需完整下载）
  │
  ├─ 提取流程:
  │   for each 新文件 or ETag 变化的文件:
  │     查 extractor_rules → 找到匹配的提取器
  │     for each 提取器:
  │       Range 请求（64KB / 128KB）
  │       提取器解析 → Vec<MetadataItem>
  │       INSERT INTO metadata (..., partial=1)
  │
  └─ 上传到 OSS 的 DB 包含:
      ├─ files 表（完整文件清单）
      ├─ metadata 表（EXIF/MP4/音频，partial 标记）
      ├─ thumbnails 表（如果 --with-thumbnails，否则为空）
      └─ scan_metadata（扫描信息）
```

### 消费者的元数据获取

```
消费者打开文件详情:
  ├─ 检查 DB 中 metadata 表是否有此文件的 namespace
  │   ├─ 有 → 直接显示（零 OSS 请求）
  │   └─ 无 → 懒加载:
  │       ├─ Range 请求获取头部
  │       ├─ 提取器解析
  │       └─ 写入本地 DB 缓存（不写回 OSS）

消费者查看缩略图:
  ├─ 检查 DB 中 thumbnails 表
  │   ├─ 有 → 直接返回 BLOB（零 OSS 请求）
  │   └─ 无 → 懒加载:
  │       ├─ GET 完整文件（昂贵）
  │       ├─ 生成缩略图
  │       ├─ 写入本地 DB 缓存（不写回 OSS）
  │       └─ 返回 BLOB
```

## 客户端 DB 就绪逻辑

### 核心决策树

所有客户端命令（view、serve、db 等）启动时，都经过同一个 `ensure_db()` 决策函数：

```
                    ┌──────────────┐
                    │  命令启动     │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ --readonly?  │
                    └──┬───────┬───┘
                   YES │       │ NO
                 ┌─────▼──┐    │
                 │ 只读路径 │    │
                 │ (报错或  │    │
                 │ 用本地 ) │    │
                 └────────┘    │
                        ┌──────▼───────┐
                        │ 本地有DB缓存? │
                        └──┬───────┬───┘
                       YES │       │ NO
                    ┌───────▼──┐   │
                    │ 对比OSS   │   │
                    │ DB版本    │   │
                    └──┬───┬───┘   │
                   SAME│   │NEWER  │
                  ┌────▼┐ ┌─▼───┐  │
                  │用本  │ │下载  │  │
                  │地缓存│ │新DB  │  │
                  └─────┘ └─────┘  │
                             ┌─────▼──────┐
                             │ OSS上有DB? │
                             └──┬──────┬──┘
                            YES │      │ NO
                          ┌─────▼┐    ┌─▼───────┐
                          │下载DB │    │自动扫描  │
                          └──────┘    │(acquire  │
                                      │ lock +   │
                                      │ full scan│
                                      │ + upload)│
                                      └──────────┘
```

### 场景清单

| 场景 | 命令 | OSS 请求 | 数据流量 | 写 OSS |
|------|------|---------|---------|--------|
| 1. 消费者，本地缓存最新 | `view` | 1 HEAD | 0 | 否 |
| 2. 消费者，本地缓存过期 | `view` | 1 HEAD + 1 GET | DB 文件大小 | 否 |
| 3. 消费者，首次，OSS 有 DB | `view` | 1 HEAD + 1 GET | DB 文件大小 | 否 |
| 4. 消费者，首次，OSS 无 DB | `view` | 1 HEAD + N List + 1 PUT DB + 1 PUT锁 + 1 DELETE锁 | 元数据 | 是（自动扫描） |
| 5. 只读，无 DB | `view --readonly` | 1 HEAD | 0 | 否（报错） |
| 6. 全量扫描 | `scan` | 1 PUT锁 + N List + 1 PUT DB + 1 DELETE锁 | 元数据 | 是 |
| 7. 增量扫描 | `scan --incremental` | 1 PUT锁 + 1 List + 1 PUT DB + 1 DELETE锁 | 元数据 | 是 |
| 8. 手动下载 | `db pull` | 1 GET | DB 文件大小 | 否 |
| 9. 手动上传 | `db push` | 1 PUT锁 + 1 PUT DB + 1 DELETE锁 | DB 文件大小 | 是 |
| 10. Web 启动 | `serve` | 同 view 场景 1-3 | 同 view | 否（除非手动触发 scan） |

## 资源限制

### 并发请求限制

所有 S3 操作必须通过 `Semaphore` 控制并发数，防止 OSS 限流和本地资源耗尽。

```rust
/// 并发控制，所有 S3 请求通过此结构体执行
#[derive(Clone)]
pub struct ConcurrencyLimiter {
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl ConcurrencyLimiter {
    /// 创建并发限制器
    /// max_concurrency: 最大并发 S3 请求数
    /// - 扫描阶段：默认 10（--concurrency 参数可调）
    /// - 元数据提取：默认 10
    /// - 缩略图生成：默认 3（下载完整文件，带宽消耗大）
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrency)),
        }
    }

    /// 执行 S3 请求，受并发限制
    /// 自动获取 Semaphore 许可，执行完毕释放
    pub async fn execute<F, T>(&self, f: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let _permit = self.semaphore.acquire().await?;
        f.await
    }
}

// 使用示例：
async fn scan_objects(
    s3: &Client,
    keys: &[ObjectKey],
    concurrency: &ConcurrencyLimiter,
) -> Result<Vec<ObjectInfo>> {
    let tasks: Vec<_> = keys.iter().map(|key| {
        concurrency.execute(async {
            s3.head_object()
                .bucket(b.as_str())
                .key(key.as_str())
                .send()
                .await
                .map_err(OssgalleyError::from)
        })
    }).collect();
    let results: Result<Vec<_>> = futures::future::join_all(tasks)
        .await
        .into_iter()
        .collect();
    results
}
```

### 内存限制

```rust
/// 缩略图缓存内存限制
pub struct ThumbnailCache {
    db: DbPool,
    max_cache_bytes: u64,           // 默认 500 MB
    current_bytes: Arc<AtomicU64>,  // 当前缓存大小
}

impl ThumbnailCache {
    /// 默认限制 500 MB
    pub fn new(db: DbPool) -> Self {
        Self {
            db,
            max_cache_bytes: 500 * 1024 * 1024,  // 500 MB
            current_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 缓存缩略图
    /// 如果超过上限，淘汰最旧的缓存项
    pub async fn cache(&self, key: &ObjectKey, data: &[u8]) -> Result<()> {
        let size = data.len() as u64;
        let current = self.current_bytes.fetch_add(size, Ordering::Acquire);
        if current + size > self.max_cache_bytes {
            self.evict_lru().await?;
        }
        self.db.insert_thumbnail(key, data).await?;
        Ok(())
    }

    /// 淘汰最久未访问的缓存，直到低于上限的 70%
    async fn evict_lru(&self) -> Result<()> {
        loop {
            let current = self.current_bytes.load(Ordering::Relaxed);
            if current < self.max_cache_bytes * 70 / 100 {
                break;
            }
            // 删除最早缓存的缩略图
            if let Some((key, key_size)) = self.db.get_oldest_thumbnail_key_and_size().await? {
                self.db.delete_thumbnail(&key).await?;
                self.current_bytes.fetch_sub(key_size as u64, Ordering::Release);
            } else {
                break;
            }
        }
        Ok(())
    }
}
```

## 输入验证边界

所有外部输入在进入系统边界时验证，之后不再重复验证。验证失败返回明确错误，不 panic。

```rust
// ── CLI 输入（clap 解析后立即验证） ──
pub struct ValidatedConfig {
    oss: OssConfig,          // 验证 endpoint URL、access key 格式、secret key 非空
    host: HostIdentifier,    // 验证 bucket/host-dir 格式
    db_path: PathBuf,        // 验证路径可写（如果不写入则不检查）
    options: CliOptions,     // 验证选项合法性
}

impl ValidatedConfig {
    /// 启动时调用，验证所有配置项。
    /// 收集所有错误后一次性报告，不逐个退出。
    pub fn from_raw(raw: RawCliInput) -> Result<Self, Vec<ConfigError>> {
        let mut errors = Vec::new();

        // 验证 OSS 配置
        let oss = match OssConfig::validate(&raw.endpoint, &raw.access_key, &raw.secret_key) {
            Ok(c) => c,
            Err(e) => { errors.push(e); return Err(errors); }
        };

        // 验证主机标识符
        let host = match HostIdentifier::parse(&raw.host) {
            Ok(h) => h,
            Err(e) => { errors.push(e); return Err(errors); }
        };

        if errors.is_empty() {
            Ok(Self { oss, host, db_path: raw.db_path, options: raw.options })
        } else {
            Err(errors)
        }
    }
}

// ── Web 输入（axum 提取器中验证） ──
pub struct BrowsePath(Vec<ObjectKey>);

impl<S> FromRequestParts<S> for BrowsePath
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or("");
        let path = extract_path_param(query)?;

        // 路径穿越防护
        if path.contains("..") || path.contains("//") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid_path", "detail": "Path must not contain '..' or '//'"}))
            ));
        }
        // 长度限制
        if path.len() > 1024 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "path_too_long", "detail": "Path must not exceed 1024 characters"}))
            ));
        }

        Ok(BrowsePath(vec![ObjectKey::new(&path)?]))
    }
}
```

## CI 流水线

```yaml
# .github/workflows/ci.yml

name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
  # 手动触发：真实 OSS 测试
  # 使用方法: gh workflow run ci.yml -f real-s3-test=true
  workflow_dispatch:
    inputs:
      real-s3-test:
        description: 'Run tests against real S3-compatible OSS'
        required: true
        default: false
        type: boolean
      oss-endpoint:
        description: 'OSS endpoint URL (e.g. https://s3.us-east-1.amazonaws.com)'
        required: true
        type: string
      oss-region:
        description: 'OSS region (e.g. us-east-1)'
        required: false
        default: 'us-east-1'
        type: string
      oss-access-key:
        description: 'OSS access key'
        required: true
        type: string
      oss-secret-key:
        description: 'OSS secret key'
        required: true
        type: string
      oss-test-bucket:
        description: 'OSS test bucket name (will be created and destroyed)'
        required: false
        default: 'ossgalley-ci-test'
        type: string

env:
  CARGO_TERM_COLOR: always

jobs:
  # ── 每次 PR 必须通过（< 5 分钟） ──
  quick:
    name: Quick checks
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: Format check
        run: cargo fmt --check

      - name: Clippy (deny warnings)
        run: cargo clippy --all-targets -- -D warnings
        env:
          RUSTFLAGS: "-D warnings"

      - name: Unit tests
        run: cargo test --lib

      - name: Doc tests
        run: cargo test --doc

      - name: Proptest (quick mode)
        run: cargo test --test proptest -- PROPTEST_CASES=100

      - name: Doc check
        run: cargo doc --no-deps --document-private-items

  # ── 集成测试（需要 MockS3 + sqlite，< 2 分钟） ──
  integration:
    name: Integration tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Run integration tests
        run: cargo test --test integration

  # ── 安全审计（每次 PR 运行） ──
  security:
    name: Security audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install cargo-audit
        run: cargo install cargo-audit

      - name: Audit dependencies
        run: cargo audit

      - name: Install cargo-deny
        run: cargo install cargo-deny

      - name: Check licenses
        run: cargo deny check licenses

  # ── 每晚运行（< 15 分钟） ──
  nightly:
    name: Nightly
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Start MinIO
        run: |
          docker run -d -p 9000:9000 \
            -e MINIO_ROOT_USER=s3oss \
            -e MINIO_ROOT_PASSWORD=s3oss1234 \
            minio/minio server /data
          sleep 3

      - name: E2E tests
        run: cargo test --test e2e -- --ignored
        env:
          OSSGALLEY_ENDPOINT: http://localhost:9000
          OSSGALLEY_ACCESS_KEY: s3oss
          OSSGALLEY_SECRET_KEY: s3oss1234

      - name: Fuzz tests (short)
        run: |
          cargo install cargo-fuzz
          cargo fuzz run exif_extractor -- -max_total_time=60
          cargo fuzz run object_key -- -max_total_time=30

      - name: Install cargo-tarpaulin
        run: cargo install cargo-tarpaulin

      - name: Coverage report
        run: cargo tarpaulin --out xml --ignore-tests

      - name: Upload to Codecov
        uses: codecov/codecov-action@v3
        with:
          file: cobertura.xml

  # ── 手动触发：真实 S3 兼容 OSS 测试（必须在 workflow_dispatch 中提供凭证） ──
  real-s3-test:
    name: Real OSS test
    if: github.event_name == 'workflow_dispatch' && inputs.real-s3-test == 'true'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Run real OSS tests
        run: cargo test --test real_s3 -- --ignored --test-threads=1
        env:
          OSSGALLEY_ENDPOINT: ${{ inputs.oss-endpoint }}
          OSSGALLEY_REGION: ${{ inputs.oss-region }}
          OSSGALLEY_ACCESS_KEY: ${{ inputs.oss-access-key }}
          OSSGALLEY_SECRET_KEY: ${{ inputs.oss-secret-key }}
          OSSGALLEY_TEST_BUCKET: ${{ inputs.oss-test-bucket }}

      - name: Notify result
        if: always()
        run: |
          if [ "${{ job.status }}" = "success" ]; then
            echo "✅ Real OSS test passed against ${{ inputs.oss-endpoint }}"
          else
            echo "❌ Real OSS test failed against ${{ inputs.oss-endpoint }}"
            echo "Check: ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}"
          fi
```

### CI 门禁规则

| 步骤 | 命令 | 门禁 | 触发 | 失败处理 |
|------|------|------|------|---------|
| 代码格式 | `cargo fmt --check` | 格式一致 | 每次 PR | 阻塞 |
| Lint | `cargo clippy -D warnings` | 零警告 | 每次 PR | 阻塞 |
| 文档 | `cargo doc --no-deps` | 零错误 | 每次 PR | 阻塞 |
| 单元测试 | `cargo test --lib` | 全部通过 | 每次 PR | 阻塞 |
| 文档测试 | `cargo test --doc` | 全部通过 | 每次 PR | 阻塞 |
| 属性测试 | `cargo test --test proptest` | 全部通过 | 每次 PR | 阻塞 |
| 集成测试 | `cargo test --test integration` | 全部通过 | 每次 PR | 阻塞 |
| 依赖审计 | `cargo audit` | 零已知漏洞 | 每次 PR | 阻塞 |
| 许可检查 | `cargo deny check` | 合规 | 每次 PR | 阻塞 |
| E2E 测试 | `cargo test --test e2e` | 全部通过 | 每晚 | 不阻塞（报告） |
| 模糊测试 | `cargo fuzz` | 零 panic | 每晚 | 不阻塞（报告） |
| 覆盖率 | `cargo tarpaulin` | > 80% | 每晚 | 不阻塞（警告） |
| 真实 OSS 测试 | `cargo test --test real_s3` | 全部通过 | 手动触发（workflow_dispatch） | 不阻塞，仅通知 |