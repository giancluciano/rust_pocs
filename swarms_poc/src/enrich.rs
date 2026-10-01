use serde::Serialize;

use crate::domain::{Destination, Listing};
use crate::geo::{haversine_km, nearest_beach};

/// Result of the Vetting Agent's review of one listing.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListingReview {
    /// Listing id exactly as provided
    pub id: String,
    /// Adults who can sleep in a real bed or proper sofa-bed (no floor mattresses)
    pub realistic_capacity: u32,
    /// True only if guests can use a pool on the property (or clearly shared but accessible)
    pub pool_confirmed: bool,
    /// Short warnings for the group, e.g. "floor mattresses", "shared condo pool"
    #[serde(default)]
    pub red_flags: Vec<String>,
}

/// A listing plus everything computed about it (distances, pool, vetting).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnrichedListing {
    pub listing: Listing,
    pub distance_center_km: f64,
    pub nearest_beach: Option<String>,
    pub distance_beach_km: Option<f64>,
    pub has_pool: bool,
    pub realistic_capacity: u32,
    pub red_flags: Vec<String>,
    pub vetted: bool,
}

impl EnrichedListing {
    pub fn new(listing: Listing, destination: &Destination) -> Self {
        let beach = nearest_beach(listing.location, &destination.beaches);
        Self {
            distance_center_km: haversine_km(listing.location, destination.center),
            nearest_beach: beach.map(|(b, _)| b.name.clone()),
            distance_beach_km: beach.map(|(_, km)| km),
            has_pool: listing.has_pool_amenity(),
            realistic_capacity: listing.max_guests,
            red_flags: vec![],
            vetted: false,
            listing,
        }
    }

    /// Applies a vetting review. Capacity can only go down: the host's
    /// `max_guests` is an upper bound the agent may not exceed.
    pub fn with_review(&self, review: &ListingReview) -> Self {
        Self {
            realistic_capacity: review.realistic_capacity.min(self.listing.max_guests),
            has_pool: review.pool_confirmed,
            red_flags: review.red_flags.clone(),
            vetted: true,
            ..self.clone()
        }
    }

    pub fn id(&self) -> &str {
        &self.listing.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::listing;
    use crate::domain::{Beach, GeoPoint};

    fn destination() -> Destination {
        Destination {
            city: "Test".into(),
            country: "Nowhere".into(),
            center: GeoPoint { lat: 0.0, lon: 0.0 },
            beaches: vec![Beach {
                name: "Sunny".into(),
                location: GeoPoint {
                    lat: 0.0,
                    lon: 0.01,
                },
            }],
        }
    }

    #[test]
    fn computes_distances_and_defaults() {
        let e = EnrichedListing::new(listing("a", 0.0, 0.02, 10, 100.0), &destination());
        assert!((e.distance_center_km - 2.22).abs() < 0.01);
        assert_eq!(e.nearest_beach.as_deref(), Some("Sunny"));
        assert!((e.distance_beach_km.unwrap() - 1.11).abs() < 0.01);
        assert_eq!(e.realistic_capacity, 10);
        assert!(!e.vetted);
    }

    #[test]
    fn review_cannot_raise_capacity_and_does_not_mutate_original() {
        let original = EnrichedListing::new(listing("a", 0.0, 0.0, 10, 100.0), &destination());
        let review = ListingReview {
            id: "a".into(),
            realistic_capacity: 14,
            pool_confirmed: true,
            red_flags: vec!["stairs".into()],
        };
        let reviewed = original.with_review(&review);
        assert_eq!(reviewed.realistic_capacity, 10);
        assert!(reviewed.has_pool && reviewed.vetted);
        assert_eq!(reviewed.red_flags, vec!["stairs".to_string()]);
        assert!(!original.vetted);
    }
}
