//! Builds the voting options: single places that fit everyone, and pairs of
//! a *main house* (the bigger one, where everyone spends the day) plus a
//! nearby *sleeping house* for whoever doesn't fit. Pure functions.

use std::cmp::Ordering;

use serde::Serialize;

use crate::enrich::EnrichedListing;
use crate::geo::haversine_km;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OptionKind {
    Single,
    Pair,
}

/// `listings[0]` is the main house; `listings[1]` (pairs only) the sleeping house.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupOption {
    pub id: String,
    pub kind: OptionKind,
    pub listings: Vec<EnrichedListing>,
    pub pair_distance_m: Option<f64>,
}

impl GroupOption {
    pub fn main_house(&self) -> &EnrichedListing {
        &self.listings[0]
    }

    pub fn sleeping_house(&self) -> Option<&EnrichedListing> {
        self.listings.get(1)
    }

    /// Fraction of the group that sleeps in the main house (capped at 1).
    pub fn main_share(&self, group_size: u32) -> f64 {
        (f64::from(self.main_house().realistic_capacity) / f64::from(group_size.max(1))).min(1.0)
    }

    pub fn capacity(&self) -> u32 {
        self.listings.iter().map(|l| l.realistic_capacity).sum()
    }

    pub fn bedrooms(&self) -> u32 {
        self.listings.iter().map(|l| l.listing.bedrooms).sum()
    }

    pub fn beds(&self) -> u32 {
        self.listings.iter().map(|l| l.listing.beds).sum()
    }

    pub fn price_per_night(&self) -> f64 {
        self.listings
            .iter()
            .map(|l| l.listing.price_per_night)
            .sum()
    }

    pub fn price_per_person_per_night(&self, group_size: u32) -> f64 {
        self.price_per_night() / f64::from(group_size.max(1))
    }

    /// Review-count-weighted average rating.
    pub fn avg_rating(&self) -> Option<f64> {
        let rated: Vec<(f64, f64)> = self
            .listings
            .iter()
            .filter_map(|l| {
                l.listing
                    .rating
                    .map(|r| (f64::from(r), f64::from(l.listing.review_count.max(1))))
            })
            .collect();
        let weight: f64 = rated.iter().map(|(_, w)| w).sum();
        (weight > 0.0).then(|| rated.iter().map(|(r, w)| r * w).sum::<f64>() / weight)
    }

    pub fn review_count(&self) -> u32 {
        self.listings.iter().map(|l| l.listing.review_count).sum()
    }

    /// Red flags labelled by house role (never by listing id).
    pub fn red_flags(&self) -> Vec<String> {
        let label = |i: usize| match (self.kind, i) {
            (OptionKind::Single, _) => "",
            (OptionKind::Pair, 0) => "Main house: ",
            (OptionKind::Pair, _) => "Sleeping house: ",
        };
        self.listings
            .iter()
            .enumerate()
            .flat_map(|(i, l)| l.red_flags.iter().map(move |f| format!("{}{f}", label(i))))
            .collect()
    }
}

pub fn single_options(listings: &[EnrichedListing], group_size: u32) -> Vec<GroupOption> {
    listings
        .iter()
        .filter(|l| l.realistic_capacity >= group_size)
        .map(|l| GroupOption {
            id: format!("S-{}", l.id()),
            kind: OptionKind::Single,
            listings: vec![l.clone()],
            pair_distance_m: None,
        })
        .collect()
}

/// Which house makes the better hub: more capacity, then a pool, then more
/// bedrooms, then a better rating.
fn hub_order(a: &EnrichedListing, b: &EnrichedListing) -> Ordering {
    a.realistic_capacity
        .cmp(&b.realistic_capacity)
        .then(a.has_pool.cmp(&b.has_pool))
        .then(a.listing.bedrooms.cmp(&b.listing.bedrooms))
        .then(
            a.listing
                .rating
                .unwrap_or(0.0)
                .total_cmp(&b.listing.rating.unwrap_or(0.0)),
        )
}

/// Pairs within `max_distance_m` that together fit the group, ordered
/// main house first. Pairs where one place alone already fits everyone are
/// skipped (that's a single option).
pub fn pair_options(
    listings: &[EnrichedListing],
    group_size: u32,
    max_distance_m: f64,
) -> Vec<GroupOption> {
    let fits_alone = |l: &EnrichedListing| l.realistic_capacity >= group_size;
    listings
        .iter()
        .enumerate()
        .flat_map(|(i, a)| listings[i + 1..].iter().map(move |b| (a, b)))
        .filter(|(a, b)| !fits_alone(a) && !fits_alone(b))
        .filter(|(a, b)| a.realistic_capacity + b.realistic_capacity >= group_size)
        .filter_map(|(a, b)| {
            let distance_m = haversine_km(a.listing.location, b.listing.location) * 1000.0;
            let (main, sleeping) = if hub_order(a, b).is_ge() {
                (a, b)
            } else {
                (b, a)
            };
            (distance_m <= max_distance_m).then(|| GroupOption {
                id: format!("P-{}+{}", main.id(), sleeping.id()),
                kind: OptionKind::Pair,
                listings: vec![main.clone(), sleeping.clone()],
                pair_distance_m: Some(distance_m),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::listing;
    use crate::domain::{Destination, GeoPoint};

    fn enriched(id: &str, lat: f64, lon: f64, capacity: u32) -> EnrichedListing {
        let dest = Destination {
            city: "T".into(),
            country: "T".into(),
            center: GeoPoint { lat: 0.0, lon: 0.0 },
            beaches: vec![],
        };
        EnrichedListing::new(
            listing(id, lat, lon, capacity, 100.0 * f64::from(capacity)),
            &dest,
        )
    }

    #[test]
    fn singles_only_include_places_that_fit_everyone() {
        let listings = vec![
            enriched("big", 0.0, 0.0, 20),
            enriched("small", 0.0, 0.0, 12),
        ];
        let singles = single_options(&listings, 20);
        assert_eq!(singles.len(), 1);
        assert_eq!(singles[0].id, "S-big");
    }

    #[test]
    fn pairs_must_be_close_and_fit_together() {
        // ~111 m apart per 0.001 deg latitude
        let listings = vec![
            enriched("a", 0.0, 0.0, 10),
            enriched("b", 0.001, 0.0, 10), // 111 m from a
            enriched("c", 0.01, 0.0, 10),  // 1.1 km from a
            enriched("d", 0.0005, 0.0, 6), // too small to pair with a/b
        ];
        let pairs = pair_options(&listings, 20, 500.0);
        let ids: Vec<_> = pairs.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["P-a+b"]);
        assert!((pairs[0].pair_distance_m.unwrap() - 111.19).abs() < 0.5);
    }

    #[test]
    fn pairs_skip_places_that_fit_alone() {
        let listings = vec![enriched("huge", 0.0, 0.0, 20), enriched("b", 0.0, 0.0, 10)];
        assert!(pair_options(&listings, 20, 500.0).is_empty());
    }

    #[test]
    fn bigger_house_becomes_main_house() {
        let listings = vec![
            enriched("small", 0.0, 0.0, 6),
            enriched("big", 0.001, 0.0, 14),
        ];
        let pairs = pair_options(&listings, 17, 500.0);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].id, "P-big+small");
        assert_eq!(pairs[0].main_house().id(), "big");
        assert_eq!(pairs[0].sleeping_house().unwrap().id(), "small");
        assert!((pairs[0].main_share(17) - 14.0 / 17.0).abs() < 1e-9);
    }

    #[test]
    fn pool_breaks_capacity_tie_for_main_house() {
        let plain = enriched("plain", 0.0, 0.0, 10);
        let pool = EnrichedListing {
            has_pool: true,
            ..enriched("pool", 0.001, 0.0, 10)
        };
        let pairs = pair_options(&[plain, pool], 20, 500.0);
        assert_eq!(pairs[0].main_house().id(), "pool");
    }

    #[test]
    fn metrics_and_labelled_red_flags() {
        let a = EnrichedListing {
            red_flags: vec!["no towels".into()],
            ..enriched("a", 0.0, 0.01, 12)
        };
        let b = EnrichedListing {
            red_flags: vec!["1 bathroom".into()],
            ..enriched("b", 0.0, 0.02, 8)
        };
        let option = GroupOption {
            id: "P-a+b".into(),
            kind: OptionKind::Pair,
            listings: vec![a, b],
            pair_distance_m: Some(1.0),
        };
        assert_eq!(option.capacity(), 20);
        assert_eq!(option.price_per_night(), 2000.0);
        assert_eq!(option.price_per_person_per_night(20), 100.0);
        assert_eq!(option.avg_rating(), Some(4.5));
        assert_eq!(
            option.red_flags(),
            vec![
                "Main house: no towels".to_string(),
                "Sleeping house: 1 bathroom".to_string()
            ]
        );
    }
}
