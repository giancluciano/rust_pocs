//! Transparent, deterministic scoring so the group can see why an option
//! ranks where it does. The LLM writes pitches; it does not pick the order.
//!
//! The group's preference: everyone spends the day in ONE main house, and a
//! smaller nearby house is only for sleeping. So location and pool are judged
//! on the main house, and pairs score higher the more of the group the main
//! house holds and the shorter the walk between houses. A single house that
//! fits everyone gets full marks on both.

use crate::options::GroupOption;

#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub beach: f64,
    pub center: f64,
    pub pool: f64,
    pub price: f64,
    pub rating: f64,
    /// How much of the group sleeps in the main house.
    pub main_house: f64,
    /// Short walk between the main and the sleeping house.
    pub walk: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            beach: 0.25,
            center: 0.10,
            pool: 0.20,
            price: 0.15,
            rating: 0.05,
            main_house: 0.20,
            walk: 0.05,
        }
    }
}

impl Weights {
    fn total(&self) -> f64 {
        self.beach
            + self.center
            + self.pool
            + self.price
            + self.rating
            + self.main_house
            + self.walk
    }
}

/// Distance at which the beach / center / walk sub-score drops to 0.5.
const BEACH_HALF_KM: f64 = 1.0;
const CENTER_HALF_KM: f64 = 5.0;
const WALK_HALF_KM: f64 = 0.2;
/// Main-house share of the group that scores 0 (even split) and 1 (mostly in the main house).
const EVEN_SPLIT_SHARE: f64 = 0.5;
const IDEAL_MAIN_SHARE: f64 = 0.8;

fn closeness(km: f64, half_km: f64) -> f64 {
    half_km / (half_km + km.max(0.0))
}

fn main_house_score(option: &GroupOption, group_size: u32) -> f64 {
    ((option.main_share(group_size) - EVEN_SPLIT_SHARE) / (IDEAL_MAIN_SHARE - EVEN_SPLIT_SHARE))
        .clamp(0.0, 1.0)
}

/// Scores in 0..=100, one per option, in the same order.
pub fn score_options(options: &[GroupOption], group_size: u32, weights: &Weights) -> Vec<f64> {
    let cheapest = options
        .iter()
        .map(|o| o.price_per_person_per_night(group_size))
        .fold(f64::INFINITY, f64::min);

    options
        .iter()
        .map(|o| {
            let main = o.main_house();
            let beach = main
                .distance_beach_km
                .map_or(0.0, |km| closeness(km, BEACH_HALF_KM));
            let center = closeness(main.distance_center_km, CENTER_HALF_KM);
            let pool = if main.has_pool { 1.0 } else { 0.0 };
            let price = cheapest / o.price_per_person_per_night(group_size).max(f64::EPSILON);
            let rating = o
                .avg_rating()
                .map_or(0.5, |r| ((r - 3.0) / 2.0).clamp(0.0, 1.0));
            let walk = o
                .pair_distance_m
                .map_or(1.0, |m| closeness(m / 1000.0, WALK_HALF_KM));

            let weighted = weights.beach * beach
                + weights.center * center
                + weights.pool * pool
                + weights.price * price
                + weights.rating * rating
                + weights.main_house * main_house_score(o, group_size)
                + weights.walk * walk;
            100.0 * weighted / weights.total()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::listing;
    use crate::enrich::EnrichedListing;
    use crate::options::OptionKind;

    fn option(id: &str, beach_km: f64, price: f64, pool: bool) -> GroupOption {
        let l = listing(id, 0.0, 0.0, 20, price);
        let e = EnrichedListing {
            listing: l,
            distance_center_km: 2.0,
            nearest_beach: Some("B".into()),
            distance_beach_km: Some(beach_km),
            has_pool: pool,
            realistic_capacity: 20,
            red_flags: vec![],
            vetted: true,
        };
        GroupOption {
            id: id.into(),
            kind: OptionKind::Single,
            listings: vec![e],
            pair_distance_m: None,
        }
    }

    #[test]
    fn closer_to_beach_scores_higher() {
        let scores = score_options(
            &[
                option("near", 0.1, 1000.0, false),
                option("far", 5.0, 1000.0, false),
            ],
            20,
            &Weights::default(),
        );
        assert!(scores[0] > scores[1]);
    }

    #[test]
    fn pool_and_price_matter() {
        let scores = score_options(
            &[
                option("pool", 1.0, 1000.0, true),
                option("cheap", 1.0, 500.0, false),
            ],
            20,
            &Weights::default(),
        );
        // pool gives +20, being 2x pricier costs 10
        assert!(scores[0] > scores[1]);
    }

    fn pair(main_cap: u32, sleep_cap: u32, main_pool: bool, sleep_pool: bool) -> GroupOption {
        let house = |id: &str, cap: u32, pool: bool| EnrichedListing {
            realistic_capacity: cap,
            has_pool: pool,
            ..option(id, 0.5, 500.0, pool).listings[0].clone()
        };
        GroupOption {
            id: format!("P-{main_cap}+{sleep_cap}"),
            kind: OptionKind::Pair,
            listings: vec![
                house("main", main_cap, main_pool),
                house("sleep", sleep_cap, sleep_pool),
            ],
            pair_distance_m: Some(100.0),
        }
    }

    #[test]
    fn big_main_house_beats_even_split() {
        let scores = score_options(
            &[pair(14, 4, true, false), pair(9, 9, true, false)],
            17,
            &Weights::default(),
        );
        assert!(scores[0] > scores[1]);
    }

    #[test]
    fn only_the_main_house_pool_counts() {
        let scores = score_options(
            &[pair(12, 6, true, false), pair(12, 6, false, true)],
            17,
            &Weights::default(),
        );
        assert!(scores[0] > scores[1]);
        let same = score_options(
            &[pair(12, 6, true, false), pair(12, 6, true, true)],
            17,
            &Weights::default(),
        );
        assert!((same[0] - same[1]).abs() < 1e-9);
    }

    #[test]
    fn single_house_beats_equivalent_pair() {
        let single = option("single", 0.5, 1000.0, true);
        let scores = score_options(&[single, pair(14, 6, true, false)], 20, &Weights::default());
        assert!(scores[0] > scores[1]);
    }

    #[test]
    fn scores_are_within_bounds() {
        let scores = score_options(&[option("a", 0.0, 1.0, true)], 20, &Weights::default());
        assert!(scores[0] > 0.0 && scores[0] <= 100.0);
    }
}
