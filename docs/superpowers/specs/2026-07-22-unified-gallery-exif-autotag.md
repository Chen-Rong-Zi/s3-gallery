# 统一 Timeline-Gallery + EXIF 自动标签设计文档

**目标：**
1. 合并 Gallery 和 Timeline 页面，按日期分组展示所有文件，图片显示缩略图，支持 tag 过滤，带无限滚动
2. 扫描时用 range 请求下载文件头部 64KB 提取 EXIF，自动生成标签存入 tags 表
3. 采用 FP 风格、数据驱动的 TagRule 规则引擎，易于扩展
4. 引入 `effective_date` 字段，时间线使用 EXIF `DateTimeOriginal`（实际拍摄时间）而非 OSS `last_modified`（上传时间）

---

## 架构概览

```
┌─ 扫描器 ──────────────────────────────────────────────┐
│  Step 6: 对每个新/变更文件                              │
│   → get_object_range(0..64KB) 下载头部                  │
│   → ExtractorRegistry 提取元数据                        │
│   → 存入 metadata 表                                   │
│   → TagRule 引擎从 metadata 生成标签                     │
│   → 存入 tags + file_tags 表                           │
│   → 更新 effective_date + metadata_state               │
└───────────────────────────────────────────────────────┘

┌─ 统一页面 ─────────────────────────────────────────────┐
│  GET /gallery?page=0&tag=xxx                           │
│   → 核心视图函数: get_timeline_gallery()                │
│   → 按 effective_date 分组 + tag 过滤 + 分页            │
│   → 渲染: 日期标题 + 缩略图/图标网格                    │
│   → 支持 HTMX 无限滚动                                 │
└───────────────────────────────────────────────────────┘
```

---

## Section 1: 统一 Timeline-Gallery 页面

### 核心视图函数

**文件:** `crates/s3-gallery-core/src/view/timeline_gallery.rs`（新增）

```rust
pub async fn get_timeline_gallery(
    db: &SqlitePool,
    host_id: Option<&str>,
    page: u32,
    page_size: u32,
    tag: Option<&str>,
) -> Result<(Vec<TimelineEntry>, bool)>
```

**查询逻辑：**
- 基础：`SELECT * FROM files WHERE is_deleted = 0`
- 可选 host_id 过滤
- 可选 tag 过滤：`INNER JOIN file_tags ft ON f.key = ft.file_key INNER JOIN tags t ON ft.tag_id = t.tag_id WHERE t.tag_name = ?`
- 按 `effective_date DESC` 排序（使用 EXIF 拍摄时间或 fallback 到 `last_modified`）
- 按 `effective_date` 分组（YYYY-MM-DD）
- 分页：每页返回 `page_size` 个日期组，多取一个组判断 `has_more`

### 路由和 Handler

**文件:** `crates/s3-gallery-cli/src/web/handlers/gallery.rs`

修改现有 handler，接受新参数：
- `page`（默认 0）
- `tag`（可选，标签名）
- `host_id`（可选，参见之前的多 host 改造）

**路由器:** `crates/s3-gallery-cli/src/web/router.rs`
- 保留 `GET /gallery`
- 移除 `GET /timeline` 路由

### 模板

**`gallery.html`**（替换现有）：过滤栏 + 按日期分组展示

```
┌─ Filter ───────────────────────────────┐
│  Tag: [▼ 全部 / 4:3 / 16:9 / Canon...]│
│  Type: [▼ 全部 / image / video / ...]  │
└────────────────────────────────────────┘
┌─ 2024-07-22 ───────────────────────────┐
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐  │
│  │ 📷   │ │ 📷   │ │ 📄   │ │ 🎬   │  │
│  │ photo│ │ photo│ │ doc  │ │ video│  │
│  └──────┘ └──────┘ └──────┘ └──────┘  │
├────────────────────────────────────────┤
│  [Load more...] (HTMX 无限滚动)        │
└────────────────────────────────────────┘
```

**`gallery_items.html`**（HTMX partial）：仅日期组 + 文件网格，不包含过滤栏

### 导航栏

- 移除独立的 Timeline 链接
- 保留 Gallery 链接

---

## Section 2: EXIF 提取 + 自动标签

### `effective_date` 字段

**为什么需要：** OSS 的 `last_modified` 是文件上传/修改时间，照片的 EXIF `DateTimeOriginal` 是实际拍摄时间。两者经常不一致（如 2020 年拍摄的照片 2024 年才上传）。时间线应使用**实际拍摄时间**。

**实现方式：** 在 `files` 表增加 `effective_date TEXT NOT NULL DEFAULT ''` 列：
- 扫描时设置：`effective_date = EXIF.DateTimeOriginal[..10] 或 last_modified[..10]`
- 时间线视图：`GROUP BY effective_date` 而非 `last_modified`
- migration：`ALTER TABLE files ADD COLUMN effective_date TEXT NOT NULL DEFAULT ''`

### 扫描器 Step 6 实现

**文件:** `crates/s3-gallery-core/src/scan/scanner.rs`

在 `run_scan()` 的 Step 6 中：

```rust
// Step 6: Extract metadata and generate tags
let mut metadata_extracted = 0u64;
if config.extract_metadata {
    let mut registry = ExtractorRegistry::new();
    registry.register(Box::new(ExifExtractor::new()));
    let tag_rules = TagRule::default_rules();

    for obj in diff.new_objects.iter().chain(diff.changed_objects.iter()) {
        if let Ok(count) = process_file_metadata(&config, &registry, &tag_rules, obj).await {
            metadata_extracted += count;
        }
    }
}
```

**`process_file_metadata` 辅助函数：**
```rust
async fn process_file_metadata(
    config: &ScanConfig,
    registry: &ExtractorRegistry,
    tag_rules: &[TagRule],
    obj: &ObjectSummary,
) -> Result<u64> {
    let key = obj.key.as_str();
    let file_name = key.rsplit('/').next().unwrap_or(key);
    let ext = match parse_extension(file_name) { Some(e) => e, None => return Ok(0) };
    let file_type = classify_extension(&ext).to_string();

    // 检查是否有支持提取器
    if registry.find(&file_type, ext.as_str()).is_empty() { return Ok(0); }

    // 只下载头部 64KB
    let data = config.s3.get_object_range(&config.bucket, &obj.key, 0, 65536).await?;
    let items = registry.extract_all(&data, &file_type, ext.as_str()).await?;
    if items.is_empty() { return Ok(0); }

    // 存入 metadata 表
    for item in &items {
        MetadataEntry::insert(&config.db, &MetadataEntry {
            file_key: key.to_string(),
            namespace: item.namespace,
            key: item.key.clone(),
            value: item.value.clone(),
            extracted_at: Utc::now().to_rfc3339(),
            partial: false,
        }).await?;
    }

    // 规则引擎生成标签
    let tags = evaluate_all(tag_rules, &items, &file_type);
    for tag in &tags {
        ensure_tag_exists(&config.db, &tag.tag_name, &tag.tag_type).await?;
        let tag_entry = TagEntry::get_by_name(&config.db, &tag.tag_name).await?;
        FileTagEntry::insert(&config.db, &FileTagEntry {
            file_key: key.to_string(),
            tag_id: tag_entry.tag_id,
        }).await?;
    }

    // 更新 effective_date
    let exif_date = items.iter()
        .find(|m| m.key == "DateTimeOriginal" || m.key == "DateTimeDigitized")
        .map(|m| m.value.as_str())
        .and_then(|v| v.get(..10));
    let effective_date = exif_date.unwrap_or(&obj.last_modified[..10.min(obj.last_modified.len())]);
    sqlx::query("UPDATE files SET effective_date = ? WHERE host_id = ? AND key = ?")
        .bind(effective_date).bind(&config.host_id).bind(key)
        .execute(&config.db).await?;

    // 更新 metadata_state
    sqlx::query("UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?")
        .bind(&config.host_id).bind(key)
        .execute(&config.db).await?;

    Ok(1)
}
```

### TagRule 规则引擎

**文件:** `crates/s3-gallery-core/src/extractor/tag_rules.rs`（新增）

```rust
/// 标签候选
pub struct TagCandidate {
    pub tag_name: String,
    pub tag_type: String,
}

/// 值转换方式
pub enum TagValue {
    /// 直接模板替换: {0}, {1} 对应 keys 提取的值
    Pattern(&'static str),
    /// 自定义纯函数转换
    Compute(fn(&[&str]) -> String),
}

/// 规则定义 = 纯数据
pub struct TagRule {
    /// 标签前缀，如 "camera" → "camera:Canon"
    pub prefix: &'static str,
    /// 需要读取的 metadata key 列表
    pub keys: &'static [&'static str],
    /// 值转换方式
    pub value: TagValue,
    /// 文件类型过滤（如 Some("jpeg") 只处理 jpeg）
    pub if_file_type: Option<&'static str>,
}
```

**核心纯函数：**
```rust
pub fn evaluate_rule(rule: &TagRule, metadata: &[MetadataItem], file_type: &str) -> Option<TagCandidate> {
    if let Some(ft) = rule.if_file_type { if file_type != ft { return None; } }
    let values: Vec<&str> = rule.keys.iter()
        .filter_map(|k| metadata.iter().find(|m| m.key == *k).map(|m| m.value.as_str()))
        .collect();
    if values.len() != rule.keys.len() { return None; }
    let tag_value = match &rule.value {
        TagValue::Pattern(p) => {
            let mut s = p.to_string();
            for (i, v) in values.iter().enumerate() { s = s.replace(&format!("{{{i}}}"), v); }
            s
        }
        TagValue::Compute(f) => f(&values),
    };
    let tag_name = if rule.prefix.is_empty() { tag_value } else { format!("{}:{}", rule.prefix, tag_value) };
    Some(TagCandidate { tag_name, tag_type: "auto".to_string() })
}

pub fn evaluate_all(rules: &[TagRule], metadata: &[MetadataItem], file_type: &str) -> Vec<TagCandidate> {
    rules.iter().filter_map(|r| evaluate_rule(r, metadata, file_type)).collect()
}
```

### 完整 TagRule 规则集

所有规则以数据方式定义在 `default_rules()` 中，每条规则一行。

```rust
pub fn default_rules() -> Vec<TagRule> {
    use TagValue::*;
    vec![
        // ========== 基础文件类型分类 ==========
        rule("",   &["file_type"], Pattern("image"),  Some("jpeg")),
        rule("",   &["file_type"], Pattern("image"),  Some("png")),
        rule("",   &["file_type"], Pattern("image"),  Some("webp")),
        rule("",   &["file_type"], Pattern("image"),  Some("bmp")),
        rule("",   &["file_type"], Pattern("image"),  Some("tiff")),
        rule("",   &["file_type"], Pattern("image"),  Some("svg")),
        rule("",   &["file_type"], Pattern("video"),  Some("mp4")),
        rule("",   &["file_type"], Pattern("video"),  Some("mov")),
        rule("",   &["file_type"], Pattern("video"),  Some("avi")),
        rule("",   &["file_type"], Pattern("video"),  Some("mkv")),
        rule("",   &["file_type"], Pattern("audio"),  Some("mp3")),
        rule("",   &["file_type"], Pattern("audio"),  Some("flac")),
        rule("",   &["file_type"], Pattern("audio"),  Some("wav")),
        rule("",   &["file_type"], Pattern("document"), Some("pdf")),
        rule("",   &["file_type"], Pattern("document"), Some("doc")),
        rule("",   &["file_type"], Pattern("document"), Some("docx")),
        rule("",   &["file_type"], Pattern("document"), Some("xls")),
        rule("",   &["file_type"], Pattern("document"), Some("pptx")),

        // ========== 相机信息（已离散）==========
        rule("camera",  &["Make"],       Pattern("{0}"), None),  // camera:Canon
        rule("model",   &["Model"],      Pattern("{0}"), None),  // model:EOS_R5
        rule("lens",    &["LensModel"],  Pattern("{0}"), None),  // lens:EF_24-70mm
        rule("lens_make", &["LensMake"], Pattern("{0}"), None),
        rule("owner",    &["CameraOwnerName"], Pattern("{0}"), None),
        rule("serial",   &["BodySerialNumber"], Pattern("{0}"), None),
        rule("software", &["Software"],  Pattern("{0}"), None),  // software:Adobe_Photoshop
        rule("artist",   &["Artist"],    Pattern("{0}"), None),
        rule("copyright",&["Copyright"], Pattern("{0}"), None),

        // ========== 拍摄参数（已离散）==========
        rule("program",  &["ExposureProgram"], Pattern("{0}"), None),  // program:manual
        rule("exposure", &["ExposureMode"],    Pattern("{0}"), None),  // exposure:auto
        rule("metering", &["MeteringMode"],    Pattern("{0}"), None),  // metering:spot
        rule("light",    &["LightSource"],     Pattern("{0}"), None),  // light:daylight
        rule("whitebalance", &["WhiteBalance"],Pattern("{0}"), None),  // whitebalance:auto
        rule("scene",    &["SceneCaptureType"],Pattern("{0}"), None),  // scene:portrait
        rule("flash",    &["Flash"],  Pattern("fired"),   None, "fired"),   // flash:fired
        rule("flash",    &["Flash"],  Pattern("not_fired"), None, "not fired"), // flash:not_fired
        rule("colorspace", &["ColorSpace"], Pattern("{0}"), None),  // colorspace:sRGB
        rule("contrast", &["Contrast"],  Pattern("{0}"), None),     // contrast:normal
        rule("saturation",&["Saturation"],Pattern("{0}"), None),    // saturation:normal
        rule("sharpness",&["Sharpness"], Pattern("{0}"), None),     // sharpness:normal
        rule("gain",     &["GainControl"],Pattern("{0}"), None),    // gain:none
        rule("distance", &["SubjectDistanceRange"], Pattern("{0}"), None), // distance:macro
        rule("sensor",   &["SensingMethod"],  Pattern("{0}"), None), // sensor:one_chip
        rule("source",   &["FileSource"],     Pattern("{0}"), None), // source:digital_camera
        rule("custom",   &["CustomRendered"], Pattern("{0}"), None),
        rule("composite",&["CompositeImage"], Pattern("{0}"), None),
        rule("compression", &["Compression"], Pattern("{0}"), None),

        // ========== 连续→离散：焦距分类 ==========
        rule("focal", &["FocalLengthIn35mmFilm"], Compute(|vals| {
            let f: f64 = vals[0].parse().unwrap_or(0.0);
            match f {
                _ if f < 20.0 => "ultrawide",
                _ if f < 35.0 => "wide",
                _ if f < 70.0 => "normal",
                _ if f < 200.0 => "tele",
                _ => "supertele",
            }.to_string()
        }), None),
        // 下备用 FocalLength（如果 FocalLengthIn35mmFilm 不存在）
        rule("focal", &["FocalLength"], Compute(|vals| {
            let f: f64 = vals[0].parse().unwrap_or(0.0);
            match f {
                _ if f < 20.0 => "ultrawide",
                _ if f < 35.0 => "wide",
                _ if f < 70.0 => "normal",
                _ if f < 200.0 => "tele",
                _ => "supertele",
            }.to_string()
        }), None),

        // ========== 连续→离散：光圈分类 ==========
        rule("aperture", &["FNumber"], Compute(|vals| {
            let f: f64 = vals[0].parse().unwrap_or(0.0);
            match f {
                _ if f <= 2.0 => "fast",
                _ if f <= 2.8 => "bright",
                _ if f <= 5.6 => "medium",
                _ => "narrow",
            }.to_string()
        }), None),

        // ========== 连续→离散：快门分类 ==========
        rule("shutter", &["ExposureTime"], Compute(|vals| {
            // 值可能是 "1/125" 或 "0.5" 等格式
            let t = parse_exposure_time(&vals[0]);
            match t {
                _ if t > 1.0 => "long",
                _ if t > 1.0/30.0 => "slow",
                _ if t > 1.0/250.0 => "normal",
                _ if t > 1.0/4000.0 => "fast",
                _ => "ultrafast",
            }.to_string()
        }), None),

        // ========== 连续→离散：ISO 分类 ==========
        rule("iso", &["PhotographicSensitivity", "ISOSpeed"], Compute(|vals| {
            let iso: f64 = vals[0].parse().unwrap_or(0.0);
            match iso {
                _ if iso < 200.0 => "low",
                _ if iso < 800.0 => "medium",
                _ if iso < 6400.0 => "high",
                _ => "extreme",
            }.to_string()
        }), None),

        // ========== 连续→离散：曝光补偿 ==========
        rule("exposure_bias", &["ExposureBiasValue"], Compute(|vals| {
            let bias: f64 = vals[0].parse().unwrap_or(0.0);
            match bias {
                _ if bias < -0.5 => "negative",
                _ if bias > 0.5 => "positive",
                _ => "normal",
            }.to_string()
        }), None),

        // ========== 连续→离散：亮度 ==========
        rule("brightness", &["BrightnessValue"], Compute(|vals| {
            let bv: f64 = vals[0].parse().unwrap_or(0.0);
            match bv {
                _ if bv < 0.0 => "dark",
                _ if bv < 5.0 => "dim",
                _ if bv < 10.0 => "normal",
                _ => "bright",
            }.to_string()
        }), None),

        // ========== 连续→离散：拍摄距离 ==========
        rule("distance", &["SubjectDistance"], Compute(|vals| {
            let d: f64 = vals[0].parse().unwrap_or(0.0);
            match d {
                _ if d < 0.3 => "macro",
                _ if d < 3.0 => "near",
                _ if d < 20.0 => "distant",
                _ => "infinity",
            }.to_string()
        }), None),

        // ========== 连续→离散：数码变焦 ==========
        rule("digital_zoom", &["DigitalZoomRatio"], Compute(|vals| {
            let z: f64 = vals[0].parse().unwrap_or(1.0);
            match z {
                _ if z <= 1.0 => "none",
                _ if z <= 2.0 => "moderate",
                _ => "heavy",
            }.to_string()
        }), None),

        // ========== 连续→离散：宽高比 ==========
        rule("aspect", &["PixelXDimension", "PixelYDimension"], Compute(|vals| {
            let w: u32 = vals[0].parse().unwrap_or(1);
            let h: u32 = vals[1].parse().unwrap_or(1);
            let g = gcd(w, h);
            format!("{}:{}", w / g, h / g)
        }), None),

        // ========== 连续→离散：分辨率 ==========
        rule("resolution", &["XResolution"], Compute(|vals| {
            let dpi: f64 = vals[0].parse().unwrap_or(0.0);
            match dpi {
                _ if dpi < 150.0 => "draft",
                _ if dpi < 300.0 => "standard",
                _ => "high",
            }.to_string()
        }), None),

        // ========== 连续→离散：时间维度 ==========
        rule("year",  &["DateTimeOriginal"], Compute(|vals| vals[0].get(..4).unwrap_or("").to_string()), None),
        rule("month", &["DateTimeOriginal"], Compute(|vals| vals[0].get(..7).unwrap_or("").to_string()), None),
        rule("season", &["DateTimeOriginal"], Compute(|vals| {
            let m: u32 = vals[0].get(5..7).and_then(|s| s.parse().ok()).unwrap_or(0);
            match m { 3..=5 => "spring", 6..=8 => "summer",
                      9..=11 => "autumn", _ => "winter" }.to_string()
        }), None),
        rule("timeofday", &["DateTimeOriginal"], Compute(|vals| {
            let h: u32 = vals[0].get(11..13).and_then(|s| s.parse().ok()).unwrap_or(0);
            match h { 4..=6 => "dawn", 7..=10 => "morning", 11..=12 => "midday",
                      13..=16 => "afternoon", 17..=19 => "dusk", _ => "night" }.to_string()
        }), None),

        // ========== 连续→离散：环境参数 ==========
        rule("temperature", &["Temperature"], Compute(|vals| {
            let t: f64 = vals[0].parse().unwrap_or(0.0);
            match t { _ if t < 5.0 => "cold", _ if t < 20.0 => "mild",
                      _ if t < 30.0 => "warm", _ => "hot" }.to_string()
        }), None),
        rule("humidity", &["Humidity"], Compute(|vals| {
            let h: f64 = vals[0].parse().unwrap_or(50.0);
            match h { _ if h < 30.0 => "dry", _ if h < 70.0 => "normal", _ => "humid" }.to_string()
        }), None),
        rule("pressure", &["Pressure"], Compute(|vals| {
            let p: f64 = vals[0].parse().unwrap_or(1013.0);
            match p { _ if p < 1000.0 => "low", _ if p < 1020.0 => "normal", _ => "high" }.to_string()
        }), None),

        // ========== GPS 位置 ==========
        rule("grid", &["GPSLatitude", "GPSLongitude"], Compute(|vals| {
            // 四舍五入到 0.01° ≈ 1km 网格
            let lat = parse_dms(&vals[0]);
            let lon = parse_dms(&vals[1]);
            format!("{:.2}_{:.2}", (lat * 100.0).round() / 100.0,
                                    (lon * 100.0).round() / 100.0)
        }), None),
        rule("altitude", &["GPSAltitude"], Compute(|vals| {
            let a: f64 = vals[0].parse().unwrap_or(0.0);
            match a { _ if a < 50.0 => "sea_level", _ if a < 500.0 => "low",
                      _ if a < 2000.0 => "medium", _ => "high" }.to_string()
        }), None),
        rule("direction", &["GPSImgDirection"], Compute(|vals| {
            let d: f64 = vals[0].parse().unwrap_or(0.0);
            let dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
            dirs[((d / 45.0).round() as usize) % 8].to_string()
        }), None),
        rule("speed", &["GPSSpeed"], Compute(|vals| {
            let s: f64 = vals[0].parse().unwrap_or(0.0);
            match s { _ if s < 1.0 => "stationary", _ if s < 6.0 => "walking",
                      _ if s < 15.0 => "running", _ if s < 120.0 => "driving",
                      _ => "flying" }.to_string()
        }), None),
        rule("gps_accuracy", &["GPSDOP"], Compute(|vals| {
            let d: f64 = vals[0].parse().unwrap_or(99.0);
            match d { _ if d < 2.0 => "excellent", _ if d < 5.0 => "good",
                      _ if d < 10.0 => "moderate", _ => "poor" }.to_string()
        }), None),
    ]
}
```

> `rule()` 是一个简化构造辅助函数，相当于 `TagRule { prefix, keys, value, if_file_type }` 的简写。对于 `Flash` 字段的特殊处理，使用带值匹配的变体。

### 辅助函数

```rust
/// 解析 EXIF ExposureTime 显示值，返回秒数
fn parse_exposure_time(s: &str) -> f64 {
    if let Some(denom) = s.strip_prefix("1/") {
        denom.parse::<f64>().map(|d| 1.0 / d).unwrap_or(0.0)
    } else {
        s.parse::<f64>().unwrap_or(0.0)
    }
}

/// 解析 GPS DMS 格式（"34 deg 42 min 12.5 sec"）为十进制度数
fn parse_dms(s: &str) -> f64 {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 5 {
        let deg: f64 = parts[0].parse().unwrap_or(0.0);
        let min: f64 = parts[2].parse().unwrap_or(0.0);
        let sec: f64 = parts[4].parse().unwrap_or(0.0);
        deg + min / 60.0 + sec / 3600.0
    } else {
        s.parse().unwrap_or(0.0)
    }
}

fn gcd(a: u32, b: u32) -> u32 { if b == 0 { a } else { gcd(b, a % b) } }
```

### 模型方法补充

**`TagEntry::ensure_exists()`** — 如果 tag 不存在则创建，返回 tag_id：
```rust
pub async fn ensure_exists(pool: &SqlitePool, tag_name: &str, tag_type: &str) -> Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO tags (tag_name, tag_type) VALUES (?, ?)")
        .bind(tag_name).bind(tag_type).execute(pool).await?;
    let row: (i64,) = sqlx::query_as("SELECT tag_id FROM tags WHERE tag_name = ?")
        .bind(tag_name).fetch_one(pool).await?;
    Ok(row.0)
}
```

---

## 涉及文件清单

### 新增文件
- `crates/s3-gallery-core/src/extractor/tag_rules.rs` — TagRule 引擎 + 完整规则集
- `crates/s3-gallery-core/src/view/timeline_gallery.rs` — 核心视图

### 修改文件
- `crates/s3-gallery-core/src/scan/scanner.rs` — Step 6 实现
- `crates/s3-gallery-core/src/db/models.rs` — 添加 `effective_date` 字段、`ensure_exists()`
- `crates/s3-gallery-core/src/db/schema.rs` — 添加 `ALTER TABLE files ADD COLUMN effective_date` migration
- `crates/s3-gallery-cli/src/web/handlers/gallery.rs` — 替换为统一页面 handler
- `crates/s3-gallery-cli/src/web/router.rs` — 移除 timeline 路由
- `crates/s3-gallery-web/templates/gallery.html` — 替换为统一模板
- `crates/s3-gallery-web/templates/gallery_items.html` — 替换 HTMX partial
- `crates/s3-gallery-web/templates/layout.html` — 移除 Timeline 导航链接

### 可删除文件
- `crates/s3-gallery-cli/src/web/handlers/timeline.rs` — 不再需要独立 handler
- `crates/s3-gallery-web/templates/timeline.html` — 不再需要独立模板
- `crates/s3-gallery-core/src/view/timeline.rs` — 不再需要独立视图

---

## 验证方案

1. **单元测试：** `TagRule::evaluate_rule()` 测试各种规则，包括连续→离散的边界值
2. **集成测试：** 扫描器 mock S3 测试，验证 metadata 表、tags 表、effective_date 正确写入
3. **手动测试：** `s3-gallery scan init --extract-metadata` + `s3-gallery serve`
4. **页面验证：** 统一页面按日期分组、缩略图、tag 过滤、无限滚动正常工作