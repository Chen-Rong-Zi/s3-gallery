# EXIF Tag + GPS Reverse Geocoding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `exif:yes` tag for files with EXIF data, and reverse geocode GPS coordinates to place names using offline administrative division data.

**Architecture:** Geocoder module with embedded JSON division data → scanner integration that adds location tags from GPS coordinates. Simple linear scan with Haversine distance for nearest-neighbor lookup.

**Tech Stack:** Rust, serde_json, Haversine formula

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/s3-gallery-core/src/extractor/geocode.rs` | **Create** | `Geocoder` struct, `reverse_geocode()`, `Division` model |
| `crates/s3-gallery-core/src/extractor/geocode_data.json` | **Create** | Embedded JSON with Chinese administrative divisions |
| `crates/s3-gallery-core/src/extractor/mod.rs` | **Modify** | Register `geocode` module |
| `crates/s3-gallery-core/src/scan/scanner.rs` | **Modify** | Add `exif:yes` tag + GPS reverse geocoding |

---

### Task 1: Geocoder Module + Data

**Files:**
- Create: `crates/s3-gallery-core/src/extractor/geocode.rs`
- Create: `crates/s3-gallery-core/src/extractor/geocode_data.json`
- Modify: `crates/s3-gallery-core/src/extractor/mod.rs`

- [ ] **Step 1: Create the administrative division data file**

Create `crates/s3-gallery-core/src/extractor/geocode_data.json` with ~300 entries covering major Chinese provinces, cities, and districts with GPS center points. Format:

```json
[
  {"name":"东城区","lat":39.9283,"lon":116.4163,"city":"北京市","province":"北京市"},
  {"name":"西城区","lat":39.9123,"lon":116.3660,"city":"北京市","province":"北京市"},
  {"name":"朝阳区","lat":39.9215,"lon":116.4432,"city":"北京市","province":"北京市"},
  {"name":"海淀区","lat":39.9597,"lon":116.2983,"city":"北京市","province":"北京市"},
  {"name":"浦东新区","lat":31.2213,"lon":121.5440,"city":"上海市","province":"上海市"},
  {"name":"天河区","lat":23.1288,"lon":113.3618,"city":"广州市","province":"广东省"},
  {"name":"南山区","lat":22.5332,"lon":113.9303,"city":"深圳市","province":"广东省"},
  ...
]
```

Include at minimum: all provinces/autonomous regions/municipalities, major cities and their districts.

- [ ] **Step 2: Write failing tests for geocode module**

In `crates/s3-gallery-core/src/extractor/geocode.rs`, add tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverse_geocode_beijing() {
        let geocoder = Geocoder::from_embedded();
        // 天安门广场 GPS
        let result = geocoder.reverse_geocode(39.9042, 116.4074);
        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.city, "北京市");
    }

    #[test]
    fn test_reverse_geocode_shanghai() {
        let geocoder = Geocoder::from_embedded();
        // 外滩附近
        let result = geocoder.reverse_geocode(31.2400, 121.4900);
        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.city, "上海市");
    }

    #[test]
    fn test_reverse_geocode_empty_ocean() {
        let geocoder = Geocoder::from_embedded();
        // 太平洋中间
        let result = geocoder.reverse_geocode(0.0, 180.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_haversine_distance() {
        // 北京到上海大约 1060km
        let dist = haversine(39.9042, 116.4074, 31.2304, 121.4737);
        assert!((dist - 1060.0).abs() < 50.0);
    }
}
```

- [ ] **Step 3: Run test to verify they fail**

Run: `cargo test -p s3-gallery-core --lib extractor::geocode::tests 2>&1 | head -10`
Expected: `error[E0432]` — module not found

- [ ] **Step 4: Implement the geocode module**

Create `crates/s3-gallery-core/src/extractor/geocode.rs`:

```rust
//! Offline reverse geocoding using embedded administrative division data.
//!
//! Uses a Haversine nearest-neighbor search over ~300 Chinese administrative
//! divisions (provinces, cities, districts) to convert GPS coordinates into
//! place names. No external API calls needed.

/// A single administrative division entry.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Division {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    pub province: String,
}

/// A resolved location from GPS coordinates.
#[derive(Debug, Clone)]
pub struct Location {
    pub district: String,
    pub city: String,
    pub province: String,
}

/// Offline reverse geocoder using embedded division data.
pub struct Geocoder {
    divisions: Vec<Division>,
}

impl Geocoder {
    /// Load divisions from the embedded JSON data file.
    pub fn from_embedded() -> Self {
        let data = include_str!("geocode_data.json");
        let divisions: Vec<Division> = serde_json::from_str(data)
            .expect("geocode_data.json should be valid JSON");
        Self { divisions }
    }

    /// Reverse geocode GPS coordinates to the nearest administrative division.
    ///
    /// Returns `None` if the nearest division is more than 1° (~111km) away.
    pub fn reverse_geocode(&self, lat: f64, lon: f64) -> Option<Location> {
        const MAX_DIST_KM: f64 = 111.0; // ~1° in km
        let mut nearest: Option<(f64, &Division)> = None;

        for div in &self.divisions {
            let dist = haversine(lat, lon, div.lat, div.lon);
            if dist < MAX_DIST_KM {
                match nearest {
                    Some((best_dist, _)) if dist < best_dist => nearest = Some((dist, div)),
                    None => nearest = Some((dist, div)),
                    _ => {}
                }
            }
        }

        nearest.map(|(_, div)| Location {
            district: div.name.clone(),
            city: div.city.clone(),
            province: div.province.clone(),
        })
    }
}

/// Calculate the great-circle distance between two GPS coordinates using
/// the Haversine formula. Returns distance in kilometers.
fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0; // Earth's radius in km
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    R * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverse_geocode_beijing() {
        let geocoder = Geocoder::from_embedded();
        let result = geocoder.reverse_geocode(39.9042, 116.4074);
        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.city, "北京市");
    }

    #[test]
    fn test_reverse_geocode_shanghai() {
        let geocoder = Geocoder::from_embedded();
        let result = geocoder.reverse_geocode(31.2400, 121.4900);
        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.city, "上海市");
    }

    #[test]
    fn test_reverse_geocode_empty_ocean() {
        let geocoder = Geocoder::from_embedded();
        let result = geocoder.reverse_geocode(0.0, 180.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_haversine_distance() {
        let dist = haversine(39.9042, 116.4074, 31.2304, 121.4737);
        assert!((dist - 1060.0).abs() < 50.0);
    }
}
```

Register the module in `crates/s3-gallery-core/src/extractor/mod.rs`:
```rust
pub mod geocode;
```

Also add `serde` dependency for `crates/s3-gallery-core/Cargo.toml` if not already present:
```toml
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p s3-gallery-core --lib extractor::geocode::tests 2>&1 | tail -15`
Expected: all 4 tests pass

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: add geocoder module with offline reverse geocoding"
```

---

### Task 2: Scanner Integration — exif:yes tag + GPS geocoding

**Files:**
- Modify: `crates/s3-gallery-core/src/scan/scanner.rs`

- [ ] **Step 1: Add exif:yes tag**

In `process_file_metadata`, after the tag generation loop (after line ~378), add:

```rust
// Add exif:yes tag for all files with extracted metadata
let exif_tag_id = TagEntry::ensure_exists(&config.db, "exif:yes", "auto").await?;
FileTagEntry::insert(&config.db, &FileTagEntry {
    file_key: key.to_string(),
    tag_id: exif_tag_id,
}).await?;
```

- [ ] **Step 2: Add GPS reverse geocoding**

After the exif:yes tag, add GPS reverse geocoding:

```rust
// Find GPS coordinates in extracted metadata
let gps_lat = items.iter()
    .find(|m| m.key == "GPSLatitude")
    .map(|m| parse_dms(&m.value));
let gps_lon = items.iter()
    .find(|m| m.key == "GPSLongitude")
    .map(|m| parse_dms(&m.value));

if let (Some(lat), Some(lon)) = (gps_lat, gps_lon) {
    let geocoder = crate::extractor::geocode::Geocoder::from_embedded();
    if let Some(location) = geocoder.reverse_geocode(lat, lon) {
        // Add location:city tag
        let tag_id = TagEntry::ensure_exists(
            &config.db,
            &format!("location:{}", location.city),
            "auto",
        ).await?;
        FileTagEntry::insert(&config.db, &FileTagEntry {
            file_key: key.to_string(),
            tag_id,
        }).await?;

        // Add location:district tag (more precise)
        let tag_id = TagEntry::ensure_exists(
            &config.db,
            &format!("location:{}", location.district),
            "auto",
        ).await?;
        FileTagEntry::insert(&config.db, &FileTagEntry {
            file_key: key.to_string(),
            tag_id,
        }).await?;
    }
}
```

- [ ] **Step 3: Add import for parse_dms**

The `parse_dms` function is in `crate::extractor::tag_rules`. Add the import at the top of `process_file_metadata`:

```rust
use crate::extractor::tag_rules::parse_dms;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test 2>&1 | tail -15`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: add exif:yes tag and GPS reverse geocoding to scanner"
```

---

## Verification

1. Run `cargo test` — all tests pass
2. Run scan with `--extract-metadata` — verify `exif:yes` tag appears for files with EXIF
3. Verify `location:xxx` tags appear for files with GPS coordinates
4. Check gallery page tag filter works with new tags