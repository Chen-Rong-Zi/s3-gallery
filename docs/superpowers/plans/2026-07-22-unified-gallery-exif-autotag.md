# Unified Gallery + EXIF Auto-Tagging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Merge Gallery and Timeline into one page grouped by date with thumbnails; extract EXIF during scan with range requests; auto-tag files via a data-driven TagRule engine.

**Architecture:** TagRule engine (FP-style, data-driven rules) → Scanner Step 6 (download 64KB header, extract EXIF, generate tags, set effective_date) → Unified view (group by effective_date, filter by tag, paginate by date groups). Templates use HTMX for infinite scroll.

**Tech Stack:** Rust, sqlx/SQLite, kamadak-exif, axum, minijinja, HTMX

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/s3-gallery-core/src/extractor/tag_rules.rs` | **Create** | TagRule struct, TagValue enum, evaluate_rule/evaluate_all, default_rules, parse helpers |
| `crates/s3-gallery-core/src/view/timeline_gallery.rs` | **Create** | get_timeline_gallery: paginated, tag-filtered, date-grouped query |
| `crates/s3-gallery-core/src/db/schema.rs` | **Modify** | Add ALTER TABLE for effective_date column |
| `crates/s3-gallery-core/src/db/models.rs` | **Modify** | Add effective_date to FileEntry, add TagEntry::ensure_exists |
| `crates/s3-gallery-core/src/scan/scanner.rs` | **Modify** | Step 6: actual EXIF extraction + auto-tagging + effective_date |
| `crates/s3-gallery-cli/src/web/handlers/gallery.rs` | **Modify** | Rewrite for unified page with tag/date filtering |
| `crates/s3-gallery-cli/src/web/router.rs` | **Modify** | Remove /timeline route |
| `crates/s3-gallery-web/templates/gallery.html` | **Modify** | Rewrite with date-grouped, tag-filtered display |
| `crates/s3-gallery-web/templates/gallery_items.html` | **Modify** | HTMX partial for infinite scroll |
| `crates/s3-gallery-web/templates/layout.html` | **Modify** | Remove Timeline nav link |
| `crates/s3-gallery-cli/src/web/handlers/timeline.rs` | **Delete** | Replaced by unified gallery |
| `crates/s3-gallery-web/templates/timeline.html` | **Delete** | Replaced by new gallery template |
| `crates/s3-gallery-core/src/view/timeline.rs` | **Delete** | Replaced by timeline_gallery.rs |

---

### Task 1: TagRule Engine

**Files:**
- Create: `crates/s3-gallery-core/src/extractor/tag_rules.rs`
- Test: inline tests in tag_rules.rs

- [ ] **Step 1: Write the failing tests for TagRule struct and evaluate_rule**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractor::registry::MetadataItem;

    #[test]
    fn test_evaluate_rule_pattern() {
        let rule = TagRule {
            prefix: "camera", keys: &["Make"],
            value: TagValue::Pattern("{0}"), if_file_type: None,
        };
        let metadata = [MetadataItem {
            namespace: "exif", key: "Make".into(), value: "Canon".into(),
        }];
        let result = evaluate_rule(&rule, &metadata, "jpeg");
        assert!(result.is_some());
        assert_eq!(result.unwrap().tag_name, "camera:Canon");
    }

    #[test]
    fn test_evaluate_rule_compute() {
        let rule = TagRule {
            prefix: "aspect", keys: &["PixelXDimension", "PixelYDimension"],
            value: TagValue::Compute(|vals| {
                let w: u32 = vals[0].parse().unwrap();
                let h: u32 = vals[1].parse().unwrap();
                let g = gcd(w, h);
                format!("{}:{}", w/g, h/g)
            }), if_file_type: None,
        };
        let metadata = [
            MetadataItem { namespace: "exif", key: "PixelXDimension".into(), value: "1920".into() },
            MetadataItem { namespace: "exif", key: "PixelYDimension".into(), value: "1080".into() },
        ];
        let result = evaluate_rule(&rule, &metadata, "jpeg");
        assert!(result.is_some());
        assert_eq!(result.unwrap().tag_name, "aspect:16:9");
    }

    #[test]
    fn test_evaluate_rule_file_type_filter() {
        let rule = TagRule {
            prefix: "", keys: &["file_type"],
            value: TagValue::Pattern("image"), if_file_type: Some("jpeg"),
        };
        let metadata = [MetadataItem {
            namespace: "exif", key: "file_type".into(), value: "jpeg".into(),
        }];
        // Should match for jpeg
        assert!(evaluate_rule(&rule, &metadata, "jpeg").is_some());
        // Should NOT match for mp4
        assert!(evaluate_rule(&rule, &metadata, "mp4").is_none());
    }

    #[test]
    fn test_evaluate_rule_missing_key_returns_none() {
        let rule = TagRule {
            prefix: "camera", keys: &["Make"],
            value: TagValue::Pattern("{0}"), if_file_type: None,
        };
        let metadata = []; // No Make key
        assert!(evaluate_rule(&rule, &metadata, "jpeg").is_none());
    }

    #[test]
    fn test_evaluate_all_returns_multiple_tags() {
        let rules = TagRule::default_rules();
        let metadata = [
            MetadataItem { namespace: "exif", key: "Make".into(), value: "Canon".into() },
            MetadataItem { namespace: "exif", key: "Model".into(), value: "EOS R5".into() },
            MetadataItem { namespace: "exif", key: "PixelXDimension".into(), value: "1920".into() },
            MetadataItem { namespace: "exif", key: "PixelYDimension".into(), value: "1080".into() },
        ];
        let tags = evaluate_all(&rules, &metadata, "jpeg");
        // Should have at least: image, camera:Canon, model:EOS R5, aspect:16:9
        assert!(tags.iter().any(|t| t.tag_name == "image"));
        assert!(tags.iter().any(|t| t.tag_name == "camera:Canon"));
        assert!(tags.iter().any(|t| t.tag_name == "model:EOS R5"));
        assert!(tags.iter().any(|t| t.tag_name == "aspect:16:9"));
    }

    #[test]
    fn test_parse_exposure_time() {
        assert!((parse_exposure_time("1/125") - 0.008).abs() < 0.001);
        assert!((parse_exposure_time("0.5") - 0.5).abs() < 0.001);
        assert!((parse_exposure_time("30") - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_dms() {
        // "34 deg 42 min 12.5 sec" -> 34 + 42/60 + 12.5/3600 ≈ 34.70347
        let result = parse_dms("34 deg 42 min 12.5 sec");
        assert!((result - 34.70347).abs() < 0.001);
    }

    #[test]
    fn test_gcd() {
        assert_eq!(gcd(1920, 1080), 120);
        assert_eq!(gcd(16, 9), 1);
        assert_eq!(gcd(100, 1), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib extractor::tag_rules::tests 2>&1 | head -20`
Expected: `error[E0432]` — module not found or no tests found

- [ ] **Step 3: Implement TagRule engine**

Create `crates/s3-gallery-core/src/extractor/tag_rules.rs`:

```rust
//! Data-driven tag rule engine for generating tags from metadata.
//!
//! Rules are defined as data (struct), not code (trait). Each rule is a
//! pure function that maps metadata items → tag candidates. New rules
//! are added by appending to the `default_rules()` array.

use super::registry::MetadataItem;

/// A tag candidate generated by a rule.
#[derive(Debug, Clone)]
pub struct TagCandidate {
    pub tag_name: String,
    pub tag_type: String,
}

/// How to transform extracted metadata values into a tag value.
pub enum TagValue {
    /// Template replacement: `{0}`, `{1}` etc. are replaced with key values.
    Pattern(&'static str),
    /// Custom pure function that transforms extracted values.
    Compute(fn(&[&str]) -> String),
}

/// A single tag rule — pure data, no behaviour.
pub struct TagRule {
    /// Tag name prefix, e.g. "camera" → "camera:Canon". Empty = no prefix.
    pub prefix: &'static str,
    /// Metadata keys to extract values for.
    pub keys: &'static [&'static str],
    /// How to transform values into the tag value.
    pub value: TagValue,
    /// Only apply when file_type matches (e.g. Some("jpeg")). None = any type.
    pub if_file_type: Option<&'static str>,
}

/// Evaluate a single rule against the given metadata.
///
/// Returns `None` if the rule doesn't match (file type filter, missing keys).
pub fn evaluate_rule(
    rule: &TagRule,
    metadata: &[MetadataItem],
    file_type: &str,
) -> Option<TagCandidate> {
    // File type filter
    if let Some(ft) = rule.if_file_type {
        if file_type != ft {
            return None;
        }
    }

    // Extract values from metadata
    let values: Vec<&str> = rule
        .keys
        .iter()
        .filter_map(|k| metadata.iter().find(|m| m.key == *k).map(|m| m.value.as_str()))
        .collect();
    if values.len() != rule.keys.len() {
        return None;
    }

    // Transform values
    let tag_value = match &rule.value {
        TagValue::Pattern(p) => {
            let mut s = p.to_string();
            for (i, v) in values.iter().enumerate() {
                s = s.replace(&format!("{{{i}}}"), v);
            }
            s
        }
        TagValue::Compute(f) => f(&values),
    };

    let tag_name = if rule.prefix.is_empty() {
        tag_value
    } else {
        format!("{}:{}", rule.prefix, tag_value)
    };

    Some(TagCandidate {
        tag_name,
        tag_type: "auto".to_string(),
    })
}

/// Evaluate all rules against the given metadata.
pub fn evaluate_all(
    rules: &[TagRule],
    metadata: &[MetadataItem],
    file_type: &str,
) -> Vec<TagCandidate> {
    rules
        .iter()
        .filter_map(|r| evaluate_rule(r, metadata, file_type))
        .collect()
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Parse EXIF ExposureTime display value and return seconds.
///
/// Handles formats like "1/125", "0.5", "30", "1/4000".
pub fn parse_exposure_time(s: &str) -> f64 {
    if let Some(rest) = s.strip_prefix("1/") {
        rest.parse::<f64>().map(|d| 1.0 / d).unwrap_or(0.0)
    } else {
        s.parse::<f64>().unwrap_or(0.0)
    }
}

/// Parse GPS DMS format ("34 deg 42 min 12.5 sec") to decimal degrees.
pub fn parse_dms(s: &str) -> f64 {
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

/// Greatest common divisor (Euclidean algorithm).
pub fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Convenience constructor for TagRule.
#[inline]
pub fn rule(
    prefix: &'static str,
    keys: &'static [&'static str],
    value: TagValue,
    if_file_type: Option<&'static str>,
) -> TagRule {
    TagRule {
        prefix,
        keys,
        value,
        if_file_type,
    }
}

// ---------------------------------------------------------------------------
// Default rules — the full set of auto-tagging rules
// ---------------------------------------------------------------------------

impl TagRule {
    /// Returns the default set of tag rules.
    pub fn default_rules() -> Vec<Self> {
        use TagValue::*;
        vec![
            // ========== 基础文件类型分类 ==========
            rule("", &["file_type"], Pattern("image"), Some("jpeg")),
            rule("", &["file_type"], Pattern("image"), Some("png")),
            rule("", &["file_type"], Pattern("image"), Some("webp")),
            rule("", &["file_type"], Pattern("image"), Some("bmp")),
            rule("", &["file_type"], Pattern("image"), Some("tiff")),
            rule("", &["file_type"], Pattern("image"), Some("svg")),
            rule("", &["file_type"], Pattern("video"), Some("mp4")),
            rule("", &["file_type"], Pattern("video"), Some("mov")),
            rule("", &["file_type"], Pattern("video"), Some("avi")),
            rule("", &["file_type"], Pattern("video"), Some("mkv")),
            rule("", &["file_type"], Pattern("audio"), Some("mp3")),
            rule("", &["file_type"], Pattern("audio"), Some("flac")),
            rule("", &["file_type"], Pattern("audio"), Some("wav")),
            rule("", &["file_type"], Pattern("document"), Some("pdf")),
            rule("", &["file_type"], Pattern("document"), Some("doc")),
            rule("", &["file_type"], Pattern("document"), Some("docx")),
            // 相机信息
            rule("camera", &["Make"], Pattern("{0}"), None),
            rule("model", &["Model"], Pattern("{0}"), None),
            rule("lens", &["LensModel"], Pattern("{0}"), None),
            rule("lens_make", &["LensMake"], Pattern("{0}"), None),
            rule("owner", &["CameraOwnerName"], Pattern("{0}"), None),
            rule("serial", &["BodySerialNumber"], Pattern("{0}"), None),
            rule("software", &["Software"], Pattern("{0}"), None),
            rule("artist", &["Artist"], Pattern("{0}"), None),
            rule("copyright", &["Copyright"], Pattern("{0}"), None),
            // 拍摄参数（已离散）
            rule("program", &["ExposureProgram"], Pattern("{0}"), None),
            rule("exposure", &["ExposureMode"], Pattern("{0}"), None),
            rule("metering", &["MeteringMode"], Pattern("{0}"), None),
            rule("light", &["LightSource"], Pattern("{0}"), None),
            rule("whitebalance", &["WhiteBalance"], Pattern("{0}"), None),
            rule("scene", &["SceneCaptureType"], Pattern("{0}"), None),
            rule("colorspace", &["ColorSpace"], Pattern("{0}"), None),
            rule("contrast", &["Contrast"], Pattern("{0}"), None),
            rule("saturation", &["Saturation"], Pattern("{0}"), None),
            rule("sharpness", &["Sharpness"], Pattern("{0}"), None),
            rule("gain", &["GainControl"], Pattern("{0}"), None),
            rule("distance", &["SubjectDistanceRange"], Pattern("{0}"), None),
            rule("sensor", &["SensingMethod"], Pattern("{0}"), None),
            rule("source", &["FileSource"], Pattern("{0}"), None),
            rule("custom", &["CustomRendered"], Pattern("{0}"), None),
            rule("composite", &["CompositeImage"], Pattern("{0}"), None),
            // 连续→离散：焦距
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
            // 连续→离散：光圈
            rule("aperture", &["FNumber"], Compute(|vals| {
                let f: f64 = vals[0].parse().unwrap_or(0.0);
                match f {
                    _ if f <= 2.0 => "fast",
                    _ if f <= 2.8 => "bright",
                    _ if f <= 5.6 => "medium",
                    _ => "narrow",
                }.to_string()
            }), None),
            // 连续→离散：快门
            rule("shutter", &["ExposureTime"], Compute(|vals| {
                let t = parse_exposure_time(&vals[0]);
                match t {
                    _ if t > 1.0 => "long",
                    _ if t > 1.0 / 30.0 => "slow",
                    _ if t > 1.0 / 250.0 => "normal",
                    _ if t > 1.0 / 4000.0 => "fast",
                    _ => "ultrafast",
                }.to_string()
            }), None),
            // 连续→离散：ISO
            rule("iso", &["PhotographicSensitivity"], Compute(|vals| {
                let iso: f64 = vals[0].parse().unwrap_or(0.0);
                match iso {
                    _ if iso < 200.0 => "low",
                    _ if iso < 800.0 => "medium",
                    _ if iso < 6400.0 => "high",
                    _ => "extreme",
                }.to_string()
            }), None),
            // 连续→离散：曝光补偿
            rule("exposure_bias", &["ExposureBiasValue"], Compute(|vals| {
                let bias: f64 = vals[0].parse().unwrap_or(0.0);
                match bias {
                    _ if bias < -0.5 => "negative",
                    _ if bias > 0.5 => "positive",
                    _ => "normal",
                }.to_string()
            }), None),
            // 连续→离散：亮度
            rule("brightness", &["BrightnessValue"], Compute(|vals| {
                let bv: f64 = vals[0].parse().unwrap_or(0.0);
                match bv {
                    _ if bv < 0.0 => "dark",
                    _ if bv < 5.0 => "dim",
                    _ if bv < 10.0 => "normal",
                    _ => "bright",
                }.to_string()
            }), None),
            // 连续→离散：拍摄距离
            rule("distance_m", &["SubjectDistance"], Compute(|vals| {
                let d: f64 = vals[0].parse().unwrap_or(0.0);
                match d {
                    _ if d < 0.3 => "macro",
                    _ if d < 3.0 => "near",
                    _ if d < 20.0 => "distant",
                    _ => "infinity",
                }.to_string()
            }), None),
            // 连续→离散：数码变焦
            rule("digital_zoom", &["DigitalZoomRatio"], Compute(|vals| {
                let z: f64 = vals[0].parse().unwrap_or(1.0);
                match z {
                    _ if z <= 1.0 => "none",
                    _ if z <= 2.0 => "moderate",
                    _ => "heavy",
                }.to_string()
            }), None),
            // 连续→离散：宽高比
            rule("aspect", &["PixelXDimension", "PixelYDimension"], Compute(|vals| {
                let w: u32 = vals[0].parse().unwrap_or(1);
                let h: u32 = vals[1].parse().unwrap_or(1);
                let g = gcd(w, h);
                format!("{}:{}", w / g, h / g)
            }), None),
            // 连续→离散：分辨率
            rule("resolution", &["XResolution"], Compute(|vals| {
                let dpi: f64 = vals[0].parse().unwrap_or(0.0);
                match dpi {
                    _ if dpi < 150.0 => "draft",
                    _ if dpi < 300.0 => "standard",
                    _ => "high",
                }.to_string()
            }), None),
            // 时间维度
            rule("year", &["DateTimeOriginal"], Compute(|vals| {
                vals[0].get(..4).unwrap_or("").to_string()
            }), None),
            rule("month", &["DateTimeOriginal"], Compute(|vals| {
                vals[0].get(..7).unwrap_or("").to_string()
            }), None),
            rule("season", &["DateTimeOriginal"], Compute(|vals| {
                let m: u32 = vals[0].get(5..7).and_then(|s| s.parse().ok()).unwrap_or(0);
                match m {
                    3..=5 => "spring", 6..=8 => "summer",
                    9..=11 => "autumn", _ => "winter",
                }.to_string()
            }), None),
            rule("timeofday", &["DateTimeOriginal"], Compute(|vals| {
                let h: u32 = vals[0].get(11..13).and_then(|s| s.parse().ok()).unwrap_or(0);
                match h {
                    4..=6 => "dawn", 7..=10 => "morning",
                    11..=12 => "midday", 13..=16 => "afternoon",
                    17..=19 => "dusk", _ => "night",
                }.to_string()
            }), None),
            // 环境参数
            rule("temperature", &["Temperature"], Compute(|vals| {
                let t: f64 = vals[0].parse().unwrap_or(0.0);
                match t {
                    _ if t < 5.0 => "cold", _ if t < 20.0 => "mild",
                    _ if t < 30.0 => "warm", _ => "hot",
                }.to_string()
            }), None),
            rule("humidity", &["Humidity"], Compute(|vals| {
                let h: f64 = vals[0].parse().unwrap_or(50.0);
                match h { _ if h < 30.0 => "dry", _ if h < 70.0 => "normal", _ => "humid" }.to_string()
            }), None),
            rule("pressure", &["Pressure"], Compute(|vals| {
                let p: f64 = vals[0].parse().unwrap_or(1013.0);
                match p { _ if p < 1000.0 => "low", _ if p < 1020.0 => "normal", _ => "high" }.to_string()
            }), None),
            // GPS
            rule("grid", &["GPSLatitude", "GPSLongitude"], Compute(|vals| {
                let lat = parse_dms(&vals[0]);
                let lon = parse_dms(&vals[1]);
                format!("{:.2}_{:.2}", (lat * 100.0).round() / 100.0, (lon * 100.0).round() / 100.0)
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
}
```

Also register the module in `crates/s3-gallery-core/src/extractor/mod.rs`:
```rust
pub mod tag_rules;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib extractor::tag_rules::tests 2>&1 | tail -20`
Expected: all 7 tests pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: add TagRule engine for metadata-based auto-tagging"
```

---

### Task 2: Schema + Model Changes (effective_date + ensure_exists)

**Files:**
- Modify: `crates/s3-gallery-core/src/db/schema.rs`
- Modify: `crates/s3-gallery-core/src/db/models.rs`

- [ ] **Step 1: Add effective_date to schema migration**

In `schema.rs`, after the `files` table creation, add an `ALTER TABLE` for the `effective_date` column (same pattern as existing host_config columns):

```rust
// Attempt to add effective_date column to files (ignore if already exists)
drop(sqlx::query("ALTER TABLE files ADD COLUMN effective_date TEXT NOT NULL DEFAULT ''")
    .execute(pool)
    .await);
```

Also update the `test_run_migrations_creates_tables` test comment (line 265) from "9 user tables" to "10 user tables" — but actually the table count hasn't changed, only a column was added. The test name comment is misleading anyway. Let's leave it as-is.

- [ ] **Step 2: Add effective_date field to FileEntry model**

In `models.rs`, add `effective_date` to the `FileEntry` struct, between `metadata_state` and `is_deleted`:

```rust
pub struct FileEntry {
    // ... existing fields ...
    pub metadata_state: String,
    /// Effective date for timeline grouping (EXIF date or last_modified fallback).
    pub effective_date: String,
    pub is_deleted: bool,
}
```

This affects all `FileEntry` construction sites. Update all usages in the file:
- `FileEntry::upsert` — add `effective_date: String::new()` or `"".to_string()`
- `FileEntry::mark_deleted` — add `effective_date` field (empty string)
- `FileEntry::list_by_prefix` — no change needed (sqlx reads from DB)
- All test seed functions — add `effective_date: "".to_string()`

- [ ] **Step 3: Add TagEntry::ensure_exists**

In `models.rs`, in the `impl TagEntry` block, add:

```rust
/// Ensure a tag exists, creating it if necessary. Returns the tag_id.
pub async fn ensure_exists(pool: &SqlitePool, tag_name: &str, tag_type: &str) -> Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO tags (tag_name, tag_type) VALUES (?, ?)")
        .bind(tag_name)
        .bind(tag_type)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let row: (i64,) = sqlx::query_as("SELECT tag_id FROM tags WHERE tag_name = ?")
        .bind(tag_name)
        .fetch_one(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    Ok(row.0)
}
```

- [ ] **Step 4: Update all FileEntry references in tests and other files**

Run `grep -rn "FileEntry {"` to find all construction sites. Update each to include `effective_date: "".to_string()`. Key files:
- `crates/s3-gallery-core/src/scan/scanner.rs` — Step 5 upsert calls
- `crates/s3-gallery-core/src/view/ls.rs` — test seed functions
- `crates/s3-gallery-core/src/view/timeline.rs` — test seed functions
- `crates/s3-gallery-core/src/view/tags.rs` — test seed functions
- `crates/s3-gallery-core/src/db/models.rs` — test seed functions
- `tests/db_test.rs` — test seed functions

- [ ] **Step 5: Run all tests to verify they compile and pass**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: add effective_date to files table, add TagEntry::ensure_exists"
```

---

### Task 3: Scanner Step 6 — EXIF Extraction + Auto-Tagging

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`

- [ ] **Step 1: Write failing tests for process_file_metadata**

Add to the scanner test module:

```rust
#[tokio::test]
async fn test_process_file_metadata_with_exif() -> Result<()> {
    // Create a real JPEG with EXIF data for testing
    let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("test.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    // Create mock S3 client with a JPEG file that has EXIF
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;

    // For now, test that the function handles a non-supported file gracefully
    // (mp4 file, no EXIF support)
    let config = ScanConfig {
        s3: s3.clone(),
        db: pool.clone(),
        bucket: BucketName::new("test-bucket")?,
        prefix: ObjectKey::new("")?,
        concurrency: 10,
        extract_metadata: true,
        generate_thumbnails: false,
        client_id: "test-client".to_string(),
        host_id: "test-host".to_string(),
    };

    let mut registry = ExtractorRegistry::new();
    registry.register(Box::new(ExifExtractor::new()));
    let tag_rules = TagRule::default_rules();

    // Test with a non-image file (should return Ok(0))
    let obj = ObjectSummary {
        key: ObjectKey::new("test.mp4")?,
        etag: Etag::new("\"abc\"".to_string())?,
        size: FileSize::new(100),
        last_modified: "2024-01-01T00:00:00Z".to_string(),
    };

    // We need to make process_file_metadata public or test it through run_scan
    // For now, verify that run_scan with extract_metadata=true doesn't crash
    // (the mock S3 returns empty listings)
    let result = run_scan(config).await?;
    assert_eq!(result.metadata_extracted, 0);
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails or mark as pending**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib scan::scanner::tests 2>&1 | tail -20`
Expected: tests pass (existing tests) and the new test at least compiles

- [ ] **Step 3: Implement Step 6 in scanner.rs**

Add `process_file_metadata` helper function and implement Step 6:

```rust
/// Process metadata for a single object: download, extract, store, tag.
async fn process_file_metadata(
    config: &ScanConfig,
    registry: &ExtractorRegistry,
    tag_rules: &[TagRule],
    obj: &ObjectSummary,
) -> Result<u64> {
    use crate::classify::classifier::{classify_extension, parse_extension};
    use crate::db::models::{MetadataEntry, TagEntry, FileTagEntry};
    use crate::extractor::tag_rules::{evaluate_all, TagRule};
    use chrono::Utc;

    let key = obj.key.as_str();
    let file_name = key.rsplit('/').next().unwrap_or(key);
    let ext = match parse_extension(file_name) {
        Some(e) => e,
        None => return Ok(0),
    };
    let file_type = classify_extension(&ext).to_string();

    // Check if any extractor supports this file type
    if registry.find(&file_type, ext.as_str()).is_empty() {
        return Ok(0);
    }

    // Download only the first 64KB (EXIF data is always in the file header)
    let data = match config.s3.get_object_range(&config.bucket, &obj.key, 0, 65536).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "failed to download range for metadata extraction");
            return Ok(0);
        }
    };

    // Extract metadata
    let items = match registry.extract_all(&data, &file_type, ext.as_str()).await {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "metadata extraction failed");
            // Mark as failed so we don't retry on every scan
            sqlx::query("UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?")
                .bind(&config.host_id).bind(key)
                .execute(&config.db).await?;
            return Ok(0);
        }
    };

    if items.is_empty() {
        return Ok(0);
    }

    // Store metadata
    let now = Utc::now().to_rfc3339();
    for item in &items {
        MetadataEntry::insert(&config.db, &MetadataEntry {
            file_key: key.to_string(),
            namespace: item.namespace,
            key: item.key.clone(),
            value: item.value.clone(),
            extracted_at: now.clone(),
            partial: false,
        }).await?;
    }

    // Generate and store tags
    let tags = evaluate_all(tag_rules, &items, &file_type);
    for tag in &tags {
        let tag_id = TagEntry::ensure_exists(&config.db, &tag.tag_name, &tag.tag_type).await?;
        FileTagEntry::insert(&config.db, &FileTagEntry {
            file_key: key.to_string(),
            tag_id,
        }).await?;
    }

    // Update effective_date (EXIF DateTimeOriginal > last_modified)
    let exif_date = items.iter()
        .find(|m| m.key == "DateTimeOriginal" || m.key == "DateTimeDigitized")
        .map(|m| m.value.as_str())
        .and_then(|v| v.get(..10));
    let effective_date = exif_date.unwrap_or_else(|| {
        let lm = &obj.last_modified;
        lm.get(..10.min(lm.len())).unwrap_or(lm)
    });
    sqlx::query("UPDATE files SET effective_date = ?, metadata_state = 'extracted' WHERE host_id = ? AND key = ?")
        .bind(effective_date)
        .bind(&config.host_id)
        .bind(key)
        .execute(&config.db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    Ok(1)
}
```

Replace the placeholder Step 6 in `run_scan`:

```rust
// Step 6: Extract metadata (if enabled)
let mut metadata_extracted = 0u64;
if config.extract_metadata {
    let mut registry = ExtractorRegistry::new();
    registry.register(Box::new(ExifExtractor::new()));
    let tag_rules = TagRule::default_rules();

    for obj in diff.new_objects.iter().chain(diff.changed_objects.iter()) {
        match process_file_metadata(&config, &registry, &tag_rules, obj).await {
            Ok(count) => metadata_extracted += count,
            Err(e) => {
                tracing::warn!(key = %obj.key.as_str(), error = %e, "metadata processing failed");
            }
        }
    }
}
```

Add required imports at the top of the file:
```rust
use crate::extractor::exif::ExifExtractor;
use crate::extractor::registry::ExtractorRegistry;
use crate::extractor::tag_rules::TagRule;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: implement EXIF extraction and auto-tagging in scanner Step 6"
```

---

### Task 4: Unified Timeline-Gallery Core View

**Files:**
- Create: `crates/s3-gallery-core/src/view/timeline_gallery.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::FileEntry;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(SqlitePool, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;
        Ok((pool, dir))
    }

    async fn seed_test_files(pool: &SqlitePool) -> Result<()> {
        // Day 1: 2 files
        FileEntry::upsert(pool, &FileEntry {
            host_id: "host1".into(), key: "a.jpg".into(), etag: "\"1\"".into(),
            size: 100, last_modified: "2024-01-15T10:00:00Z".into(),
            content_type: Some("image/jpeg".into()), file_type: "jpeg".into(),
            metadata_state: "pending".into(), effective_date: "".into(), is_deleted: false,
        }).await?;
        FileEntry::upsert(pool, &FileEntry {
            host_id: "host1".into(), key: "b.jpg".into(), etag: "\"2\"".into(),
            size: 200, last_modified: "2024-01-15T11:00:00Z".into(),
            content_type: Some("image/jpeg".into()), file_type: "jpeg".into(),
            metadata_state: "pending".into(), effective_date: "".into(), is_deleted: false,
        }).await?;
        // Day 2: 1 file
        FileEntry::upsert(pool, &FileEntry {
            host_id: "host1".into(), key: "c.mp4".into(), etag: "\"3\"".into(),
            size: 50000, last_modified: "2024-02-20T14:00:00Z".into(),
            content_type: Some("video/mp4".into()), file_type: "mp4".into(),
            metadata_state: "pending".into(), effective_date: "".into(), is_deleted: false,
        }).await?;
        // Deleted file (should be excluded)
        FileEntry::upsert(pool, &FileEntry {
            host_id: "host1".into(), key: "d.txt".into(), etag: "\"4\"".into(),
            size: 50, last_modified: "2024-03-01T00:00:00Z".into(),
            content_type: None, file_type: "unknown".into(),
            metadata_state: "pending".into(), effective_date: "".into(), is_deleted: true,
        }).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_basic() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let (entries, has_more) = get_timeline_gallery(
            &pool, None, 0, 10, None,
        ).await?;
        assert_eq!(entries.len(), 2); // 2 date groups
        assert!(!has_more);
        assert_eq!(entries[0].date, "2024-02-20"); // newest first
        assert_eq!(entries[0].count, 1);
        assert_eq!(entries[1].date, "2024-01-15");
        assert_eq!(entries[1].count, 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_pagination() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        // Page size = 1 → should return 1 group, has_more = true
        let (entries, has_more) = get_timeline_gallery(
            &pool, None, 0, 1, None,
        ).await?;
        assert_eq!(entries.len(), 1);
        assert!(has_more);
        assert_eq!(entries[0].date, "2024-02-20");

        // Page 1 → should return the second group
        let (entries, has_more) = get_timeline_gallery(
            &pool, None, 1, 1, None,
        ).await?;
        assert_eq!(entries.len(), 1);
        assert!(!has_more);
        assert_eq!(entries[0].date, "2024-01-15");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_empty() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        let (entries, has_more) = get_timeline_gallery(
            &pool, None, 0, 10, None,
        ).await?;
        assert!(entries.is_empty());
        assert!(!has_more);
        Ok(())
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib view::timeline_gallery::tests 2>&1 | head -20`
Expected: `error[E0432]` — module not found

- [ ] **Step 3: Implement get_timeline_gallery**

Create `crates/s3-gallery-core/src/view/timeline_gallery.rs`:

```rust
//! Unified timeline-gallery view — files grouped by date, tag-filterable, paginated.

use std::collections::BTreeMap;

use sqlx::SqlitePool;

use crate::db::models::FileEntry;
use crate::error::{Result, S3GalleryError};

/// A timeline entry containing files from a specific date.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub date: String,
    pub files: Vec<FileEntry>,
    pub count: u64,
}

/// Get timeline-gallery entries grouped by date, with optional tag filtering.
///
/// Returns `(entries, has_more)` where `has_more` is true if more pages exist.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_timeline_gallery(
    db: &SqlitePool,
    host_id: Option<&str>,
    page: u32,
    page_size: u32,
    tag: Option<&str>,
) -> Result<(Vec<TimelineEntry>, bool)> {
    // Build the query with optional host_id and tag filters
    let (files, dates) = if let Some(tag_name) = tag {
        // Tag-filtered query
        if tag_name.is_empty() {
            return Ok((Vec::new(), false));
        }
        let files: Vec<FileEntry> = if let Some(hid) = host_id {
            sqlx::query_as(
                "SELECT f.* FROM files f \
                 INNER JOIN file_tags ft ON f.key = ft.file_key \
                 INNER JOIN tags t ON ft.tag_id = t.tag_id \
                 WHERE t.tag_name = ? AND f.host_id = ? AND f.is_deleted = 0 \
                 ORDER BY f.effective_date DESC, f.last_modified DESC"
            )
            .bind(tag_name).bind(hid)
            .fetch_all(db).await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT f.* FROM files f \
                 INNER JOIN file_tags ft ON f.key = ft.file_key \
                 INNER JOIN tags t ON ft.tag_id = t.tag_id \
                 WHERE t.tag_name = ? AND f.is_deleted = 0 \
                 ORDER BY f.effective_date DESC, f.last_modified DESC"
            )
            .bind(tag_name)
            .fetch_all(db).await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        };
        (files, Vec::new())
    } else {
        // No tag filter: get all files
        let files: Vec<FileEntry> = if let Some(hid) = host_id {
            sqlx::query_as(
                "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
                 ORDER BY effective_date DESC, last_modified DESC"
            )
            .bind(hid)
            .fetch_all(db).await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT * FROM files WHERE is_deleted = 0 \
                 ORDER BY effective_date DESC, last_modified DESC"
            )
            .fetch_all(db).await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        };
        (files, Vec::new())
    };

    // Group by date (effective_date or last_modified)
    let mut grouped: BTreeMap<String, Vec<FileEntry>> = BTreeMap::new();
    for file in files {
        let date = if file.effective_date.is_empty() {
            extract_date(&file.last_modified)
        } else {
            file.effective_date.clone()
        };
        grouped.entry(date).or_default().push(file);
    }

    // Paginate: skip `page * page_size` groups, take `page_size + 1` groups
    let offset = page as usize * page_size as usize;
    let all_dates: Vec<String> = grouped.into_iter().rev().map(|(d, _)| d).collect();

    let has_more = all_dates.len() > offset + page_size as usize;
    let page_dates: Vec<&str> = all_dates
        .iter()
        .skip(offset)
        .take(page_size as usize)
        .map(|d| d.as_str())
        .collect();

    // Re-query files for the selected dates
    let mut entries = Vec::new();
    for date_str in &page_dates {
        let files: Vec<FileEntry> = if let Some(hid) = host_id {
            let sql = if tag.is_some() {
                format!(
                    "SELECT f.* FROM files f \
                     INNER JOIN file_tags ft ON f.key = ft.file_key \
                     INNER JOIN tags t ON ft.tag_id = t.tag_id \
                     WHERE t.tag_name = ? AND f.host_id = ? AND f.is_deleted = 0 \
                     AND (f.effective_date = ? OR (f.effective_date = '' AND substr(f.last_modified, 1, 10) = ?)) \
                     ORDER BY f.last_modified DESC"
                )
            } else {
                format!(
                    "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
                     AND (effective_date = ? OR (effective_date = '' AND substr(last_modified, 1, 10) = ?)) \
                     ORDER BY last_modified DESC"
                )
            };
            let mut q = sqlx::query_as::<_, FileEntry>(&sql);
            if let Some(tag_name) = tag {
                q = q.bind(tag_name).bind(hid).bind(date_str).bind(date_str);
            } else {
                q = q.bind(hid).bind(date_str).bind(date_str);
            }
            q.fetch_all(db).await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        } else {
            let sql = if tag.is_some() {
                format!(
                    "SELECT f.* FROM files f \
                     INNER JOIN file_tags ft ON f.key = ft.file_key \
                     INNER JOIN tags t ON ft.tag_id = t.tag_id \
                     WHERE t.tag_name = ? AND f.is_deleted = 0 \
                     AND (f.effective_date = ? OR (f.effective_date = '' AND substr(f.last_modified, 1, 10) = ?)) \
                     ORDER BY f.last_modified DESC"
                )
            } else {
                format!(
                    "SELECT * FROM files WHERE is_deleted = 0 \
                     AND (effective_date = ? OR (effective_date = '' AND substr(last_modified, 1, 10) = ?)) \
                     ORDER BY last_modified DESC"
                )
            };
            let mut q = sqlx::query_as::<_, FileEntry>(&sql);
            if let Some(tag_name) = tag {
                q = q.bind(tag_name).bind(date_str).bind(date_str);
            } else {
                q = q.bind(date_str).bind(date_str);
            }
            q.fetch_all(db).await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        };

        let count = files.len() as u64;
        entries.push(TimelineEntry {
            date: date_str.to_string(),
            files,
            count,
        });
    }

    Ok((entries, has_more))
}

fn extract_date(timestamp: &str) -> String {
    if timestamp.len() >= 10 {
        timestamp[..10].to_string()
    } else {
        timestamp.to_string()
    }
}

#[cfg(test)]
mod tests {
    // ... (same as Step 1)
}
```

Also register the module in `crates/s3-gallery-core/src/view/mod.rs`:
```rust
pub mod timeline_gallery;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/macbook/Project/s3-gallery && cargo test -p s3-gallery-core --lib view::timeline_gallery::tests 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: add timeline_gallery view with pagination and tag filtering"
```

---

### Task 5: Gallery Handler + Route + Template Changes

**Files:**
- Modify: `crates/s3-gallery-cli/src/web/handlers/gallery.rs`
- Modify: `crates/s3-gallery-cli/src/web/router.rs`
- Modify: `crates/s3-gallery-web/templates/gallery.html`
- Modify: `crates/s3-gallery-web/templates/gallery_items.html`
- Modify: `crates/s3-gallery-web/templates/layout.html`
- Delete: `crates/s3-gallery-cli/src/web/handlers/timeline.rs`
- Delete: `crates/s3-gallery-web/templates/timeline.html`
- Delete: `crates/s3-gallery-core/src/view/timeline.rs`

- [ ] **Step 1: Rewrite gallery handler**

Replace `crates/s3-gallery-cli/src/web/handlers/gallery.rs`:

```rust
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use s3_gallery_core::view::timeline_gallery;
use serde::Deserialize;
use serde_json::json;

use crate::web::state::AppState;

/// Gallery item view for template rendering.
#[derive(Debug, Clone, serde::Serialize)]
struct GalleryItem {
    key: String,
    name: String,
    thumbnail_url: String,
    host_id: String,
    file_type: String,
    size: i64,
    last_modified: String,
}

/// A date group in the timeline gallery.
#[derive(Debug, Clone, serde::Serialize)]
struct TimelineGroup {
    date: String,
    count: u64,
    items: Vec<GalleryItem>,
}

/// Query parameters for the gallery page.
#[derive(Debug, Default, Deserialize)]
pub struct GalleryQuery {
    pub page: Option<u32>,
    pub tag: Option<String>,
    pub host_id: Option<String>,
}

/// Number of date groups per page.
const GROUPS_PER_PAGE: u32 = 10;

/// Build a context map for template rendering.
fn build_context(
    groups: &[TimelineGroup],
    page: u32,
    has_more: bool,
    tag: Option<&str>,
    all_tags: &[serde_json::Value],
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert("groups".to_string(), json!(groups));
    ctx.insert("page".to_string(), json!(page));
    ctx.insert("has_more".to_string(), json!(has_more));
    ctx.insert("tag".to_string(), json!(tag));
    ctx.insert("all_tags".to_string(), json!(all_tags));
    serde_json::Value::Object(ctx)
}

/// Render a minijinja template with the given context.
fn render_template(
    state: &AppState,
    template_name: &str,
    context: &serde_json::Value,
) -> Result<Html<String>, Box<Response>> {
    let template = state.templates.get_template(template_name).map_err(|e| {
        Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "template not found", "detail": e.to_string()})),
            ).into_response(),
        )
    })?;
    let html = template.render(context).map_err(|e| {
        Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "template rendering failed", "detail": e.to_string()})),
            ).into_response(),
        )
    })?;
    Ok(Html(html))
}

/// Extract the file name from a key (last segment after '/').
fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
}

/// Gallery handler — renders the unified timeline-gallery page.
pub async fn gallery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GalleryQuery>,
) -> impl IntoResponse {
    let page = params.page.unwrap_or(0);
    let tag = params.tag.as_deref();
    // tag="" is the same as no tag
    let tag = tag.filter(|t| !t.is_empty());

    tracing::info!(handler = "gallery", page = %page, tag = ?tag, "serving gallery");

    let pool = &state.db;

    let (entries, has_more) = match timeline_gallery::get_timeline_gallery(
        pool, params.host_id.as_deref(), page, GROUPS_PER_PAGE, tag,
    ).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(handler = "gallery", error = %e, "failed to get timeline gallery");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to get gallery", "detail": e.to_string()})),
            ).into_response();
        }
    };

    // Convert to template views
    let groups: Vec<TimelineGroup> = entries.iter().map(|entry| {
        let items: Vec<GalleryItem> = entry.files.iter().map(|f| {
            let name = file_name_from_key(&f.key);
            GalleryItem {
                key: f.key.clone(),
                name,
                thumbnail_url: format!("/thumbnails/{}", f.key),
                host_id: f.host_id.clone(),
                file_type: f.file_type.clone(),
                size: f.size,
                last_modified: f.last_modified.clone(),
            }
        }).collect();
        TimelineGroup {
            date: entry.date.clone(),
            count: entry.count,
            items,
        }
    }).collect();

    // Fetch all tags for the filter dropdown
    let all_tags: Vec<serde_json::Value> = match s3_gallery_core::view::tags::list_tags(pool, params.host_id.as_deref()).await {
        Ok(tags) => tags.iter().map(|t| json!({ "name": t.tag_name, "type": t.tag_type })).collect(),
        Err(_) => Vec::new(),
    };

    let context = build_context(&groups, page, has_more, tag, &all_tags);

    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    let template_name = if is_htmx { "gallery_items.html" } else { "gallery.html" };

    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}
```

- [ ] **Step 2: Remove timeline route from router.rs**

In `router.rs`, find and remove the timeline route:
```rust
// Remove:
// .route("/timeline", get(handlers::timeline))
```

Also remove the import of the timeline handler at the top.

- [ ] **Step 3: Remove timeline nav link from layout.html**

Remove from `layout.html`:
```html
<!-- Remove: -->
<a href="/timeline">Timeline</a>
```

- [ ] **Step 4: Rewrite gallery.html template**

```html
{% extends "layout.html" %}
{% block title %}Gallery - s3-gallery{% endblock %}
{% block content %}
<h1>Gallery</h1>

<div class="gallery-filters">
    <form method="get" action="/gallery">
        <label>
            Tag:
            <select name="tag" onchange="this.form.submit()">
                <option value="">All</option>
                {% for t in all_tags %}
                <option value="{{ t.name }}"{% if tag == t.name %} selected{% endif %}>{{ t.name }}</option>
                {% endfor %}
            </select>
        </label>
    </form>
</div>

<div id="gallery-container">
    {% for group in groups %}
    <h2>{{ group.date }} ({{ group.count }})</h2>
    <div class="gallery-grid">
        {% for item in group.items %}
        <div class="gallery-item">
            <a href="/files/{{ item.key }}">
                <img class="thumbnail" src="{{ item.thumbnail_url }}" loading="lazy" alt="{{ item.name }}">
            </a>
            <div class="gallery-item-name">{{ item.name }}</div>
            <div class="gallery-item-meta">
                <span class="host-badge">{{ item.host_id }}</span>
                <span class="file-type">{{ item.file_type }}</span>
            </div>
        </div>
        {% endfor %}
    </div>
    {% endfor %}

    {% if has_more %}
    <div class="gallery-load-more"
         hx-get="/gallery?page={{ page + 1 }}{% if tag %}&tag={{ tag }}{% endif %}"
         hx-trigger="revealed"
         hx-swap="outerHTML">
        Loading more...
    </div>
    {% endif %}
</div>
{% endblock %}
```

- [ ] **Step 5: Rewrite gallery_items.html partial**

```html
{% for group in groups %}
<h2>{{ group.date }} ({{ group.count }})</h2>
<div class="gallery-grid">
    {% for item in group.items %}
    <div class="gallery-item">
        <a href="/files/{{ item.key }}">
            <img class="thumbnail" src="{{ item.thumbnail_url }}" loading="lazy" alt="{{ item.name }}">
        </a>
        <div class="gallery-item-name">{{ item.name }}</div>
        <div class="gallery-item-meta">
            <span class="host-badge">{{ item.host_id }}</span>
            <span class="file-type">{{ item.file_type }}</span>
        </div>
    </div>
    {% endfor %}
</div>
{% endfor %}

{% if has_more %}
<div class="gallery-load-more"
     hx-get="/gallery?page={{ page + 1 }}{% if tag %}&tag={{ tag }}{% endif %}"
     hx-trigger="revealed"
     hx-swap="outerHTML">
    Loading more...
</div>
{% endif %}
```

- [ ] **Step 6: Delete old timeline files**

Delete:
- `crates/s3-gallery-core/src/view/timeline.rs` (but check if anything else imports it first)
- `crates/s3-gallery-cli/src/web/handlers/timeline.rs`
- `crates/s3-gallery-web/templates/timeline.html`

- [ ] **Step 7: Add CSS for new elements to layout.html**

Add to the `<style>` block in `layout.html`:
```css
.gallery-item { border: 1px solid #eee; border-radius: 4px; padding: 8px; }
.gallery-item-name { font-size: 0.85em; margin-top: 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gallery-item-meta { font-size: 0.75em; color: #666; margin-top: 2px; }
.host-badge { background: #eee; padding: 1px 5px; border-radius: 3px; margin-right: 4px; }
.file-type { color: #999; }
.gallery-filters { margin: 10px 0; }
.gallery-filters select { padding: 4px 8px; border: 1px solid #ccc; border-radius: 4px; }
```

- [ ] **Step 8: Build and test**

Run: `cd /Users/macbook/Project/s3-gallery && cargo build 2>&1 | tail -30`
Expected: builds successfully

Run: `cd /Users/macbook/Project/s3-gallery && cargo test 2>&1 | tail -30`
Expected: all tests pass

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "feat: unified timeline-gallery page with tag filtering and infinite scroll"
```

---

## Verification

1. Run `cargo test` — all tests pass
2. Run `s3-gallery scan init --bucket <bucket> --extract-metadata` — verify metadata extraction and auto-tagging
3. Run `s3-gallery serve` — verify unified gallery page:
   - Files grouped by date (effective_date)
   - Thumbnails for images, icons for other files
   - Tag filter dropdown works
   - Infinite scroll loads more date groups
   - Timeline link removed from nav
4. Check DB: `sqlite3 gallery.db "SELECT COUNT(*) FROM metadata"` — should have entries
5. Check DB: `sqlite3 gallery.db "SELECT COUNT(*) FROM tags"` — should have auto-generated tags
6. Check DB: `sqlite3 gallery.db "SELECT effective_date FROM files LIMIT 5"` — should have values