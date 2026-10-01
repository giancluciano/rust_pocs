use crate::domain::{Beach, GeoPoint};

const EARTH_RADIUS_KM: f64 = 6371.0;

/// Great-circle distance in kilometres. Straight line, not walking distance.
pub fn haversine_km(a: GeoPoint, b: GeoPoint) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.sqrt().asin()
}

pub fn nearest_beach(point: GeoPoint, beaches: &[Beach]) -> Option<(&Beach, f64)> {
    beaches
        .iter()
        .map(|b| (b, haversine_km(point, b.location)))
        .min_by(|x, y| x.1.total_cmp(&y.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_to_self_is_zero() {
        let p = GeoPoint {
            lat: -27.59,
            lon: -48.54,
        };
        assert!(haversine_km(p, p).abs() < 1e-9);
    }

    #[test]
    fn one_degree_latitude_is_about_111_km() {
        let a = GeoPoint { lat: 0.0, lon: 0.0 };
        let b = GeoPoint { lat: 1.0, lon: 0.0 };
        assert!((haversine_km(a, b) - 111.19).abs() < 0.1);
    }

    #[test]
    fn nearest_beach_picks_closest() {
        let beaches = vec![
            Beach {
                name: "far".into(),
                location: GeoPoint { lat: 1.0, lon: 0.0 },
            },
            Beach {
                name: "near".into(),
                location: GeoPoint {
                    lat: 0.01,
                    lon: 0.0,
                },
            },
        ];
        let (beach, km) = nearest_beach(GeoPoint { lat: 0.0, lon: 0.0 }, &beaches).unwrap();
        assert_eq!(beach.name, "near");
        assert!((km - 1.11).abs() < 0.01);
    }

    #[test]
    fn nearest_beach_none_when_no_beaches() {
        assert!(nearest_beach(GeoPoint { lat: 0.0, lon: 0.0 }, &[]).is_none());
    }
}
