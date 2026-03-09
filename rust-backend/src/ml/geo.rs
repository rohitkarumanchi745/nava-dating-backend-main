//! Geo-aware scoring: gravity model + location density smoothing.
//!
//! Gravity model: score ∝ 1 / (1 + distance^β) where β controls decay.
//! Density smoothing: adjust expectations in sparse vs dense areas so users in
//! low-density regions aren't penalized for having fewer nearby candidates.
//!
//! Supports both kilometers and miles for user-facing display.

use sqlx::PgPool;

/// Conversion factor: 1 km ≈ 0.621371 miles.
const KM_TO_MILES: f64 = 0.621371;
/// Conversion factor: 1 mile ≈ 1.60934 km.
const MILES_TO_KM: f64 = 1.60934;

/// Distance unit for user-facing display and configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceUnit {
    Kilometers,
    Miles,
}

/// Gravity model parameters.
#[derive(Debug, Clone)]
pub struct GeoScorer {
    /// Decay exponent β. Higher = steeper distance penalty.
    /// 1.5 is a good default for dating: strong local preference but not zero at 50km.
    pub beta: f64,
    /// Maximum distance (km internally) to consider. Beyond this, score = 0.
    pub max_distance_km: f64,
    /// Density smoothing bandwidth (km). Radius for counting nearby users.
    pub density_bandwidth_km: f64,
    /// Minimum density floor (prevents division by zero in sparse areas).
    pub density_floor: f64,
    /// User-facing distance unit (all internal math uses km).
    pub display_unit: DistanceUnit,
}

impl Default for GeoScorer {
    fn default() -> Self {
        Self {
            beta: 1.5,
            max_distance_km: 100.0,
            density_bandwidth_km: 25.0,
            density_floor: 5.0,
            display_unit: DistanceUnit::Kilometers,
        }
    }
}

impl GeoScorer {
    /// Create a scorer configured for miles (e.g. US/UK users).
    /// Internally still uses km; `max_distance` is accepted in miles and converted.
    pub fn with_miles(max_distance_miles: f64) -> Self {
        Self {
            max_distance_km: max_distance_miles * MILES_TO_KM,
            density_bandwidth_km: 15.0 * MILES_TO_KM, // ~15 miles
            display_unit: DistanceUnit::Miles,
            ..Self::default()
        }
    }
}

/// Result of geo scoring between two users.
#[derive(Debug, Clone)]
pub struct GeoScore {
    /// Raw gravity score (0.0-1.0)
    pub gravity_score: f64,
    /// Density-adjusted score (gravity * density_factor)
    pub density_adjusted_score: f64,
    /// Distance in km between the two users
    pub distance_km: f64,
    /// Distance in miles between the two users
    pub distance_miles: f64,
    /// Local user density around the candidate
    pub local_density: f64,
}

impl GeoScore {
    /// Get the distance in the specified unit.
    pub fn distance_in(&self, unit: DistanceUnit) -> f64 {
        match unit {
            DistanceUnit::Kilometers => self.distance_km,
            DistanceUnit::Miles => self.distance_miles,
        }
    }

    /// Format distance for user-facing display (e.g. "12 km" or "8 mi").
    pub fn display_distance(&self, unit: DistanceUnit) -> String {
        match unit {
            DistanceUnit::Kilometers => {
                if self.distance_km < 1.0 {
                    format!("{:.0} m", self.distance_km * 1000.0)
                } else {
                    format!("{:.0} km", self.distance_km)
                }
            }
            DistanceUnit::Miles => {
                if self.distance_miles < 1.0 {
                    format!("{:.0} ft", self.distance_miles * 5280.0)
                } else {
                    format!("{:.0} mi", self.distance_miles)
                }
            }
        }
    }
}

/// Convert km to miles.
pub fn km_to_miles(km: f64) -> f64 {
    km * KM_TO_MILES
}

/// Convert miles to km.
pub fn miles_to_km(miles: f64) -> f64 {
    miles * MILES_TO_KM
}

impl GeoScorer {
    /// Compute gravity score from distance alone.
    /// Returns 0.0–1.0 where 1.0 = same location, 0.0 = beyond max_distance.
    pub fn gravity_score(&self, distance_km: f64) -> f64 {
        if distance_km >= self.max_distance_km {
            return 0.0;
        }
        1.0 / (1.0 + (distance_km / 10.0).powf(self.beta))
    }

    /// Compute density factor for a location.
    /// Returns a multiplier > 1.0 in sparse areas (boosts distant users) and
    /// ~1.0 in dense areas (lets gravity dominate).
    pub fn density_factor(&self, local_density: f64) -> f64 {
        // In dense areas (density >> floor), factor ≈ 1.0.
        // In sparse areas (density → floor), factor boosts up to 2.0.
        let effective = local_density.max(self.density_floor);
        let reference = 50.0; // Typical medium-density area
        let ratio = reference / effective;
        ratio.sqrt().clamp(0.5, 2.0)
    }

    /// Full geo score between user and candidate.
    /// `distance_km` is always in kilometers (internal unit).
    pub fn score(
        &self,
        distance_km: f64,
        candidate_local_density: f64,
    ) -> GeoScore {
        let gravity = self.gravity_score(distance_km);
        let df = self.density_factor(candidate_local_density);
        GeoScore {
            gravity_score: gravity,
            density_adjusted_score: (gravity * df).clamp(0.0, 1.0),
            distance_km,
            distance_miles: distance_km * KM_TO_MILES,
            local_density: candidate_local_density,
        }
    }

    /// Convenience: score using a distance provided in the scorer's display unit.
    /// Converts to km internally before scoring.
    pub fn score_in_display_unit(
        &self,
        distance: f64,
        candidate_local_density: f64,
    ) -> GeoScore {
        let distance_km = match self.display_unit {
            DistanceUnit::Kilometers => distance,
            DistanceUnit::Miles => distance * MILES_TO_KM,
        };
        self.score(distance_km, candidate_local_density)
    }

    /// Batch-compute local density for a user's location from the database.
    /// Counts users within `density_bandwidth_km` using PostGIS or Haversine.
    pub async fn local_density(
        &self,
        pool: &PgPool,
        user_id: i32,
    ) -> f64 {
        // Use haversine approximation via lat/lng columns
        let count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM users u1
               CROSS JOIN LATERAL (
                   SELECT latitude, longitude FROM users WHERE id = $1
               ) me
               WHERE u1.id != $1
                 AND u1.latitude IS NOT NULL
                 AND me.latitude IS NOT NULL
                 AND (
                     6371 * acos(
                         LEAST(1.0, cos(radians(me.latitude)) * cos(radians(u1.latitude))
                         * cos(radians(u1.longitude) - radians(me.longitude))
                         + sin(radians(me.latitude)) * sin(radians(u1.latitude)))
                     )
                 ) <= $2"#,
        )
        .bind(user_id)
        .bind(self.density_bandwidth_km)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        (count as f64).max(self.density_floor)
    }

    /// Haversine distance between two lat/lng points (km).
    pub fn haversine(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
        let r = 6371.0; // Earth radius km
        let d_lat = (lat2 - lat1).to_radians();
        let d_lng = (lng2 - lng1).to_radians();
        let a = (d_lat / 2.0).sin().powi(2)
            + lat1.to_radians().cos()
                * lat2.to_radians().cos()
                * (d_lng / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        r * c
    }
}

/// Location cluster for density smoothing.
/// Groups nearby users into clusters so we can precompute density per region.
#[derive(Debug, Clone)]
pub struct LocationCluster {
    pub center_lat: f64,
    pub center_lng: f64,
    pub user_count: u64,
    pub radius_km: f64,
}

impl LocationCluster {
    /// Simple grid-based clustering from the database.
    /// Divides the world into ~50km cells and counts users per cell.
    pub async fn build_clusters(pool: &PgPool) -> Vec<Self> {
        #[derive(sqlx::FromRow)]
        struct Cell {
            lat_bucket: Option<f64>,
            lng_bucket: Option<f64>,
            cnt: Option<i64>,
        }

        let cells = sqlx::query_as::<_, Cell>(
            r#"SELECT
                 ROUND(latitude::numeric * 2, 0) / 2 AS lat_bucket,
                 ROUND(longitude::numeric * 2, 0) / 2 AS lng_bucket,
                 COUNT(*) AS cnt
               FROM users
               WHERE latitude IS NOT NULL AND longitude IS NOT NULL
               GROUP BY lat_bucket, lng_bucket
               HAVING COUNT(*) >= 2"#,
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        cells
            .into_iter()
            .filter_map(|c| {
                Some(LocationCluster {
                    center_lat: c.lat_bucket?,
                    center_lng: c.lng_bucket?,
                    user_count: c.cnt? as u64,
                    radius_km: 25.0,
                })
            })
            .collect()
    }

    /// Find the density at a given point by looking up the nearest cluster.
    pub fn density_at(clusters: &[LocationCluster], lat: f64, lng: f64) -> f64 {
        let mut best_density = 0.0;
        let mut best_dist = f64::MAX;
        for cluster in clusters {
            let dist = GeoScorer::haversine(lat, lng, cluster.center_lat, cluster.center_lng);
            if dist < cluster.radius_km && dist < best_dist {
                best_dist = dist;
                best_density = cluster.user_count as f64;
            }
        }
        best_density.max(5.0) // floor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gravity_score() {
        let scorer = GeoScorer::default();
        assert!((scorer.gravity_score(0.0) - 1.0).abs() < 0.01);
        assert!(scorer.gravity_score(10.0) > scorer.gravity_score(50.0));
        assert!(scorer.gravity_score(200.0) < 0.01);
    }

    #[test]
    fn test_density_factor() {
        let scorer = GeoScorer::default();
        // Dense area: factor ≈ 1.0
        let dense = scorer.density_factor(50.0);
        assert!((dense - 1.0).abs() < 0.1);
        // Sparse area: factor > 1.0
        let sparse = scorer.density_factor(5.0);
        assert!(sparse > 1.5);
    }

    #[test]
    fn test_haversine() {
        // NYC to LA ≈ 3944 km ≈ 2451 miles
        let d_km = GeoScorer::haversine(40.7128, -74.0060, 34.0522, -118.2437);
        assert!((d_km - 3944.0).abs() < 50.0);
        let d_mi = km_to_miles(d_km);
        assert!((d_mi - 2451.0).abs() < 50.0);
        // Same point
        assert!(GeoScorer::haversine(0.0, 0.0, 0.0, 0.0) < 0.001);
    }

    #[test]
    fn test_miles_scorer() {
        let scorer = GeoScorer::with_miles(60.0);
        assert_eq!(scorer.display_unit, DistanceUnit::Miles);
        // 60 miles ≈ 96.56 km
        assert!((scorer.max_distance_km - 96.56).abs() < 1.0);

        // Score at 5 km ≈ 3.1 miles — should have strong gravity
        let result = scorer.score(5.0, 50.0);
        assert!((result.distance_miles - 3.11).abs() < 0.1);
        assert!(result.gravity_score > 0.5);
    }

    #[test]
    fn test_score_in_display_unit() {
        let scorer = GeoScorer::with_miles(60.0);
        // Pass distance in miles
        let result = scorer.score_in_display_unit(10.0, 50.0);
        // 10 miles ≈ 16.09 km
        assert!((result.distance_km - 16.09).abs() < 0.1);
        assert!((result.distance_miles - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_display_distance() {
        let scorer = GeoScorer::default();
        let result = scorer.score(25.0, 50.0);
        assert_eq!(result.display_distance(DistanceUnit::Kilometers), "25 km");
        assert_eq!(result.display_distance(DistanceUnit::Miles), "16 mi");

        // Sub-km distance
        let close = scorer.score(0.5, 50.0);
        assert_eq!(close.display_distance(DistanceUnit::Kilometers), "500 m");
    }

    #[test]
    fn test_unit_conversions() {
        assert!((km_to_miles(1.0) - 0.621371).abs() < 0.001);
        assert!((miles_to_km(1.0) - 1.60934).abs() < 0.001);
        // Round-trip
        assert!((km_to_miles(miles_to_km(42.0)) - 42.0).abs() < 0.001);
    }
}
