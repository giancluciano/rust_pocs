pub mod airbnb;
pub mod fixture;

use std::future::Future;
use std::pin::Pin;

use crate::domain::{Destination, Listing, Stay};

#[derive(Debug, Clone, PartialEq)]
pub struct SearchQuery {
    pub stay: Stay,
    pub min_guests: u32,
    pub max_guests: Option<u32>,
    pub min_bedrooms: Option<u32>,
    pub max_price_per_night: Option<f64>,
}

impl SearchQuery {
    pub fn matches(&self, listing: &Listing) -> bool {
        listing.is_available(&self.stay)
            && listing.max_guests >= self.min_guests
            && self.max_guests.is_none_or(|max| listing.max_guests <= max)
            && self.min_bedrooms.is_none_or(|min| listing.bedrooms >= min)
            && self
                .max_price_per_night
                .is_none_or(|max| listing.price_per_night <= max)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("fixture not found for city '{city}' (looked for {path})")]
    FixtureNotFound { city: String, path: String },
    #[error("failed to read fixture: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid fixture JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] crate::http::HttpError),
    #[error("could not understand the page: {0}")]
    Parse(String),
    #[error("could not locate '{0}' on OpenStreetMap")]
    UnknownCity(String),
}

pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

/// Source of rental listings: the offline fixture or the Airbnb scraper.
pub trait ListingProvider: Send + Sync {
    fn destination(&self) -> &Destination;
    fn search<'a>(&'a self, query: &'a SearchQuery) -> ProviderFuture<'a, Vec<Listing>>;
}
