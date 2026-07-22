//! Offline reverse geocoding using embedded administrative division data.
//!
//! Uses a Haversine nearest-neighbor search over ~460 Chinese administrative
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
        // Safe: geocode_data.json is embedded at compile time and verified
        // by tests. If it's invalid JSON, the test suite will catch it.
        let divisions: Vec<Division> = serde_json::from_str(data).unwrap_or_default();
        Self { divisions }
    }

    /// Reverse geocode GPS coordinates to the nearest administrative division.
    ///
    /// Returns `None` if the nearest division is more than 1° (~111km) away.
    pub fn reverse_geocode(&self, lat: f64, lon: f64) -> Option<Location> {
        const MAX_DIST_KM: f64 = 111.0;
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
    const R: f64 = 6371.0;
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
        // Tiananmen Square
        let result = geocoder.reverse_geocode(39.9042, 116.4074);
        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.city, "北京市");
    }

    #[test]
    fn test_reverse_geocode_shanghai() {
        let geocoder = Geocoder::from_embedded();
        // The Bund area
        let result = geocoder.reverse_geocode(31.2400, 121.4900);
        assert!(result.is_some());
        let loc = result.unwrap();
        assert_eq!(loc.city, "上海市");
    }

    #[test]
    fn test_reverse_geocode_empty_ocean() {
        let geocoder = Geocoder::from_embedded();
        // Middle of the Pacific Ocean
        let result = geocoder.reverse_geocode(0.0, 180.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_haversine_distance() {
        // Beijing to Shanghai ~1060km
        let dist = haversine(39.9042, 116.4074, 31.2304, 121.4737);
        assert!((dist - 1060.0).abs() < 50.0);
    }
}
