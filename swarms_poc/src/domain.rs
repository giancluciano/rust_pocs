//! Core data types. Values are built once and never mutated; "updates"
//! (e.g. applying a vetting review) return new values.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beach {
    pub name: String,
    pub location: GeoPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Destination {
    pub city: String,
    pub country: String,
    pub center: GeoPoint,
    pub beaches: Vec<Beach>,
}

/// Half-open date range `[from, to)`, matching check-in / check-out semantics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl DateRange {
    pub fn overlaps(&self, other: &DateRange) -> bool {
        self.from < other.to && other.from < self.to
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stay {
    range: DateRange,
}

impl Stay {
    pub fn new(checkin: NaiveDate, checkout: NaiveDate) -> Result<Self, String> {
        if checkout <= checkin {
            return Err(format!(
                "checkout ({checkout}) must be after checkin ({checkin})"
            ));
        }
        Ok(Self {
            range: DateRange {
                from: checkin,
                to: checkout,
            },
        })
    }

    pub fn checkin(&self) -> NaiveDate {
        self.range.from
    }

    pub fn checkout(&self) -> NaiveDate {
        self.range.to
    }

    pub fn nights(&self) -> u32 {
        (self.range.to - self.range.from).num_days() as u32
    }

    pub fn range(&self) -> &DateRange {
        &self.range
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub url: String,
    pub location: GeoPoint,
    pub max_guests: u32,
    pub bedrooms: u32,
    pub beds: u32,
    pub bathrooms: f32,
    pub price_per_night: f64,
    pub currency: String,
    pub rating: Option<f32>,
    pub review_count: u32,
    pub amenities: Vec<String>,
    pub description: String,
    #[serde(default)]
    pub booked: Vec<DateRange>,
}

impl Listing {
    pub fn has_pool_amenity(&self) -> bool {
        self.amenities
            .iter()
            .any(|a| a.to_lowercase().contains("pool"))
    }

    pub fn is_available(&self, stay: &Stay) -> bool {
        !self.booked.iter().any(|b| b.overlaps(stay.range()))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn listing(id: &str, lat: f64, lon: f64, max_guests: u32, price: f64) -> Listing {
        Listing {
            id: id.to_string(),
            name: format!("Listing {id}"),
            url: format!("https://example.com/{id}"),
            location: GeoPoint { lat, lon },
            max_guests,
            bedrooms: max_guests / 2,
            beds: max_guests / 2,
            bathrooms: 2.0,
            price_per_night: price,
            currency: "BRL".to_string(),
            rating: Some(4.5),
            review_count: 10,
            amenities: vec!["Wifi".to_string()],
            description: String::new(),
            booked: vec![],
        }
    }

    pub fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn stay_rejects_checkout_before_checkin() {
        assert!(Stay::new(date("2027-01-10"), date("2027-01-10")).is_err());
        assert!(Stay::new(date("2027-01-10"), date("2027-01-09")).is_err());
    }

    #[test]
    fn stay_counts_nights() {
        let stay = Stay::new(date("2027-01-10"), date("2027-01-17")).unwrap();
        assert_eq!(stay.nights(), 7);
    }

    #[test]
    fn availability_respects_half_open_ranges() {
        let base = listing("a", 0.0, 0.0, 10, 100.0);
        let booked = Listing {
            booked: vec![DateRange {
                from: date("2027-01-12"),
                to: date("2027-01-15"),
            }],
            ..base
        };
        let overlapping = Stay::new(date("2027-01-10"), date("2027-01-13")).unwrap();
        let ends_at_booking = Stay::new(date("2027-01-08"), date("2027-01-12")).unwrap();
        let starts_at_checkout = Stay::new(date("2027-01-15"), date("2027-01-20")).unwrap();

        assert!(!booked.is_available(&overlapping));
        assert!(booked.is_available(&ends_at_booking));
        assert!(booked.is_available(&starts_at_checkout));
    }

    #[test]
    fn pool_amenity_is_case_insensitive() {
        let with_pool = Listing {
            amenities: vec!["Shared POOL".to_string()],
            ..listing("a", 0.0, 0.0, 4, 1.0)
        };
        assert!(with_pool.has_pool_amenity());
        assert!(!listing("b", 0.0, 0.0, 4, 1.0).has_pool_amenity());
    }
}
