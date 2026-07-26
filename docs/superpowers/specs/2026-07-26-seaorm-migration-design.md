# SeaORM 迁移 — 类型安全整改设计

> **目标：** 将项目从 sqlx raw query 迁移到 SeaORM，解决 23 项类型安全违规，使 DB 列的 TEXT 类型在 Rust 中自动映射到枚举和 Newtype
>
> **架构：** 引入 SeaORM Entity 定义层 + 保留 Raw SQL 混合模式。Entity 定义自动映射类型，复杂聚合查询保留 SQL 字符串但绑定到 Entity 类型。
>
> **技术栈：** SeaORM 1.x (sqlx-sqlite + runtime-tokio-rustls)

---

## 一、设计原则

1. **语义有意义的 String 字段必须用 Newtype** — DB 存 TEXT，Rust 类型用 `ObjectKey` / `Etag` / `HostId` / `FileSize`
2. **分类字段必须用枚举** — `FileType` / `MetadataState` / `TagType` / `Direction` / `S3Operation` 等
3. **Raw SQL 保留** — 复杂聚合查询保留 SQL 字符串，但结果绑定到 Entity 类型享受自动映射
4. **DB 迁移零数据风险** — `DeriveActiveEnum` 的 `string_value` 与现有 DB 值完全一致，无需数据迁移
5. **`namespace_custom` 额外列** — `MetadataNamespace::Custom(String)` 使用额外列存储自定义值
6. **CRUD 方法迁移到 Entity 的 ActiveModel** — 现有 `models.rs` 中的 CRUD 方法逐批迁移到 SeaORM 的 `ActiveModel::insert()` / `Entity::update_many()` 等操作

---

## 二、Entity 类型映射

### 2.1 枚举 → 自动映射（DeriveActiveEnum）

所有枚举字段使用 `DeriveActiveEnum` 自动生成 `TryGetable` / `ValueType` / `Display` / `FromStr`。

`string_value` 必须与现有 DB 中的 TEXT 值完全一致。

**涉及的枚举：**

| 枚举 | DB 列 | 变体数 | string_value 值 | 特殊处理 |
|:-----|:------|:------:|:---------------|:---------|
| `FileType` | `files.file_type` | 29 | `"jpeg"`, `"png"`, `"tar_gz"`, `"seven_z"` 等 | 无 |
| `MetadataState` | `files.metadata_state` | 3 | `"pending"`, `"extracted"`, `"failed"` | 无 |
| `MetadataNamespace` | `metadata.namespace` | 5 | `"exif"`, `"video"`, `"audio"`, `"general"`, `"custom"` | `Custom` 变体用额外列 |
| `TagType` | `tags.tag_type` | 2 | `"auto"`, `"manual"` | 无 |
| `Direction` | `traffic_log.direction` | 2 | `"download"`, `"upload"` | 新增枚举 |
| `S3Operation` | `traffic_log.operation` | 8 | `"GetObject"`, `"GetObjectRange"`, `"PutObject"`, `"PutObjectIfNoneMatch"`, `"ListObjects"`, `"HeadObject"`, `"DeleteObject"`, `"ObjectExists"` | 从 `traffic_recorder.rs` 移入，Display 保持 PascalCase |
| `ThumbnailFormat` | `thumbnails.format` | 3 | `"jpeg"`, `"png"`, `"webp"` | 新增枚举 |
| `BusinessLabel` | `traffic_log.business` | — | 保留 String | 用户可扩展 |

**各枚举的 DeriveActiveEnum 定义：**

```rust
#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    #[sea_orm(string_value = "jpeg")]    #[serde(rename = "jpeg")]    Jpeg,
    #[sea_orm(string_value = "png")]     #[serde(rename = "png")]     Png,
    #[sea_orm(string_value = "gif")]     #[serde(rename = "gif")]     Gif,
    #[sea_orm(string_value = "webp")]    #[serde(rename = "webp")]    WebP,
    #[sea_orm(string_value = "bmp")]     #[serde(rename = "bmp")]     Bmp,
    #[sea_orm(string_value = "svg")]     #[serde(rename = "svg")]     Svg,
    #[sea_orm(string_value = "tiff")]    #[serde(rename = "tiff")]    Tiff,
    #[sea_orm(string_value = "mp4")]     #[serde(rename = "mp4")]     Mp4,
    #[sea_orm(string_value = "mov")]     #[serde(rename = "mov")]     Mov,
    #[sea_orm(string_value = "avi")]     #[serde(rename = "avi")]     Avi,
    #[sea_orm(string_value = "mkv")]     #[serde(rename = "mkv")]     Mkv,
    #[sea_orm(string_value = "webm")]    #[serde(rename = "webm")]    WebM,
    #[sea_orm(string_value = "mp3")]     #[serde(rename = "mp3")]     Mp3,
    #[sea_orm(string_value = "flac")]    #[serde(rename = "flac")]    Flac,
    #[sea_orm(string_value = "wav")]     #[serde(rename = "wav")]     Wav,
    #[sea_orm(string_value = "ogg")]     #[serde(rename = "ogg")]     Ogg,
    #[sea_orm(string_value = "aac")]     #[serde(rename = "aac")]     Aac,
    #[sea_orm(string_value = "m4a")]     #[serde(rename = "m4a")]     M4a,
    #[sea_orm(string_value = "pdf")]     #[serde(rename = "pdf")]     Pdf,
    #[sea_orm(string_value = "doc")]     #[serde(rename = "doc")]     Doc,
    #[sea_orm(string_value = "docx")]    #[serde(rename = "docx")]    Docx,
    #[sea_orm(string_value = "xls")]     #[serde(rename = "xls")]     Xls,
    #[sea_orm(string_value = "xlsx")]    #[serde(rename = "xlsx")]    Xlsx,
    #[sea_orm(string_value = "ppt")]     #[serde(rename = "ppt")]     Ppt,
    #[sea_orm(string_value = "pptx")]    #[serde(rename = "pptx")]    Pptx,
    #[sea_orm(string_value = "zip")]     #[serde(rename = "zip")]     Zip,
    #[sea_orm(string_value = "rar")]     #[serde(rename = "rar")]     Rar,
    #[sea_orm(string_value = "tar_gz")]  #[serde(rename = "tar_gz")]  TarGz,
    #[sea_orm(string_value = "seven_z")] #[serde(rename = "seven_z")] SevenZ,
    #[sea_orm(string_value = "unknown")] #[serde(rename = "unknown")] Unknown,
}

#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
// 注意：S3Operation 的 DB 值为 PascalCase（如 "GetObject"），
// serde 默认使用变体名（PascalCase），因此无需 rename_all
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

### 2.2 Newtype → 自定义 TryGetable（宏桥接）

```rust
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
```

`FileSize` 特殊处理（i64 ↔ u64）：

```rust
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

### 2.3 S3Path trait + Prefix 类型

```rust
/// 统一的路径操作 trait，适用于 ObjectKey 和 Prefix。
pub trait S3Path: Sized {
    fn as_str(&self) -> &str;
    fn parent(&self) -> Option<Prefix>;
    fn last_segment(&self) -> Option<&str>;
    fn join_key(&self, name: &str) -> ObjectKey;
    fn join_dir(&self, name: &str) -> Prefix;
}

/// 目录前缀 / 列表前缀。
///
/// 验证规则：空字符串（根目录）或以 `/` 结尾（目录路径），max 1024 字符。
/// 用于：`S3Client::list_objects` 的 prefix 参数、`dir_sizes.dir_path`、
/// `scan scope_prefix` 等目录/前缀场景。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Prefix(String);

impl Prefix {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        let len = s.len();
        if len > 1024 {
            return Err(S3GalleryError::ValidationError(format!(
                "prefix must be at most 1024 characters, got {len}"
            )));
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
        let parent_str = self.0.get(..=pos)?;  // 保留尾随 /
        Some(Prefix(parent_str.to_string()))
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

**改动范围（S3Client trait 和所有实现）：**

| 位置 | 当前 | 改为 |
|:-----|:----:|:----:|
| `S3Client::list_objects(bucket, prefix)` | `prefix: &ObjectKey` | `prefix: &Prefix` |
| `ScanRequest.scope_prefix` | `ObjectKey` | `Prefix` |
| `ScanConfig.prefix` | `ObjectKey` | `Prefix` |
| `OssConfig.prefix` | `ObjectKey` | `Prefix` |
| `entity::dir_size.dir_path` | `String` | `Prefix` |
| `ObjectKey::parent()` 返回值 | `Option<ObjectKey>` | `Option<Prefix>` |

### 2.4 MetadataNamespace 特殊处理

```rust
#[derive(Debug, Clone, PartialEq, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum MetadataNamespace {
    #[sea_orm(string_value = "exif")]    Exif,
    #[sea_orm(string_value = "video")]   Video,
    #[sea_orm(string_value = "audio")]   Audio,
    #[sea_orm(string_value = "general")] General,
    #[sea_orm(string_value = "custom")]  Custom,  // 自定义值存到 namespace_custom 列
}
```

Entity 中 `namespace_custom` 列：

```rust
pub struct Model {
    pub file_key: ObjectKey,
    pub namespace: MetadataNamespace,
    pub namespace_custom: Option<String>,  // 当 namespace = Custom 时填充
    pub key: String,
    pub value: String,
    pub extracted_at: String,
    pub partial: bool,
}
```

**数据迁移脚本**（在 SeaORM 迁移中执行）：

```sql
-- 1. 添加 namespace_custom 列
ALTER TABLE metadata ADD COLUMN namespace_custom TEXT;

-- 2. 将非标准 namespace 值迁移到新的列结构
UPDATE metadata
SET namespace_custom = namespace,
    namespace = 'custom'
WHERE namespace NOT IN ('exif', 'video', 'audio', 'general');
```

迁移后，`namespace` 列只包含 5 个标准值（`exif`, `video`, `audio`, `general`, `custom`），`DeriveActiveEnum` 可以安全反序列化。自定义值（如 `"XMP"`）存储在 `namespace_custom` 列中。

---

## 三、Entity 定义（14 张表）

### 3.1 files Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,        // 复合 PK 的一部分
    #[sea_orm(primary_key)]
    pub key: ObjectKey,         // 复合 PK 的一部分
    pub etag: Etag,
    pub size: FileSize,
    pub last_modified: String,
    pub content_type: Option<String>,
    pub file_type: FileType,
    pub metadata_state: MetadataState,
    pub is_deleted: bool,
    pub effective_date: String, // 回填的日期字段
}
```

### 3.2 host_config Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "host_config")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    pub host_name: String,
    pub host_type: String,      // 保留 String（用户可扩展）
    pub description: String,
    pub created_at: String,
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
}
```

### 3.3 metadata Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "metadata")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,        // 复合 PK
    #[sea_orm(primary_key)]
    pub namespace: MetadataNamespace, // 复合 PK
    #[sea_orm(primary_key)]
    pub key: String,                // 复合 PK
    pub namespace_custom: Option<String>,  // 新增列
    pub value: String,
    pub extracted_at: String,
    pub partial: bool,
}
```

### 3.4 tag + file_tag Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tags")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub tag_id: i64,
    pub tag_name: String,
    pub tag_type: TagType,      // 改为枚举
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,
    #[sea_orm(primary_key)]
    pub tag_id: i64,
}
```

### 3.5 traffic_log Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_log")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
    pub host_id: HostId,
    pub operation: S3Operation,     // 改为枚举
    pub business: String,           // 保留 String（用户可扩展）
    pub direction: Direction,       // 改为枚举
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}
```

### 3.6 traffic_file_log Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_file_log")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
    pub host_id: HostId,
    pub file_key: ObjectKey,        // 改为 Newtype
    pub business: String,
    pub bytes: i64,
    pub count: i64,
    pub recorded_at: String,
}
```

### 3.7 traffic_stats Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "traffic_stats")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    #[sea_orm(primary_key)]
    pub period: String,
    #[sea_orm(primary_key)]
    pub operation: S3Operation,     // 改为枚举
    #[sea_orm(primary_key)]
    pub business: String,
    #[sea_orm(primary_key)]
    pub direction: Direction,       // 改为枚举
    pub total_bytes: i64,
    pub total_count: i64,
}
```

### 3.8 thumbnail Entity

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "thumbnails")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub file_key: ObjectKey,        // 改为 Newtype
    pub data: Vec<u8>,
    pub format: ThumbnailFormat,    // 改为枚举
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub cached_at: String,
}
```

### 3.9 其余 Entity（简略定义）

```rust
// classification_rules
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "classification_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub extension: String,
    pub file_type: String,
    pub priority: i32,
    pub description: Option<String>,
}

// extractor_rules
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "extractor_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub extension: String,
    #[sea_orm(primary_key)]
    pub extractor_name: String,
    pub priority: i32,
}

// scan_metadata
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "scan_metadata")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    pub last_scanned_key: Option<String>,
    pub last_scanned_at: Option<String>,
    pub total_files: Option<i64>,
    pub total_size: Option<i64>,
    pub db_schema_version: i64,
}

// dir_sizes
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "dir_sizes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub host_id: HostId,
    #[sea_orm(primary_key)]
    pub dir_path: Prefix,         // 改为 Prefix（目录路径，空字符串或 / 结尾）
    pub total_size: i64,
    pub total_files: i64,         // 注意：表字段名为 total_files，非 file_count
}

// scan_objects
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "scan_objects")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub scan_id: String,
    #[sea_orm(primary_key)]
    pub key: ObjectKey,
    pub host_id: HostId,
    pub etag: Etag,
    pub size: FileSize,
    pub last_modified: String,
    pub is_deleted: bool,
}
```

---

## 四、Relation 定义

```rust
// file.rs
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

// file_tag.rs
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
```

---

## 五、查询模式转换

### 5.1 C 层：Finder API（80% 查询）

```rust
// 多行查询
let files: Vec<file::Model> = file::Entity::find()
    .filter(file::Column::HostId.eq(host_id))
    .filter(file::Column::IsDeleted.eq(false))
    .order_by(file::Column::Key, Order::Asc)
    .all(&db).await?;

// 单行查询
let file = file::Entity::find_by_id((host_id.clone(), key.clone()))
    .one(&db).await?
    .ok_or(S3GalleryError::NotFound(key.to_string()))?;

// INSERT
let model = file::ActiveModel {
    key: Set(key),
    host_id: Set(host_id),
    etag: Set(etag),
    size: Set(size),
    last_modified: Set(last_modified),
    file_type: Set(file_type),         // 直接赋枚举值
    metadata_state: Set(metadata_state),
    is_deleted: Set(false),
    ..Default::default()
};
model.insert(&db).await?;

// UPDATE
file::Entity::update_many()
    .col_expr(file::Column::IsDeleted, Expr::value(true))
    .filter(file::Column::HostId.eq(host_id))
    .filter(file::Column::Key.eq(key))
    .exec(&db).await?;

// DELETE
file::Entity::delete_many()
    .filter(file::Column::HostId.eq(host_id))
    .filter(file::Column::Key.eq(key))
    .exec(&db).await?;
```

### 5.2 S 层：Select 构建器（15% 查询）

```rust
// 动态条件查询
let mut select = file::Entity::find()
    .filter(file::Column::IsDeleted.eq(false));

if let Some(host_id) = &host_id {
    select = select.filter(file::Column::HostId.eq(host_id.clone()));
}
if let Some(file_type) = &file_type {
    select = select.filter(file::Column::FileType.eq(file_type.clone()));
}

let files = select
    .order_by(file::Column::LastModified, Order::Desc)
    .limit(limit)
    .all(&db).await?;

// JOIN 查询（tags.rs）
let files = file::Entity::find()
    .join_rev(file_tags::Relation::Tags.def())
    .filter(tag::Column::TagName.eq(tag_name))
    .filter(file::Column::HostId.eq(host_id))
    .filter(file::Column::IsDeleted.eq(false))
    .all(&db).await?;
```

### 5.3 R 层：Raw SQL 混合（5% 查询）

```rust
// 复杂聚合查询保留 Raw SQL
use sea_orm::Statement;

let stmt = Statement::from_string(
    sea_orm::DatabaseBackend::Sqlite,
    "SELECT business, direction, COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
     FROM traffic_log WHERE recorded_at >= ? AND recorded_at <= ? \
     GROUP BY business, direction"
);

let rows = db.query_all(stmt).await?
    .into_iter()
    .map(|row| {
        let business: String = row.try_get_by(0)?;
        let direction: String = row.try_get_by(1)?;
        let bytes: i64 = row.try_get_by(2)?;
        let count: i64 = row.try_get_by(3)?;
        Ok((business, direction, bytes, count))
    })
    .collect::<Result<Vec<_>, _>>()?;
```

---

## 六、CRUD 迁移策略

现有 `models.rs`（~950 行，~30 个 CRUD 方法）**不一次性删除**，而是逐步迁移：

### 迁移步骤

1. **阶段 1**：创建 `entity/` 目录，定义所有 14 个 Entity
2. **阶段 2**：`types.rs` 添加 `DeriveActiveEnum` + 桥接宏
3. **阶段 3**：`db/pool.rs` 改为返回 `DatabaseConnection`，`db/schema.rs` 替换为 `db/migrate.rs`
4. **阶段 4**：`view/` 模块逐个改为 SeaORM 查询（编译通过后，不再依赖旧 `models.rs`）
5. **阶段 5**：`scan/` 和 `s3/` 模块替换 pool 类型
6. **阶段 6**：删除 `models.rs`，所有调用者已迁移完毕

### 具体 CRUD 方法映射

| 当前 `models.rs` 方法 | SeaORM 替代 |
|:----------------------|:------------|
| `HostConfigEntry::insert()` | `host_config::ActiveModel { ... }.insert(&db)` |
| `HostConfigEntry::list_all()` | `host_config::Entity::find().all(&db)` |
| `FileEntry::find_by_key()` | `file::Entity::find_by_id((host_id, key)).one(&db)` |
| `FileEntry::upsert()` | `file::ActiveModel { ... }.insert(&db)` 或 `update_many()` |
| `FileEntry::list_by_host()` | `file::Entity::find().filter(host_id.eq(h)).all(&db)` |
| `MetadataEntry::list_by_file()` | `metadata::Entity::find().filter(file_key.eq(k)).all(&db)` |
| `TagEntry::insert()` | `tag::ActiveModel { ... }.insert(&db)` |
| `TagEntry::ensure_exists()` | `tag::Entity::find().filter(name.eq(n)).one(&db)` + insert |
| `FileTagEntry::insert()` | `file_tag::ActiveModel { ... }.insert(&db)` |
| `ThumbnailEntry::update()` | `thumbnail::ActiveModel { ... }.insert(&db)` |
| `TrafficLogEntry::insert_batch()` | 保留 Raw SQL 批量 INSERT |
| `ScanObjectEntry::list_by_scan()` | `scan_object::Entity::find().filter(scan_id.eq(s)).all(&db)` |

---

## 七、迁移系统

### 7.1 schema.rs 替换为 SeaORM Migration

```rust
// crates/s3-gallery-core/src/db/migrate.rs
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct InitialMigration;

#[async_trait::async_trait]
impl MigrationTrait for InitialMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 创建 14 张表（完整定义见 schema.rs）
        manager.create_table(
            Table::create()
                .table(File::Table)
                .if_not_exists()
                .col(ColumnDef::new(File::HostId).text().not_null())
                .col(ColumnDef::new(File::Key).text().not_null())
                .col(ColumnDef::new(File::Etag).text().not_null())
                .col(ColumnDef::new(File::Size).big_integer().not_null())
                .col(ColumnDef::new(File::LastModified).text().not_null())
                .col(ColumnDef::new(File::ContentType).text())
                .col(ColumnDef::new(File::FileType).text().not_null())
                .col(ColumnDef::new(File::MetadataState).text().not_null())
                .col(ColumnDef::new(File::IsDeleted).boolean().not_null().default(false))
                .col(ColumnDef::new(File::EffectiveDate).text().not_null().default(""))
                .primary_key(
                    Index::create()
                        .col(File::HostId)
                        .col(File::Key)
                )
                .to_owned()
        ).await?;

        // metadata 表（含 namespace_custom 列）
        manager.create_table(
            Table::create()
                .table(Metadata::Table)
                .if_not_exists()
                .col(ColumnDef::new(Metadata::FileKey).text().not_null())
                .col(ColumnDef::new(Metadata::Namespace).text().not_null())
                .col(ColumnDef::new(Metadata::NamespaceCustom).text())
                .col(ColumnDef::new(Metadata::Key).text().not_null())
                .col(ColumnDef::new(Metadata::Value).text().not_null())
                .col(ColumnDef::new(Metadata::ExtractedAt).text().not_null())
                .col(ColumnDef::new(Metadata::Partial).boolean().not_null().default(false))
                .primary_key(
                    Index::create()
                        .col(Metadata::FileKey)
                        .col(Metadata::Namespace)
                        .col(Metadata::Key)
                )
                .to_owned()
        ).await?;

        // ... 其余 12 张表（完整定义参见 schema.rs）

        // ── 创建索引 ──
        manager.create_index(Index::create().name("idx_files_file_type").table(File::Table).col(File::FileType).to_owned()).await?;
        manager.create_index(Index::create().name("idx_files_host_id").table(File::Table).col(File::HostId).to_owned()).await?;
        manager.create_index(Index::create().name("idx_files_last_modified").table(File::Table).col(File::LastModified).to_owned()).await?;
        manager.create_index(Index::create().name("idx_metadata_namespace").table(Metadata::Table).col(Metadata::Namespace).to_owned()).await?;
        manager.create_index(Index::create().name("idx_metadata_file_key").table(Metadata::Table).col(Metadata::FileKey).to_owned()).await?;
        manager.create_index(Index::create().name("idx_metadata_key_value").table(Metadata::Table).col(Metadata::Key).col(Metadata::Value).to_owned()).await?;
        // 注意：namespace_custom 列不单独建索引，因为查询时通常和 namespace 一起使用

        // 所有表定义完成后，执行 namespace_custom 数据迁移
        let db = manager.get_connection();
        db.execute_unprepared(
            "UPDATE metadata SET namespace_custom = namespace, namespace = 'custom' \
             WHERE namespace NOT IN ('exif', 'video', 'audio', 'general')"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(File::Table).to_owned()).await?;
        // ... 其余表
        Ok(())
    }
}
```

### 7.2 pool.rs 改造

```rust
pub async fn create_pool(path: &Path) -> Result<DatabaseConnection, S3GalleryError> {
    let url = format!("sqlite:{}?mode=rwc", path.display());
    Database::connect(&url).await.map_err(|e| {
        S3GalleryError::DbError(format!("Failed to connect to database: {e}"))
    })
}
```

### 7.3 全局替换：SqlitePool → DatabaseConnection

涉及文件：`view/*.rs`、`scan/*.rs`、`s3/*.rs`、`cmd_*.rs`、`tests/*.rs`

---

## 八、文件改动清单

### 新增文件

| 文件 | 内容 |
|:-----|:------|
| `entity/mod.rs` | 模块导出 |
| `entity/file.rs` | files 表 Entity |
| `entity/host_config.rs` | host_config 表 Entity |
| `entity/metadata.rs` | metadata 表 Entity |
| `entity/thumbnail.rs` | thumbnails 表 Entity |
| `entity/tag.rs` | tags 表 Entity |
| `entity/file_tag.rs` | file_tags 表 Entity |
| `entity/classification_rule.rs` | classification_rules 表 Entity |
| `entity/extractor_rule.rs` | extractor_rules 表 Entity |
| `entity/scan_metadata.rs` | scan_metadata 表 Entity |
| `entity/dir_size.rs` | dir_sizes 表 Entity |
| `entity/scan_object.rs` | scan_objects 表 Entity |
| `entity/traffic_log.rs` | traffic_log 表 Entity |
| `entity/traffic_file_log.rs` | traffic_file_log 表 Entity |
| `entity/traffic_stats.rs` | traffic_stats 表 Entity |
| `db/migrate.rs` | SeaORM Migration 替代 schema.rs |

### 修改文件

| 文件 | 改动 |
|:-----|:------|
| `types.rs` | 枚举加 `DeriveActiveEnum`，Newtype 加桥接宏，删除手工 `Display`/`FromStr` |
| `db/mod.rs` | 导出 entity，重导出 `create_pool` |
| `db/pool.rs` | 返回 `DatabaseConnection` 替代 `SqlitePool` |
| `db/schema.rs` | 删除（迁移到 `db/migrate.rs`） |
| `db/models.rs` | 删除（Entity 替代） |
| `view/*.rs` | 全部改为 SeaORM 查询 |
| `scan/*.rs` | 替换 `SqlitePool` 为 `DatabaseConnection` |
| `s3/traffic_persist.rs` | 替换 pool 类型 |
| `s3/traffic_recorder.rs` | 删除 `S3Operation` 枚举定义（移入 `types.rs`） |
| `s3/config.rs` | 删除 `HostIdentifier.host_type`（由 Entity 管理） |
| `cmd_*.rs` | 替换 pool 类型 |
| `tests/*.rs` | 替换 pool 类型 |
| `Cargo.toml` | 添加 `sea-orm`、`sea-orm-migration` 依赖 |

---

## 九、实施顺序

1. **依赖和 Entity 定义** — 添加 sea-orm 依赖，创建 14 个 Entity 文件
2. **类型改造** — types.rs 加 DeriveActiveEnum，Newtype 桥接宏
3. **DB 迁移** — 创建 migrate.rs，替换 schema.rs，pool.rs 改造
4. **view 模块迁移** — 8 个 view 文件改为 SeaORM 查询
5. **scan/s3 模块适配** — 替换 SqlitePool 为 DatabaseConnection
6. **CLI 和测试适配** — cmd_*.rs、tests/*.rs 适配
7. **编译修复和测试** — 修复所有编译错误，确保 277+ 测试通过

> **`models.rs` 在实施步骤 7 中删除**，确保所有调用者已迁移完毕后再删除。

---

## 十、附录：string_value 与现有 DB 值对照表

| 枚举 | 变体 | 现有 Display | 现有 DB 值 | string_value | 匹配? |
|:-----|:-----|:------------|:-----------|:-------------|:-----:|
| FileType::Jpeg | `"jpeg"` | `"jpeg"` | `"jpeg"` | ✅ |
| FileType::TarGz | `"tar_gz"` | `"tar_gz"` | `"tar_gz"` | ✅ |
| FileType::SevenZ | `"seven_z"` | `"seven_z"` | `"seven_z"` | ✅ |
| MetadataState::Pending | `"pending"` | `"pending"` | `"pending"` | ✅ |
| MetadataState::Extracted | `"extracted"` | `"extracted"` | `"extracted"` | ✅ |
| MetadataState::Failed | `"failed"` | `"failed"` | `"failed"` | ✅ |
| MetadataNamespace::Exif | `"exif"` | `"exif"` | `"exif"` | ✅ |
| MetadataNamespace::Custom | `"XMP"` (内部值) | `"custom"` | `"custom"` | ⚠️ 需数据迁移 |
| S3Operation::GetObject | `"GetObject"` | `"GetObject"` | `"GetObject"` | ✅ |
| S3Operation::ObjectExists | `"ObjectExists"` | `"ObjectExists"` | `"ObjectExists"` | ✅ |
| Direction::Download | 新增 | `"download"` | `"download"` | ✅ |
| Direction::Upload | 新增 | `"upload"` | `"upload"` | ✅ |
| TagType::Auto | 新增 | `"auto"` | `"auto"` | ✅ |
| TagType::Manual | 新增 | `"manual"` | `"manual"` | ✅ |

### 需同步修改的测试

| 测试 | 文件 | 行 | 原因 |
|:-----|:-----|:--:|:------|
| `metadata_namespace_from_str_invalid` | `types.rs` | 1607 | 当前断言 `"custom"` 无效，迁移后 `DeriveActiveEnum` 自动接受 `"custom"`→ `Custom`，需删除此测试 |
| `bucket_name_too_short` | `types.rs` | 1029 | 使用 `.unwrap_err()`，迁移后 `BucketName` 等类型保持 `FromStr`，无需改动 |
| `metadata_namespace_from_str_invalid` | `types.rs` | 1607 | 同上，需删除或改为测试有效路径 |