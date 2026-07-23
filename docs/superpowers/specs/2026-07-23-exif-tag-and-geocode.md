# EXIF 标签 + GPS 逆地理编码设计文档

**目标：**
1. 有 EXIF 数据的文件自动打上 `exif:yes` 标签
2. GPS 坐标通过离线行政区划数据库翻译为地名，生成 `location:xxx` 标签

---

## 架构

```
扫描器 process_file_metadata()
  ├─ 提取 EXIF metadata → 存入 metadata 表
  ├─ TagRule 引擎生成标签（焦距、光圈、ISO 等）
  ├─ 添加 exif:yes 标签（有 EXIF 数据即打）
  ├─ GPS 坐标 → reverse_geocode() → 行政区划名称
  │     └─ 嵌入的 JSON 行政区划数据（省市区三级）
  └─ 生成 location:北京市 / location:海淀区 标签
```

---

## Section 1: `exif:yes` 标签

**文件：** `crates/s3-gallery-core/src/scan/scanner.rs`

在 `process_file_metadata` 函数中，成功提取 metadata 并存储后，添加 `exif:yes` 标签：

```rust
// 在 tags 生成循环之后添加
let exif_tag_id = TagEntry::ensure_exists(&config.db, "exif:yes", "auto").await?;
FileTagEntry::insert(&config.db, &FileTagEntry {
    file_key: key.to_string(),
    tag_id: exif_tag_id,
}).await?;
```

---

## Section 2: GPS 逆地理编码

### 2.1 行政区划数据

**来源：** 开源中国行政区划数据（省市区三级），包含名称和 GPS 中心点坐标。
推荐使用 `modood/Administrative-divisions-of-China` 的 `json/area.json` 数据，
或 `xiangyuecn/AreaCity-JsSpider-StatsGov` 的省市区三级数据。

**数据准备：** 从开源项目下载原始数据，转换为 `{name, lat, lon, city, province}` 格式的扁平 JSON 数组。使用 `serde_json` 反序列化。

**格式：** 嵌入 JSON 文件 `crates/s3-gallery-core/src/extractor/geocode_data.json`

```json
[
  {"name": "东城区", "lat": 39.9283, "lon": 116.4163, "city": "北京市", "province": "北京市"},
  {"name": "海淀区", "lat": 39.9597, "lon": 116.2983, "city": "北京市", "province": "北京市"}
]
```

**数据量：** ~3000 条区县级记录，约 150-200KB JSON。

### 2.2 逆地理编码模块

**文件：** `crates/s3-gallery-core/src/extractor/geocode.rs`

```rust
/// 行政区划条目
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Division {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    pub province: String,
}

/// 地名结果
#[derive(Debug, Clone)]
pub struct Location {
    pub district: String,   // 区县名，如 "海淀区"
    pub city: String,       // 城市名，如 "北京市"
    pub province: String,   // 省份名，如 "北京市"
}

/// 逆地理编码器
pub struct Geocoder {
    divisions: Vec<Division>,
}

impl Geocoder {
    /// 从嵌入的 JSON 数据加载行政区划
    pub fn from_embedded() -> Self { ... }

    /// 给定 GPS 坐标，返回最近行政区划的地名
    pub fn reverse_geocode(&self, lat: f64, lon: f64) -> Option<Location> {
        // 使用 Haversine 公式计算距离
        // 线性扫描所有记录，找到最近的一条
        // 仅在地理包围盒内匹配（经纬度差 < 1°）
    }
}
```

### 2.3 扫描器集成

在 `process_file_metadata` 中，提取 GPS 坐标后调用逆地理编码：

```rust
// 从 metadata 中提取 GPS 坐标
let gps_lat = items.iter()
    .find(|m| m.key == "GPSLatitude")
    .map(|m| parse_dms(&m.value));
let gps_lon = items.iter()
    .find(|m| m.key == "GPSLongitude")
    .map(|m| parse_dms(&m.value));

if let (Some(lat), Some(lon)) = (gps_lat, gps_lon) {
    let geocoder = Geocoder::from_embedded();
    if let Some(location) = geocoder.reverse_geocode(lat, lon) {
        // 生成 location:北京市 标签
        let tag_id = TagEntry::ensure_exists(&config.db, &format!("location:{}", location.city), "auto").await?;
        FileTagEntry::insert(&config.db, &FileTagEntry {
            file_key: key.to_string(),
            tag_id,
        }).await?;
        // 可选：location:海淀区（更精确）
        let tag_id = TagEntry::ensure_exists(&config.db, &format!("location:{}", location.district), "auto").await?;
        FileTagEntry::insert(&config.db, &FileTagEntry {
            file_key: key.to_string(),
            tag_id,
        }).await?;
    }
}
```

---

## 涉及文件

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/s3-gallery-core/src/scan/scanner.rs` | 修改 | 添加 `exif:yes` 标签 + GPS 逆地理编码调用 |
| `crates/s3-gallery-core/src/extractor/geocode.rs` | **新增** | 逆地理编码模块 |
| `crates/s3-gallery-core/src/extractor/geocode_data.json` | **新增** | 行政区划数据 |
| `crates/s3-gallery-core/src/extractor/mod.rs` | 修改 | 注册 `geocode` 模块 |

---

## 验证方案

1. 单元测试：`Geocoder::reverse_geocode()` 对已知坐标返回正确地名
2. 扫描测试：对包含 GPS 的图片扫描后，验证 `location:xxx` 标签正确生成
3. `exif:yes` 标签：验证有 EXIF 数据的文件都有此标签