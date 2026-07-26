# SeaORM 迁移实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将项目从 sqlx raw query 迁移到 SeaORM，解决 23 项类型安全违规，将 DB 字段自动映射到 Rust 枚举和 Newtype

**Architecture:** 引入 SeaORM Entity 定义层（14 个 Entity 文件） + 保留 Raw SQL 混合模式。Entity 定义自动映射类型，复杂聚合查询保留 SQL 字符串但绑定到 SeaORM 的 `QueryResult`。`models.rs` 逐步淘汰，每个模块迁移后独立编译。

**Tech Stack:** SeaORM 1.x (sqlx-sqlite + runtime-tokio-rustls + macros), sea-orm-migration

---

## 文件结构

```
crates/s3-gallery-core/src/
├── entity/                    # 新增：14 个 Entity 定义
│   ├── mod.rs
│   ├── file.rs, host_config.rs, metadata.rs
│   ├── tag.rs, file_tag.rs, thumbnail.rs
│   ├── classification_rule.rs, extractor_rule.rs
│   ├── scan_metadata.rs, dir_size.rs, scan_object.rs
│   ├── traffic_log.rs, traffic_file_log.rs, traffic_stats.rs
├── types.rs                   # 修改：加 DeriveActiveEnum + 桥接宏 + Prefix + S3Path
├── db/
│   ├── migrate.rs             # 新增：SeaORM Migration 替代 schema.rs
│   ├── pool.rs                # 修改：返回 DatabaseConnection
│   ├── schema.rs              # 删除（迁移到 migrate.rs）
│   └── models.rs              # 删除（Entity 替代，最后一 task 删除）
├── view/                      # 修改：全部改为 SeaORM 查询
│   ├── ls.rs, tree.rs, stat.rs, search.rs
│   ├── tags.rs, timeline.rs, timeline_gallery.rs
│   ├── duplicates.rs, export.rs, traffic.rs
├── scan/                      # 修改：SqlitePool → DatabaseConnection
├── s3/                        # 修改：SqlitePool → DatabaseConnection + Prefix 类型
├── lib.rs                     # 修改：添加 entity 模块
crates/s3-gallery-core/Cargo.toml  # 修改：添加 sea-orm 依赖
```

---

### Task 1: 添加 SeaORM 依赖 + 创建 Entity 模块

**Files:**
- Modify: `crates/s3-gallery-core/Cargo.toml`
- Create: `crates/s3-gallery-core/src/entity/mod.rs`
- Create: `crates/s3-gallery-core/src/entity/file.rs`
- Create: `crates/s3-gallery-core/src/entity/host_config.rs`
- Create: `crates/s3-gallery-core/src/entity/metadata.rs`
- Create: `crates/s3-gallery-core/src/entity/tag.rs`
- Create: `crates/s3-gallery-core/src/entity/file_tag.rs`
- Create: `crates/s3-gallery-core/src/entity/thumbnail.rs`
- Create: `crates/s3-gallery-core/src/entity/classification_rule.rs`
- Create: `crates/s3-gallery-core/src/entity/extractor_rule.rs`
- Create: `crates/s3-gallery-core/src/entity/scan_metadata.rs`
- Create: `crates/s3-gallery-core/src/entity/dir_size.rs`
- Create: `crates/s3-gallery-core/src/entity/scan_object.rs`
- Create: `crates/s3-gallery-core/src/entity/traffic_log.rs`
- Create: `crates/s3-gallery-core/src/entity/traffic_file_log.rs`
- Create: `crates/s3-gallery-core/src/entity/traffic_stats.rs`

- [ ] **Step 1: 添加 Cargo.toml 依赖**

```toml
# crates/s3-gallery-core/Cargo.toml — 在 [dependencies] 中添加
sea-orm = { version = "1", features = ["sqlx-sqlite", "runtime-tokio-rustls", "macros"] }
sea-orm-migration = { version = "1", features = ["sqlx-sqlite", "runtime-tokio-rustls"] }
```

- [ ] **Step 2: 创建 entity/mod.rs**

```rust
// crates/s3-gallery-core/src/entity/mod.rs
pub mod file;
pub mod host_config;
pub mod metadata;
pub mod tag;
pub mod file_tag;
pub mod thumbnail;
pub mod classification_rule;
pub mod extractor_rule;
pub mod scan_metadata;
pub mod dir_size;
pub mod scan_object;
pub mod traffic_log;
pub mod traffic_file_log;
pub mod traffic_stats;
```

- [ ] **Step 3: 创建 14 个 Entity 文件**

每个 Entity 文件内容见 spec 第 3 节。以下是关键 Entity 的完整内容：

**entity/file.rs:**
```rust
use sea_orm::entity::prelude::*;
use crate::types::{FileType, MetadataState, ObjectKey, Etag, FileSize, HostId};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    #[sea_orm(primary_key)]
    pub key: ObjectKey,
    pub etag: Etag,
    pub size: FileSize,
    pub last_modified: String,
    pub content_type: Option<String>,
    pub file_type: FileType,
    pub metadata_state: MetadataState,
    pub is_deleted: bool,
    pub effective_date: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::host_config::Entity",
        from = "Column::HostId",
        to = "super::host_config::Column::HostId"
    )]
    HostConfig,
    #[sea_orm(has_many = "super::file_tag::Entity")]
    FileTags,
    #[sea_orm(has_many = "super::metadata::Entity")]
    Metadata,
    #[sea_orm(has_many = "super::scan_object::Entity")]
    ScanObjects,
}

impl ActiveModelBehavior for ActiveModel {}
```

**entity/metadata.rs:**
```rust
use sea_orm::entity::prelude::*;
use crate::types::{ObjectKey, MetadataNamespace};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "metadata")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,
    #[sea_orm(primary_key)]
    pub namespace: MetadataNamespace,
    #[sea_orm(primary_key)]
    pub key: String,
    pub namespace_custom: Option<String>,
    pub value: String,
    pub extracted_at: String,
    pub partial: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileKey",
        to = "super::file::Column::Key"
    )]
    File,
}

impl ActiveModelBehavior for ActiveModel {}
```

**entity/tag.rs:**
```rust
use sea_orm::entity::prelude::*;
use crate::types::TagType;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tags")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub tag_id: i64,
    pub tag_name: String,
    pub tag_type: TagType,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::file_tag::Entity")]
    FileTags,
}

impl ActiveModelBehavior for ActiveModel {}
```

**entity/file_tag.rs:**
```rust
use sea_orm::entity::prelude::*;
use crate::types::ObjectKey;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,
    #[sea_orm(primary_key)]
    pub tag_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tag::Entity",
        from = "Column::TagId",
        to = "super::tag::Column::TagId"
    )]
    Tag,
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileKey",
        to = "super::file::Column::Key"
    )]
    File,
}

impl ActiveModelBehavior for ActiveModel {}
```

**entity/traffic_log.rs:**
```rust
use sea_orm::entity::prelude::*;
use crate::types::{HostId, S3Operation, Direction};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_log")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
    pub host_id: HostId,
    pub operation: S3Operation,
    pub business: String,
    pub direction: Direction,
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}

impl ActiveModelBehavior for ActiveModel {}
```

其余 9 个 Entity 文件按 spec 第 3 节的内容创建。每个文件包含 `DeriveEntityModel`、`DeriveRelation`（如果有关联）、`ActiveModelBehavior`。

- [ ] **Step 4: 在 lib.rs 中添加 entity 模块**

```rust
// crates/s3-gallery-core/src/lib.rs
pub mod entity;
```

- [ ] **Step 5: 验证编译**

```bash
cd /Users/macbook/Project/s3-gallery
cargo check 2>&1 | head -30
```
Expected: 编译错误，因为 types.rs 中的类型尚未实现 `TryGetable` 等 SeaORM trait。这是预期的，下一步修复。

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/Cargo.toml crates/s3-gallery-core/src/entity/ crates/s3-gallery-core/Cargo.lock
git commit -m "feat: add sea-orm dependency and 14 entity definitions"
```

---

### Task 2: types.rs 改造 — DeriveActiveEnum + 桥接宏 + Prefix + S3Path

**Files:**
- Modify: `crates/s3-gallery-core/src/types.rs`

- [ ] **Step 1: 添加 DeriveActiveEnum 到现有枚举**

将 `FileType`、`MetadataState`、`MetadataNamespace`、`FileCategory`、`SyncStatus`、`DbStatus`、`DbAction`、`SortField`、`SortOrder`、`ViewMode`、`ScanMode` 从手工 `Display`/`FromStr` 改为 `DeriveActiveEnum`。

**只改 `FileType`、`MetadataState`、`MetadataNamespace` 三个（其余的未存 DB，不需要 DeriveActiveEnum）：**

```rust
// 替换 FileType 的 derive 和 Display/FromStr 实现
#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    #[sea_orm(string_value = "jpeg")]    #[serde(rename = "jpeg")]    Jpeg,
    #[sea_orm(string_value = "png")]     #[serde(rename = "png")]     Png,
    // ... 全部 29 个变体
    #[sea_orm(string_value = "unknown")] #[serde(rename = "unknown")] Unknown,
}

// 删除手工 Display 和 FromStr 实现（DeriveActiveEnum 自动生成）
```

**删除 `MetadataState` 的 Display/FromStr：**
```rust
#[derive(Debug, Clone, Copy, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum MetadataState {
    #[sea_orm(string_value = "pending")]   Pending,
    #[sea_orm(string_value = "extracted")] Extracted,
    #[sea_orm(string_value = "failed")]    Failed,
}
// 删除 impl Display 和 impl FromStr 块
```

**删除 `MetadataNamespace` 的 Display/FromStr：**
```rust
#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum MetadataNamespace {
    #[sea_orm(string_value = "exif")]    Exif,
    #[sea_orm(string_value = "video")]   Video,
    #[sea_orm(string_value = "audio")]   Audio,
    #[sea_orm(string_value = "general")] General,
    #[sea_orm(string_value = "custom")]  Custom,
}
// 删除 impl Display 和 impl FromStr 块
```

**添加 `S3Operation`、`Direction`、`TagType`、`ThumbnailFormat` 四个新枚举（从 `traffic_recorder.rs` 移入）：**

```rust
// 在 types.rs 末尾添加
#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum S3Operation {
    #[sea_orm(string_value = "GetObject")]          GetObject,
    #[sea_orm(string_value = "GetObjectRange")]     GetObjectRange,
    #[sea_orm(string_value = "PutObject")]          PutObject,
    #[sea_orm(string_value = "PutObjectIfNoneMatch")] PutObjectIfNoneMatch,
    #[sea_orm(string_value = "ListObjects")]        ListObjects,
    #[sea_orm(string_value = "HeadObject")]         HeadObject,
    #[sea_orm(string_value = "DeleteObject")]       DeleteObject,
    #[sea_orm(string_value = "ObjectExists")]       ObjectExists,
}

#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[sea_orm(string_value = "download")] Download,
    #[sea_orm(string_value = "upload")]   Upload,
}

#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum TagType {
    #[sea_orm(string_value = "auto")]   Auto,
    #[sea_orm(string_value = "manual")] Manual,
}

#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum ThumbnailFormat {
    #[sea_orm(string_value = "jpeg")] Jpeg,
    #[sea_orm(string_value = "png")]  Png,
    #[sea_orm(string_value = "webp")] WebP,
}
```

- [ ] **Step 2: 添加 Newtype 桥接宏**

在 types.rs 顶部（或 `use` 语句之后）添加：

```rust
use sea_orm::TryGetable;

/// 桥接 Newtype 到 SeaORM 的 TryGetable 和 Value trait。
macro_rules! impl_sea_orm_value_for_newtype {
    ($ty:ty) => {
        impl sea_orm::TryGetable for $ty {
            fn try_get_by<I: sea_orm::ColIdx>(
                res: &sea_orm::DbBackendQueryResult, idx: I
            ) -> std::result::Result<Self, sea_orm::TryGetError> {
                let s: String = res.try_get_by::<String>(idx)?;
                s.parse().map_err(|e| sea_orm::TryGetError::DbErr(
                    sea_orm::DbErr::Custom(e.to_string())
                ))
            }
        }
        impl From<$ty> for sea_orm::Value {
            fn from(v: $ty) -> Self {
                sea_orm::Value::String(Some(Box::new(v.to_string())))
            }
        }
    };
}

impl_sea_orm_value_for_newtype!(ObjectKey);
impl_sea_orm_value_for_newtype!(Etag);
impl_sea_orm_value_for_newtype!(HostId);
impl_sea_orm_value_for_newtype!(FileExtension);
impl_sea_orm_value_for_newtype!(Prefix);

impl sea_orm::TryGetable for FileSize {
    fn try_get_by<I: sea_orm::ColIdx>(
        res: &sea_orm::DbBackendQueryResult, idx: I
    ) -> std::result::Result<Self, sea_orm::TryGetError> {
        let n: i64 = res.try_get_by::<i64>(idx)?;
        Ok(FileSize::new(n.max(0) as u64))
    }
}
impl From<FileSize> for sea_orm::Value {
    fn from(v: FileSize) -> Self {
        sea_orm::Value::BigInt(Some(v.as_u64() as i64))
    }
}
```

- [ ] **Step 3: 添加 Prefix 类型和 S3Path trait**

```rust
/// 统一的路径操作 trait。
pub trait S3Path: Sized {
    fn as_str(&self) -> &str;
    fn parent(&self) -> Option<Prefix>;
    fn last_segment(&self) -> Option<&str>;
    fn join_key(&self, name: &str) -> ObjectKey;
    fn join_dir(&self, name: &str) -> Prefix;
}

/// 目录前缀（空字符串或以 / 结尾）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Prefix(String);

impl Prefix {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.len() > 1024 {
            return Err(S3GalleryError::ValidationError(
                format!("prefix must be at most 1024 characters, got {}", s.len())
            ));
        }
        if !s.is_empty() && !s.ends_with('/') {
            return Err(S3GalleryError::ValidationError(
                "prefix must be empty or end with '/'".into()
            ));
        }
        Ok(Self(s))
    }
    pub fn as_str(&self) -> &str { &self.0 }
    pub fn is_root(&self) -> bool { self.0.is_empty() }
}

impl std::fmt::Display for Prefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for Prefix {
    type Err = S3GalleryError;
    fn from_str(s: &str) -> Result<Self> { Self::new(s) }
}

impl S3Path for Prefix {
    fn parent(&self) -> Option<Prefix> {
        if self.0.is_empty() { return None; }
        let trimmed = self.0.trim_end_matches('/');
        let pos = trimmed.rfind('/')?;
        Some(Prefix(trimmed[..=pos].to_string()))
    }
    fn last_segment(&self) -> Option<&str> {
        if self.0.is_empty() { return None; }
        let trimmed = self.0.trim_end_matches('/');
        let pos = trimmed.rfind('/')?;
        Some(&trimmed[pos + 1..])
    }
    fn join_key(&self, name: &str) -> ObjectKey {
        ObjectKey::new(format!("{}{}", self.0, name)).expect("valid key")
    }
    fn join_dir(&self, name: &str) -> Prefix {
        Prefix::new(format!("{}{}/", self.0, name)).expect("valid prefix")
    }
}

impl S3Path for ObjectKey {
    fn parent(&self) -> Option<Prefix> {
        let pos = self.0.rfind('/')?;
        Some(Prefix(self.0[..=pos].to_string()))
    }
    fn last_segment(&self) -> Option<&str> { self.file_name() }
    fn join_key(&self, name: &str) -> ObjectKey {
        ObjectKey::new(format!("{}{}", self.0, name)).expect("valid key")
    }
    fn join_dir(&self, name: &str) -> Prefix {
        Prefix::new(format!("{}{}/", self.0, name)).expect("valid prefix")
    }
}
```

- [ ] **Step 4: 更新测试**

删除 `metadata_namespace_from_str_invalid` 测试（因为 `DeriveActiveEnum` 现在接受 `"custom"` 值）。

```rust
// 删除此测试：
#[test]
fn metadata_namespace_from_str_invalid() {
    let err = "custom".parse::<MetadataNamespace>().unwrap_err();
    assert!(matches!(err, S3GalleryError::ValidationError(_)));
}
```

- [ ] **Step 5: 删除 `traffic_recorder.rs` 中的 `S3Operation` 枚举定义**

打开 `crates/s3-gallery-core/src/s3/traffic_recorder.rs`，删除 `S3Operation` 枚举定义（约 30 行），改为从 `crate::types` 导入。

```rust
// 在 traffic_recorder.rs 顶部添加
use crate::types::S3Operation;

// 删除 pub enum S3Operation { ... } 块
// 删除 impl S3Operation { pub fn from_request(...) ... } 块
// 删除 impl Display for S3Operation { ... } 块
```

- [ ] **Step 6: 验证编译**

```bash
cargo check 2>&1 | head -20
```
Expected: 编译错误集中在 `traffic_recorder.rs` 中删除的 `S3Operation` 引用，以及 view 模块中使用了旧的 `SqlitePool` 的导入。这是预期的。

- [ ] **Step 7: Commit**

```bash
git add crates/s3-gallery-core/src/types.rs crates/s3-gallery-core/src/s3/traffic_recorder.rs
git commit -m "refactor: add DeriveActiveEnum, Newtype bridges, Prefix type, S3Path trait"
```

---

### Task 3: DB 迁移系统 — migrate.rs + pool.rs

**Files:**
- Create: `crates/s3-gallery-core/src/db/migrate.rs`
- Modify: `crates/s3-gallery-core/src/db/pool.rs`
- Modify: `crates/s3-gallery-core/src/db/mod.rs`

- [ ] **Step 1: 创建 db/migrate.rs**

```rust
// crates/s3-gallery-core/src/db/migrate.rs
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct InitialMigration;

#[async_trait::async_trait]
impl MigrationTrait for InitialMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // host_config
        manager.create_table(
            Table::create()
                .table(host_config::Entity)
                .if_not_exists()
                .col(ColumnDef::new(host_config::Column::HostId).text().primary_key().not_null())
                .col(ColumnDef::new(host_config::Column::HostName).text().not_null())
                .col(ColumnDef::new(host_config::Column::HostType).text().not_null().default("unknown"))
                .col(ColumnDef::new(host_config::Column::Description).text().not_null().default(""))
                .col(ColumnDef::new(host_config::Column::CreatedAt).text().not_null())
                .to_owned()
        ).await?;

        // files
        manager.create_table(
            Table::create()
                .table(file::Entity)
                .if_not_exists()
                .col(ColumnDef::new(file::Column::HostId).text().not_null())
                .col(ColumnDef::new(file::Column::Key).text().not_null())
                .col(ColumnDef::new(file::Column::Etag).text().not_null())
                .col(ColumnDef::new(file::Column::Size).big_integer().not_null())
                .col(ColumnDef::new(file::Column::LastModified).text().not_null())
                .col(ColumnDef::new(file::Column::ContentType).text())
                .col(ColumnDef::new(file::Column::FileType).text().not_null())
                .col(ColumnDef::new(file::Column::MetadataState).text().not_null().default("pending"))
                .col(ColumnDef::new(file::Column::IsDeleted).boolean().not_null().default(false))
                .col(ColumnDef::new(file::Column::EffectiveDate).text().not_null().default(""))
                .primary_key(Index::create().col(file::Column::HostId).col(file::Column::Key))
                .to_owned()
        ).await?;

        // metadata
        manager.create_table(
            Table::create()
                .table(metadata::Entity)
                .if_not_exists()
                .col(ColumnDef::new(metadata::Column::FileKey).text().not_null())
                .col(ColumnDef::new(metadata::Column::Namespace).text().not_null())
                .col(ColumnDef::new(metadata::Column::NamespaceCustom).text())
                .col(ColumnDef::new(metadata::Column::Key).text().not_null())
                .col(ColumnDef::new(metadata::Column::Value).text().not_null())
                .col(ColumnDef::new(metadata::Column::ExtractedAt).text().not_null())
                .col(ColumnDef::new(metadata::Column::Partial).boolean().not_null().default(false))
                .primary_key(Index::create().col(metadata::Column::FileKey).col(metadata::Column::Namespace).col(metadata::Column::Key))
                .to_owned()
        ).await?;

        // classification_rules
        manager.create_table(
            Table::create()
                .table(classification_rule::Entity)
                .if_not_exists()
                .col(ColumnDef::new(classification_rule::Column::Extension).text().primary_key().not_null())
                .col(ColumnDef::new(classification_rule::Column::FileType).text().not_null())
                .col(ColumnDef::new(classification_rule::Column::Priority).integer().not_null().default(0))
                .col(ColumnDef::new(classification_rule::Column::Description).text())
                .to_owned()
        ).await?;

        // extractor_rules
        manager.create_table(
            Table::create()
                .table(extractor_rule::Entity)
                .if_not_exists()
                .col(ColumnDef::new(extractor_rule::Column::Extension).text().not_null())
                .col(ColumnDef::new(extractor_rule::Column::ExtractorName).text().not_null())
                .col(ColumnDef::new(extractor_rule::Column::Priority).integer().not_null().default(0))
                .primary_key(Index::create().col(extractor_rule::Column::Extension).col(extractor_rule::Column::ExtractorName))
                .to_owned()
        ).await?;

        // thumbnails
        manager.create_table(
            Table::create()
                .table(thumbnail::Entity)
                .if_not_exists()
                .col(ColumnDef::new(thumbnail::Column::FileKey).text().primary_key().not_null())
                .col(ColumnDef::new(thumbnail::Column::Data).blob().not_null())
                .col(ColumnDef::new(thumbnail::Column::Format).text().not_null().default("jpeg"))
                .col(ColumnDef::new(thumbnail::Column::Width).integer())
                .col(ColumnDef::new(thumbnail::Column::Height).integer())
                .col(ColumnDef::new(thumbnail::Column::CachedAt).text().not_null())
                .to_owned()
        ).await?;

        // tags
        manager.create_table(
            Table::create()
                .table(tag::Entity)
                .if_not_exists()
                .col(ColumnDef::new(tag::Column::TagId).integer().auto_increment().primary_key().not_null())
                .col(ColumnDef::new(tag::Column::TagName).text().not_null().unique_key())
                .col(ColumnDef::new(tag::Column::TagType).text().not_null().default("auto"))
                .to_owned()
        ).await?;

        // file_tags
        manager.create_table(
            Table::create()
                .table(file_tag::Entity)
                .if_not_exists()
                .col(ColumnDef::new(file_tag::Column::FileKey).text().not_null())
                .col(ColumnDef::new(file_tag::Column::TagId).integer().not_null())
                .primary_key(Index::create().col(file_tag::Column::FileKey).col(file_tag::Column::TagId))
                .to_owned()
        ).await?;

        // scan_metadata
        manager.create_table(
            Table::create()
                .table(scan_metadata::Entity)
                .if_not_exists()
                .col(ColumnDef::new(scan_metadata::Column::HostId).text().primary_key().not_null())
                .col(ColumnDef::new(scan_metadata::Column::LastScannedKey).text())
                .col(ColumnDef::new(scan_metadata::Column::LastScannedAt).text())
                .col(ColumnDef::new(scan_metadata::Column::TotalFiles).big_integer())
                .col(ColumnDef::new(scan_metadata::Column::TotalSize).big_integer())
                .col(ColumnDef::new(scan_metadata::Column::DbSchemaVersion).big_integer().not_null().default(1))
                .to_owned()
        ).await?;

        // dir_sizes
        manager.create_table(
            Table::create()
                .table(dir_size::Entity)
                .if_not_exists()
                .col(ColumnDef::new(dir_size::Column::HostId).text().not_null())
                .col(ColumnDef::new(dir_size::Column::DirPath).text().not_null())
                .col(ColumnDef::new(dir_size::Column::TotalSize).big_integer().not_null())
                .col(ColumnDef::new(dir_size::Column::TotalFiles).big_integer().not_null())
                .primary_key(Index::create().col(dir_size::Column::HostId).col(dir_size::Column::DirPath))
                .to_owned()
        ).await?;

        // scan_objects
        manager.create_table(
            Table::create()
                .table(scan_object::Entity)
                .if_not_exists()
                .col(ColumnDef::new(scan_object::Column::ScanId).text().not_null())
                .col(ColumnDef::new(scan_object::Column::Key).text().not_null())
                .col(ColumnDef::new(scan_object::Column::HostId).text().not_null())
                .col(ColumnDef::new(scan_object::Column::Etag).text().not_null())
                .col(ColumnDef::new(scan_object::Column::Size).big_integer().not_null())
                .col(ColumnDef::new(scan_object::Column::LastModified).text().not_null())
                .col(ColumnDef::new(scan_object::Column::IsDeleted).boolean().not_null().default(false))
                .primary_key(Index::create().col(scan_object::Column::ScanId).col(scan_object::Column::Key))
                .to_owned()
        ).await?;

        // traffic_log
        manager.create_table(
            Table::create()
                .table(traffic_log::Entity)
                .if_not_exists()
                .col(ColumnDef::new(traffic_log::Column::Id).integer().auto_increment().primary_key().not_null())
                .col(ColumnDef::new(traffic_log::Column::HostId).text().not_null())
                .col(ColumnDef::new(traffic_log::Column::Operation).text().not_null())
                .col(ColumnDef::new(traffic_log::Column::Business).text().not_null())
                .col(ColumnDef::new(traffic_log::Column::Direction).text().not_null())
                .col(ColumnDef::new(traffic_log::Column::Bytes).big_integer().not_null())
                .col(ColumnDef::new(traffic_log::Column::Count).big_integer().not_null())
                .col(ColumnDef::new(traffic_log::Column::RecordedAt).text().not_null())
                .to_owned()
        ).await?;

        // traffic_file_log
        manager.create_table(
            Table::create()
                .table(traffic_file_log::Entity)
                .if_not_exists()
                .col(ColumnDef::new(traffic_file_log::Column::Id).integer().auto_increment().primary_key().not_null())
                .col(ColumnDef::new(traffic_file_log::Column::HostId).text().not_null())
                .col(ColumnDef::new(traffic_file_log::Column::FileKey).text().not_null())
                .col(ColumnDef::new(traffic_file_log::Column::Business).text().not_null())
                .col(ColumnDef::new(traffic_file_log::Column::Bytes).big_integer().not_null())
                .col(ColumnDef::new(traffic_file_log::Column::Count).big_integer().not_null())
                .col(ColumnDef::new(traffic_file_log::Column::RecordedAt).text().not_null())
                .to_owned()
        ).await?;

        // traffic_stats
        manager.create_table(
            Table::create()
                .table(traffic_stats::Entity)
                .if_not_exists()
                .col(ColumnDef::new(traffic_stats::Column::HostId).text().not_null())
                .col(ColumnDef::new(traffic_stats::Column::Period).text().not_null())
                .col(ColumnDef::new(traffic_stats::Column::Operation).text().not_null())
                .col(ColumnDef::new(traffic_stats::Column::Business).text().not_null())
                .col(ColumnDef::new(traffic_stats::Column::Direction).text().not_null())
                .col(ColumnDef::new(traffic_stats::Column::TotalBytes).big_integer().not_null())
                .col(ColumnDef::new(traffic_stats::Column::TotalCount).big_integer().not_null())
                .primary_key(Index::create()
                    .col(traffic_stats::Column::HostId)
                    .col(traffic_stats::Column::Period)
                    .col(traffic_stats::Column::Operation)
                    .col(traffic_stats::Column::Business)
                    .col(traffic_stats::Column::Direction))
                .to_owned()
        ).await?;

        // ── 索引 ──
        manager.create_index(Index::create().name("idx_files_file_type").table(file::Entity).col(file::Column::FileType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_files_host_id").table(file::Entity).col(file::Column::HostId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_files_last_modified").table(file::Entity).col(file::Column::LastModified).to_owned()).await?;
        manager.create_index(Index::create().name("idx_metadata_namespace").table(metadata::Entity).col(metadata::Column::Namespace).to_owned()).await?;
        manager.create_index(Index::create().name("idx_metadata_file_key").table(metadata::Entity).col(metadata::Column::FileKey).to_owned()).await?;
        manager.create_index(Index::create().name("idx_metadata_key_value").table(metadata::Entity).col(metadata::Column::Key).col(metadata::Column::Value).to_owned()).await?;
        manager.create_index(Index::create().name("idx_tags_tag_type").table(tag::Entity).col(tag::Column::TagType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_thumbnails_cached_at").table(thumbnail::Entity).col(thumbnail::Column::CachedAt).to_owned()).await?;
        manager.create_index(Index::create().name("idx_traffic_log_host_time").table(traffic_log::Entity).col(traffic_log::Column::HostId).col(traffic_log::Column::RecordedAt).to_owned()).await?;
        manager.create_index(Index::create().name("idx_traffic_log_business").table(traffic_log::Entity).col(traffic_log::Column::Business).to_owned()).await?;
        manager.create_index(Index::create().name("idx_traffic_file_log_host_key").table(traffic_file_log::Entity).col(traffic_file_log::Column::HostId).col(traffic_file_log::Column::FileKey).to_owned()).await?;
        manager.create_index(Index::create().name("idx_traffic_file_log_time").table(traffic_file_log::Entity).col(traffic_file_log::Column::RecordedAt).to_owned()).await?;
        manager.create_index(Index::create().name("idx_scan_objects_scan_id").table(scan_object::Entity).col(scan_object::Column::ScanId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_scan_objects_host_id").table(scan_object::Entity).col(scan_object::Column::HostId).to_owned()).await?;

        // ── namespace_custom 数据迁移 ──
        let db = manager.get_connection();
        db.execute_unprepared(
            "UPDATE metadata SET namespace_custom = namespace, namespace = 'custom' \
             WHERE namespace NOT IN ('exif', 'video', 'audio', 'general')"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(traffic_stats::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(traffic_file_log::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(traffic_log::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(scan_object::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(dir_size::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(scan_metadata::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(file_tag::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(tag::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(thumbnail::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(extractor_rule::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(classification_rule::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(metadata::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(file::Entity).to_owned()).await?;
        manager.drop_table(Table::drop().table(host_config::Entity).to_owned()).await?;
        Ok(())
    }
}
```

- [ ] **Step 2: 改造 db/pool.rs**

```rust
// crates/s3-gallery-core/src/db/pool.rs
use std::path::Path;
use sea_orm::{Database, DatabaseConnection};
use crate::error::{Result, S3GalleryError};

pub async fn create_pool(path: &Path) -> Result<DatabaseConnection> {
    let url = format!("sqlite:{}?mode=rwc", path.display());
    Database::connect(&url).await.map_err(|e| {
        S3GalleryError::DbError(format!("Failed to connect to database: {e}"))
    })
}
```

- [ ] **Step 3: 更新 db/mod.rs**

```rust
// crates/s3-gallery-core/src/db/mod.rs
pub mod migrate;
pub mod pool;
pub mod status;
pub mod models;  // 保留，最后一 task 删除
```

- [ ] **Step 4: 更新 db/schema.rs 中的 run_migrations 函数**

将 `run_migrations` 改为使用 SeaORM 的 Migrator，保持向后兼容（其他模块暂时仍调用 `run_migrations`）：

```rust
// crates/s3-gallery-core/src/db/schema.rs — 修改 run_migrations 函数
use sea_orm_migration::MigratorTrait;
use crate::db::migrate::InitialMigration;

pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    InitialMigration::up(db, None).await.map_err(|e| {
        S3GalleryError::DbError(format!("Migration failed: {e}"))
    })?;
    Ok(())
}
```

- [ ] **Step 5: 验证编译**

```bash
cargo check 2>&1 | head -30
```
Expected: 编译错误集中在 view 模块中使用了旧的 `SqlitePool` 类型。

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/db/migrate.rs crates/s3-gallery-core/src/db/pool.rs crates/s3-gallery-core/src/db/mod.rs crates/s3-gallery-core/src/db/schema.rs
git commit -m "refactor: add SeaORM migration system, replace pool with DatabaseConnection"
```

---

### Task 4: view 模块迁移 — 简单查询（C 层）

**Files:**
- Modify: `crates/s3-gallery-core/src/view/ls.rs`
- Modify: `crates/s3-gallery-core/src/view/tree.rs`
- Modify: `crates/s3-gallery-core/src/view/stat.rs`
- Modify: `crates/s3-gallery-core/src/view/export.rs`

- [ ] **Step 1: 迁移 view/ls.rs**

将 `SqlitePool` 参数改为 `&DatabaseConnection`，所有查询改为 SeaORM Finder API：

```rust
// 修改前
pub async fn list_directory(
    db: &SqlitePool,
    host_id: &str,
    prefix: &str,
) -> Result<Vec<FileEntry>> {
    let files: Vec<FileEntry> = sqlx::query_as(
        "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 ORDER BY key"
    )
    .bind(host_id)
    .fetch_all(db)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    Ok(files)
}

// 修改后
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, Order, ColumnTrait};
use crate::entity::file;

pub async fn list_directory(
    db: &DatabaseConnection,
    host_id: &str,
    prefix: &str,
) -> Result<Vec<file::Model>> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .order_by(file::Column::Key, Order::Asc)
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    Ok(files)
}
```

**注意：** `ls.rs` 返回类型从 `FileEntry` 改为 `file::Model`。调用者需要相应调整。

- [ ] **Step 2: 迁移 view/tree.rs**

```rust
// 修改后
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait};
use crate::entity::file;

pub async fn build_tree(db: &DatabaseConnection, host_id: &str) -> Result<TreeNode> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .order_by(file::Column::Key, Order::Asc)
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    // 构建树形结构...
}
```

- [ ] **Step 3: 迁移 view/stat.rs**

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait};
use crate::entity::file;

pub async fn get_stats(db: &DatabaseConnection, host_id: &str) -> Result<FileStats> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    // 在内存中统计...
}
```

- [ ] **Step 4: 迁移 view/export.rs**

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait};
use crate::entity::file;

pub async fn export_files(db: &DatabaseConnection, host_id: &str, format: &str) -> Result<String> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    // 导出逻辑...
}
```

- [ ] **Step 5: 验证编译**

```bash
cargo check 2>&1 | head -20
```
Expected: 编译错误集中在 `view/mod.rs` 中的 `LocalView` 结构体仍使用 `SqlitePool`，以及调用了旧的 `FileEntry` 类型。

- [ ] **Step 6: Commit**

```bash
git add crates/s3-gallery-core/src/view/ls.rs crates/s3-gallery-core/src/view/tree.rs crates/s3-gallery-core/src/view/stat.rs crates/s3-gallery-core/src/view/export.rs
git commit -m "refactor: migrate view/ls/tree/stat/export to SeaORM finder API"
```

---

### Task 5: view 模块迁移 — 复杂查询（S 层 + R 层）

**Files:**
- Modify: `crates/s3-gallery-core/src/view/search.rs`
- Modify: `crates/s3-gallery-core/src/view/tags.rs`
- Modify: `crates/s3-gallery-core/src/view/timeline.rs`
- Modify: `crates/s3-gallery-core/src/view/timeline_gallery.rs`
- Modify: `crates/s3-gallery-core/src/view/duplicates.rs`
- Modify: `crates/s3-gallery-core/src/view/traffic.rs`

- [ ] **Step 1: 迁移 view/search.rs（动态条件）**

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, Order};
use crate::entity::file;

pub async fn search_by_name(
    db: &DatabaseConnection,
    query: &str,
    host_id: Option<&str>,
    file_type: Option<&str>,
    limit: u64,
) -> Result<Vec<file::Model>> {
    let mut select = file::Entity::find()
        .filter(file::Column::IsDeleted.eq(false));

    if let Some(host_id) = host_id {
        select = select.filter(file::Column::HostId.eq(host_id));
    }
    if let Some(file_type) = file_type {
        select = select.filter(file::Column::FileType.eq(file_type));
    }

    let files = select
        .filter(file::Column::Key.contains(query))  // LIKE '%query%'
        .order_by(file::Column::LastModified, Order::Desc)
        .limit(limit)
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    Ok(files)
}
```

- [ ] **Step 2: 迁移 view/tags.rs（JOIN 查询）**

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, RelationTrait, ColumnTrait};
use crate::entity::{file, tag, file_tag};

pub async fn get_files_by_tag(
    db: &DatabaseConnection,
    tag_name: &str,
    host_id: Option<&str>,
) -> Result<Vec<file::Model>> {
    let mut query = file::Entity::find()
        .join_rev(file_tag::Relation::Tags.def())
        .filter(tag::Column::TagName.eq(tag_name))
        .filter(file::Column::IsDeleted.eq(false));

    if let Some(host_id) = host_id {
        query = query.filter(file::Column::HostId.eq(host_id));
    }

    query.all(db).await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))
}
```

- [ ] **Step 3: 迁移 view/timeline.rs**

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, Order};

pub async fn get_timeline(
    db: &DatabaseConnection,
    host_id: Option<&str>,
) -> Result<Vec<file::Model>> {
    let mut query = file::Entity::find()
        .filter(file::Column::IsDeleted.eq(false))
        .order_by(file::Column::LastModified, Order::Desc);

    if let Some(host_id) = host_id {
        query = query.filter(file::Column::HostId.eq(host_id));
    }

    query.all(db).await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))
}
```

- [ ] **Step 4: 迁移 view/duplicates.rs**

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, Order};

pub async fn find_duplicates(
    db: &DatabaseConnection,
    host_id: Option<&str>,
) -> Result<Vec<DuplicateGroup>> {
    // 简单查询：按 size + etag 分组查找
    // Raw SQL 保留，因为需要 GROUP BY 聚合
    let stmt = Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT size, etag, COUNT(*) as cnt FROM files \
         WHERE is_deleted = 0 GROUP BY size, etag HAVING COUNT(*) > 1"
    );
    let rows = db.query_all(stmt).await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    // 处理结果...
}
```

- [ ] **Step 5: 迁移 view/traffic.rs（Raw SQL 保留）**

```rust
use sea_orm::{DatabaseConnection, Statement, QueryTrait};

pub async fn get_traffic_summary(
    db: &DatabaseConnection,
    host_id: Option<&str>,
) -> Result<Vec<BusinessTraffic>> {
    // 复杂聚合查询保留 Raw SQL
    let stmt = Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT business, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
         FROM traffic_log GROUP BY business, direction"
    );
    let rows = db.query_all(stmt).await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    // 解析结果...
}
```

- [ ] **Step 6: 更新 view/mod.rs（LocalView 结构体）**

```rust
// crates/s3-gallery-core/src/view/mod.rs
use sea_orm::DatabaseConnection;

pub struct LocalView {
    pub db: DatabaseConnection,  // 改 DatabaseConnection
}
```

- [ ] **Step 7: 验证编译**

```bash
cargo check 2>&1 | head -30
```
Expected: 编译错误集中在 scan/ 和 s3/ 模块中使用了旧的 `SqlitePool`。

- [ ] **Step 8: Commit**

```bash
git add crates/s3-gallery-core/src/view/search.rs crates/s3-gallery-core/src/view/tags.rs crates/s3-gallery-core/src/view/timeline.rs crates/s3-gallery-core/src/view/timeline_gallery.rs crates/s3-gallery-core/src/view/duplicates.rs crates/s3-gallery-core/src/view/traffic.rs crates/s3-gallery-core/src/view/mod.rs
git commit -m "refactor: migrate view/search/tags/timeline/duplicates/traffic to SeaORM"
```

---

### Task 6: scan/s3 模块适配

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`
- Modify: `crates/s3-gallery-core/src/scan/discover.rs`
- Modify: `crates/s3-gallery-core/src/scan/aggregate.rs`
- Modify: `crates/s3-gallery-core/src/scan/process.rs`
- Modify: `crates/s3-gallery-core/src/scan/diff_layer.rs`
- Modify: `crates/s3-gallery-core/src/scan/batch_service.rs`
- Modify: `crates/s3-gallery-core/src/scan/exif_service.rs`
- Modify: `crates/s3-gallery-core/src/scan/tag_service.rs`
- Modify: `crates/s3-gallery-core/src/scan/scan_objects.rs`
- Modify: `crates/s3-gallery-core/src/s3/traffic_persist.rs`
- Modify: `crates/s3-gallery-core/src/s3/client.rs`
- Modify: `crates/s3-gallery-core/src/s3/real.rs`
- Modify: `crates/s3-gallery-core/src/s3/mock.rs`
- Modify: `crates/s3-gallery-core/src/s3/logged.rs`
- Modify: `crates/s3-gallery-core/src/s3/s3_service.rs`
- Modify: `crates/s3-gallery-core/src/s3/traffic_recorder.rs`
- Modify: `crates/s3-gallery-core/src/s3/config.rs`
- Modify: `crates/s3-gallery-core/src/s3/lock.rs`

- [ ] **Step 1: 全局替换 SqlitePool → DatabaseConnection**

在 scan/ 和 s3/ 目录中，将所有函数签名中的 `SqlitePool` 替换为 `&DatabaseConnection`：

```rust
// 替换前
async fn foo(pool: &SqlitePool, ...) -> Result<()>

// 替换后
async fn foo(db: &DatabaseConnection, ...) -> Result<()>
```

所有 `sqlx::query(...)` 替换为相应的 SeaORM 查询。

- [ ] **Step 2: 更新 S3Client trait 的 list_objects 方法**

```rust
// crates/s3-gallery-core/src/s3/client.rs
pub trait S3Client: Send + Sync {
    async fn list_objects(
        &self,
        bucket: &BucketName,
        prefix: &Prefix,  // 从 &ObjectKey 改为 &Prefix
    ) -> Result<Vec<ObjectSummary>>;
    // 其余方法不变...
}
```

- [ ] **Step 3: 更新 Prefix 相关的所有实现**

在 `real.rs`、`mock.rs`、`logged.rs`、`traffic_recorder.rs` 中更新 `list_objects` 的 `prefix` 参数类型为 `&Prefix`。

- [ ] **Step 4: 更新 ScanRequest 和 ScanConfig 中的 prefix 字段**

```rust
// crates/s3-gallery-core/src/scan/pipeline.rs
pub struct ScanRequest {
    pub scope_prefix: Prefix,  // 从 ObjectKey 改为 Prefix
    // ...
}
```

- [ ] **Step 5: 更新 s3/config.rs 中的 prefix 字段**

```rust
// crates/s3-gallery-core/src/s3/config.rs
pub struct OssConfig {
    pub prefix: Prefix,  // 从 ObjectKey 改为 Prefix
    // ...
}
```

- [ ] **Step 6: 更新 scan 模块中的 SQL 查询**

在 `aggregate.rs` 中，将 `traffic_log` 查询改为使用 SeaORM 或保留 Raw SQL：

```rust
// aggregate.rs 中的查询改为 Raw SQL 保留
use sea_orm::Statement;

let stmt = Statement::from_string(
    sea_orm::DatabaseBackend::Sqlite,
    "SELECT business, operation, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
     FROM traffic_log WHERE recorded_at >= ? AND recorded_at <= ? AND business LIKE 'scan_%' \
     GROUP BY business, operation, direction"
);
```

- [ ] **Step 7: 更新 scan_objects.rs 中的 CRUD**

```rust
// crates/s3-gallery-core/src/scan/scan_objects.rs
use crate::entity::scan_object;

pub async fn list_by_scan(db: &DatabaseConnection, scan_id: &str) -> Result<Vec<scan_object::Model>> {
    scan_object::Entity::find()
        .filter(scan_object::Column::ScanId.eq(scan_id))
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))
}
```

- [ ] **Step 8: 验证编译**

```bash
cargo check 2>&1 | head -30
```
Expected: 编译错误集中在 CLI 和测试文件中使用了旧的 `SqlitePool`。

- [ ] **Step 9: Commit**

```bash
git add crates/s3-gallery-core/src/scan/ crates/s3-gallery-core/src/s3/
git commit -m "refactor: adapt scan/s3 modules to SeaORM + Prefix type"
```

---

### Task 7: CLI 和测试适配

**Files:**
- Modify: `crates/s3-gallery-cli/src/cmd_serve.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_scan.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_init.rs`
- Modify: `crates/s3-gallery-cli/src/cmd_db.rs`
- Modify: `crates/s3-gallery-cli/src/web/handlers/*.rs`
- Modify: `tests/*.rs`

- [ ] **Step 1: 替换 CLI 命令中的 SqlitePool**

在 `cmd_serve.rs`、`cmd_scan.rs`、`cmd_init.rs`、`cmd_db.rs` 中，将所有 `SqlitePool` 替换为 `DatabaseConnection`。

- [ ] **Step 2: 更新 web handlers 中的 DatabaseConnection 引用**

- [ ] **Step 3: 更新测试文件中的 SqlitePool**

在 `tests/` 目录中，将所有测试辅助函数中的 `SqlitePool` 替换为 `DatabaseConnection`。

```rust
// 修改前
async fn setup_test_db() -> SqlitePool { ... }

// 修改后
async fn setup_test_db() -> DatabaseConnection { ... }
```

- [ ] **Step 4: 验证编译**

```bash
cargo check 2>&1 | head -30
```
Expected: 编译成功，或少量错误。

- [ ] **Step 5: Commit**

```bash
git add crates/s3-gallery-cli/ tests/
git commit -m "refactor: adapt CLI and tests to SeaORM DatabaseConnection"
```

---

### Task 8: 删除旧文件 + 最终编译修复

**Files:**
- Delete: `crates/s3-gallery-core/src/db/models.rs`
- Delete: `crates/s3-gallery-core/src/db/schema.rs`（如果 migrate.rs 已完成）

- [ ] **Step 1: 删除 models.rs 和 schema.rs**

```bash
git rm crates/s3-gallery-core/src/db/models.rs
git rm crates/s3-gallery-core/src/db/schema.rs
```

- [ ] **Step 2: 更新 db/mod.rs 移除旧模块引用**

```rust
// crates/s3-gallery-core/src/db/mod.rs
pub mod migrate;
pub mod pool;
pub mod status;
// pub mod models;  // 删除
// pub mod schema;  // 删除，如已移除
```

- [ ] **Step 3: 修复所有编译错误**

```bash
cargo check 2>&1
```

修复任何剩余的编译错误（类型不匹配、导入缺失等）。

- [ ] **Step 4: 运行测试**

```bash
cargo test --lib 2>&1
cargo test --test integration 2>&1 | head -50
```

- [ ] **Step 5: 修复测试失败**

修复 `metadata_namespace_from_str_invalid` 测试（已删除），以及任何因类型变化导致的测试失败。

- [ ] **Step 6: 最终提交**

```bash
git add crates/s3-gallery-core/src/db/ crates/s3-gallery-core/src/lib.rs
git commit -m "refactor: remove old models.rs and schema.rs, finalize SeaORM migration"
```