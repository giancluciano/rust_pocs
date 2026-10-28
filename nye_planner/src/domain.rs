//! Core data types shared by every module. Values are built once and never
//! mutated; agents report results through tools, which store new values.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

/// Check-in / check-out, half-open: the checkout night is not included.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stay {
    checkin: NaiveDate,
    checkout: NaiveDate,
}

impl Stay {
    pub fn new(checkin: NaiveDate, checkout: NaiveDate) -> Result<Self, String> {
        if checkout <= checkin {
            return Err(format!("checkout ({checkout}) must be after checkin ({checkin})"));
        }
        Ok(Self { checkin, checkout })
    }

    pub fn checkin(&self) -> NaiveDate {
        self.checkin
    }

    pub fn checkout(&self) -> NaiveDate {
        self.checkout
    }

    pub fn nights(&self) -> u32 {
        (self.checkout - self.checkin).num_days() as u32
    }

    /// The Dec 31 that falls inside the stay, if the stay covers New Year's Eve night.
    pub fn new_years_eve(&self) -> Option<NaiveDate> {
        self.checkin
            .iter_days()
            .take_while(|d| *d < self.checkout)
            .find(|d| d.month() == 12 && d.day() == 31)
    }
}

/// What the user asked for on the command line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TripRequest {
    /// Free text, e.g. "the coastline of Santa Catarina".
    pub region: String,
    pub people: u32,
    pub stay: Stay,
}

/// A city the region planner thinks is worth checking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateCity {
    pub name: String,
    /// State / province, used to disambiguate searches ("Bombinhas, SC").
    pub state: String,
    pub country: String,
    /// One line on why the planner picked it.
    pub why: String,
}

impl CandidateCity {
    /// "Bombinhas, Santa Catarina, Brazil": the string used for geocoding and search.
    pub fn full_name(&self) -> String {
        [&self.name, &self.state, &self.country]
            .iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What a fireworks researcher found for one city.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksEvidence {
    /// Matches `CandidateCity::name`.
    pub city: String,
    /// True only when a source announces a fireworks show for this New Year's Eve.
    pub confirmed: bool,
    /// One or two sentences: where, when, who announced it.
    pub summary: String,
    pub source_url: Option<String>,
    pub source_title: Option<String>,
}

/// One Airbnb listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub url: String,
    pub location: GeoPoint,
    /// Guest capacity from the listing page, or the search's guest filter if
    /// the detail page was not fetched.
    pub max_guests: u32,
    pub bedrooms: u32,
    pub beds: u32,
    pub bathrooms: f32,
    /// Total price for the stay divided by its nights.
    pub price_per_night: f64,
    pub currency: String,
    pub rating: Option<f32>,
    pub review_count: u32,
    pub amenities: Vec<String>,
    pub description: String,
}

impl Listing {
    pub fn has_pool(&self) -> bool {
        self.amenities.iter().any(|a| a.to_lowercase().contains("pool"))
    }

    pub fn price_per_person_per_night(&self, people: u32) -> f64 {
        self.price_per_night / f64::from(people.max(1))
    }
}

/// A listing the curator recommends, with its reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StayPick {
    pub listing_id: String,
    pub reason: String,
}

/// Final result for one city: the input to the markdown report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CityPlan {
    pub city: CandidateCity,
    pub fireworks: FireworksEvidence,
    /// Short travel-writer paragraph about New Year's Eve in this city.
    pub notes: Option<String>,
    /// Picked listings in the curator's order, each with its reason.
    pub stays: Vec<(Listing, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn stay_rejects_checkout_before_checkin() {
        assert!(Stay::new(date("2026-12-31"), date("2026-12-30")).is_err());
    }

    #[test]
    fn stay_counts_nights() {
        let stay = Stay::new(date("2026-12-30"), date("2027-01-02")).unwrap();
        assert_eq!(stay.nights(), 3);
    }

    #[test]
    fn stay_finds_new_years_eve() {
        let stay = Stay::new(date("2026-12-30"), date("2027-01-02")).unwrap();
        assert_eq!(stay.new_years_eve(), Some(date("2026-12-31")));
    }

    #[test]
    fn stay_without_new_years_eve() {
        let stay = Stay::new(date("2027-01-01"), date("2027-01-03")).unwrap();
        assert_eq!(stay.new_years_eve(), None);
        // Checking out on Dec 31 means the night of the 31st is not included.
        let stay = Stay::new(date("2026-12-29"), date("2026-12-31")).unwrap();
        assert_eq!(stay.new_years_eve(), None);
    }

    #[test]
    fn full_name_skips_empty_parts() {
        let city = CandidateCity {
            name: "Bombinhas".into(),
            state: "Santa Catarina".into(),
            country: " ".into(),
            why: String::new(),
        };
        assert_eq!(city.full_name(), "Bombinhas, Santa Catarina");
    }
}
