//! Airbnb scraper, ported from swarms_poc. OWNER: scraper agent.
//!
//! One `AirbnbSearch` is shared by every city, so all Airbnb requests go
//! through a single rate limiter and cookie session.

use std::path::Path;
use std::time::Duration;

use crate::domain::{Listing, Stay};

#[derive(Debug, Clone)]
pub struct AirbnbConfig {
    /// Search result pages fetched per city.
    pub max_pages: u32,
    /// Listing detail pages fetched per city (real capacity, rating, amenities).
    pub max_listing_details: u32,
    /// Minimum time between Airbnb requests; never below 1 second.
    pub min_interval: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum AirbnbError {
    #[error(transparent)]
    Http(#[from] crate::http::HttpError),
    #[error("could not understand the page: {0}")]
    Parse(String),
    #[error("could not locate '{0}' on OpenStreetMap")]
    UnknownCity(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct AirbnbSearch {
    _private: (),
}

impl AirbnbSearch {
    pub fn new(config: AirbnbConfig, cache_dir: &Path) -> Result<Self, AirbnbError> {
        let _ = (config, cache_dir);
        todo!("scraper agent")
    }

    /// Entire homes in `city_query` (e.g. "Bombinhas, Santa Catarina, Brazil")
    /// for `guests` people over `stay`, cheapest first. The city is geocoded on
    /// OpenStreetMap to bound the search area. Listings whose known capacity is
    /// below `guests` are dropped.
    pub async fn search_city(
        &self,
        city_query: &str,
        stay: &Stay,
        guests: u32,
    ) -> Result<Vec<Listing>, AirbnbError> {
        let _ = (city_query, stay, guests);
        todo!("scraper agent")
    }
}
