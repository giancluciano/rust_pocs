//! Scrapes Airbnb's public search and listing pages.
//!
//! Volume is kept low on purpose: every request goes through a `PoliteClient`
//! (1 req/s, one at a time, backoff, circuit breaker, disk cache), search is
//! capped at `max_pages` per query and listing pages at `max_listing_details`
//! per run. Listing pages are fetched without dates so their cache is reused
//! across searches and runs.

pub mod parse;

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderValue};
use tokio::sync::Mutex;

use super::{ListingProvider, ProviderError, ProviderFuture, SearchQuery};
use crate::cache::DiskCache;
use crate::domain::{Destination, Listing, Stay};
use crate::http::{PoliteClient, PoliteConfig};
use crate::osm::{self, BoundingBox};
use parse::{ListingDetail, SearchHit};

const BASE_URL: &str = "https://www.airbnb.com";
/// A current desktop Chrome UA. One fixed identity: no rotation.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
/// Prices are always requested (and reported) in Brazilian reais.
pub const CURRENCY: &str = "BRL";
/// Airbnb's guest picker stops at 16.
const MAX_ADULTS_FILTER: u32 = 16;
const CACHE_TTL: Duration = Duration::from_secs(12 * 3600);

#[derive(Debug, Clone)]
pub struct AirbnbConfig {
    pub max_pages: u32,
    pub max_listing_details: u32,
    pub min_interval: Duration,
}

pub struct AirbnbProvider {
    /// Single client => single rate limiter and cookie session for the site.
    http: PoliteClient,
    destination: Destination,
    bbox: BoundingBox,
    config: AirbnbConfig,
    details: Mutex<HashMap<String, Option<ListingDetail>>>,
}

fn looks_blocked(body: &str) -> bool {
    !parse::has_page_state(body) && body.to_lowercase().contains("captcha")
}

fn airbnb_client(config: &AirbnbConfig, cache: DiskCache) -> Result<PoliteClient, ProviderError> {
    let mut polite = PoliteConfig {
        min_interval: config.min_interval,
        // pages per query x a handful of queries, plus listing pages, plus warm-up
        request_budget: config.max_pages * 10 + config.max_listing_details + 1,
        block_detector: looks_blocked,
        cache: Some(cache),
        warmup_url: Some(format!("{BASE_URL}/")),
        ..PoliteConfig::new("airbnb.com", USER_AGENT)
    };
    polite.headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );
    polite
        .headers
        .insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    Ok(PoliteClient::new(polite)?)
}

impl AirbnbProvider {
    /// Resolves the city on OpenStreetMap (center, bounding box, beaches) and
    /// prepares the Airbnb session. No Airbnb request is made yet.
    pub async fn connect(
        city: &str,
        config: AirbnbConfig,
        cache_dir: &Path,
    ) -> Result<Self, ProviderError> {
        let osm_http = osm::osm_client(cache_dir)?;
        let info = osm::geocode(&osm_http, city).await?;
        let beaches = osm::beaches(&osm_http, &info).await?;
        tracing::info!(
            city = %info.name, country = %info.country, beaches = beaches.len(),
            "destination resolved on OpenStreetMap"
        );

        let cache = DiskCache::new(cache_dir.join("airbnb"), CACHE_TTL)?;
        Ok(Self {
            http: airbnb_client(&config, cache)?,
            destination: Destination {
                city: info.name,
                country: info.country,
                center: info.center,
                beaches,
            },
            bbox: info.bbox,
            config,
            details: Mutex::new(HashMap::new()),
        })
    }

    fn search_url(&self, query: &SearchQuery, cursor: Option<&str>) -> String {
        let mut url = reqwest::Url::parse(BASE_URL).expect("static URL");
        url.path_segments_mut()
            .expect("base URL has a path")
            .extend(["s", self.destination.city.as_str(), "homes"]);
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("checkin", &query.stay.checkin().to_string())
                .append_pair("checkout", &query.stay.checkout().to_string())
                .append_pair(
                    "adults",
                    &query.min_guests.min(MAX_ADULTS_FILTER).to_string(),
                )
                .append_pair("room_types[]", "Entire home/apt")
                .append_pair("ne_lat", &self.bbox.north.to_string())
                .append_pair("ne_lng", &self.bbox.east.to_string())
                .append_pair("sw_lat", &self.bbox.south.to_string())
                .append_pair("sw_lng", &self.bbox.west.to_string())
                .append_pair("search_by_map", "true")
                .append_pair("search_type", "user_map_move")
                .append_pair("currency", CURRENCY)
                .append_pair("locale", "en");
            if let Some(n) = query.min_bedrooms {
                q.append_pair("min_bedrooms", &n.to_string());
            }
            if let Some(cursor) = cursor {
                q.append_pair("cursor", cursor);
            }
        }
        url.to_string()
    }

    fn listing_url(id: &str) -> String {
        format!("{BASE_URL}/rooms/{id}?locale=en")
    }

    /// Fetches (once per run) the listing page. `None` when the detail budget
    /// is used up or the page can't be parsed; the listing is then kept with
    /// search-page data only.
    async fn detail(
        &self,
        id: &str,
        referer: &str,
    ) -> Result<Option<ListingDetail>, ProviderError> {
        // Held across the fetch so concurrent searches never fetch the same
        // page twice; requests are serialized by the rate limiter anyway.
        let mut details = self.details.lock().await;
        if let Some(known) = details.get(id) {
            return Ok(known.clone());
        }
        if details.len() as u32 >= self.config.max_listing_details {
            tracing::warn!(id, "listing detail budget reached; using search data only");
            return Ok(None);
        }
        let detail = match self.http.get(&Self::listing_url(id), Some(referer)).await {
            Ok(html) => parse::parse_listing(&html)
                .inspect_err(|e| tracing::warn!(id, error = %e, "could not parse listing page"))
                .ok(),
            Err(e @ crate::http::HttpError::Status { .. }) => {
                tracing::warn!(id, error = %e, "listing page unavailable");
                None
            }
            Err(e) => return Err(e.into()),
        };
        details.insert(id.to_string(), detail.clone());
        Ok(detail)
    }

    async fn search_impl(&self, query: &SearchQuery) -> Result<Vec<Listing>, ProviderError> {
        let mut listings = Vec::new();
        let mut cursor: Option<String> = None;
        for page_index in 0..self.config.max_pages as usize {
            let url = self.search_url(query, cursor.as_deref());
            let page = parse::parse_search(&self.http.get(&url, None).await?, page_index)?;
            tracing::info!(
                page = page_index + 1,
                hits = page.hits.len(),
                "airbnb search page"
            );

            for hit in &page.hits {
                let detail = self.detail(&hit.id, &url).await?;
                let listing = to_listing(hit, detail.as_ref(), query);
                if query.matches(&listing) {
                    listings.push(listing);
                }
            }
            match page.next_cursor {
                Some(next) if !page.hits.is_empty() => cursor = Some(next),
                _ => break,
            }
        }
        tracing::info!(
            found = listings.len(),
            requests_so_far = self.http.requests_made(),
            "airbnb search done"
        );
        Ok(listings)
    }
}

fn price_per_night(hit: &SearchHit, stay: &Stay) -> f64 {
    match &hit.price {
        Some(p) if p.is_total => p.amount / f64::from(stay.nights().max(1)),
        Some(p) => p.amount,
        None => 0.0,
    }
}

/// Merges search-page data with listing-page data (listing page wins).
/// Without a listing page, capacity falls back to the search's guest filter,
/// which Airbnb guarantees as a lower bound.
pub fn to_listing(hit: &SearchHit, detail: Option<&ListingDetail>, query: &SearchQuery) -> Listing {
    let d = detail;
    Listing {
        id: hit.id.clone(),
        name: d
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| hit.name.clone()),
        url: format!("{BASE_URL}/rooms/{}", hit.id),
        location: d.and_then(|d| d.location).unwrap_or(hit.location),
        max_guests: d
            .and_then(|d| d.person_capacity)
            .unwrap_or_else(|| query.min_guests.min(MAX_ADULTS_FILTER)),
        bedrooms: d.and_then(|d| d.bedrooms).or(hit.bedrooms).unwrap_or(0),
        beds: d.and_then(|d| d.beds).or(hit.beds).unwrap_or(0),
        bathrooms: d.and_then(|d| d.bathrooms).or(hit.bathrooms).unwrap_or(0.0),
        price_per_night: price_per_night(hit, &query.stay),
        currency: CURRENCY.to_string(),
        rating: d.and_then(|d| d.rating).or(hit.rating),
        review_count: d.and_then(|d| d.review_count).unwrap_or(hit.review_count),
        amenities: d.map(|d| d.amenities.clone()).unwrap_or_default(),
        description: d
            .map(|d| d.description.clone())
            .unwrap_or_else(|| "(listing page not fetched: capacity is a lower bound)".into()),
        booked: vec![],
    }
}

impl ListingProvider for AirbnbProvider {
    fn destination(&self) -> &Destination {
        &self.destination
    }

    fn search<'a>(&'a self, query: &'a SearchQuery) -> ProviderFuture<'a, Vec<Listing>> {
        Box::pin(self.search_impl(query))
    }
}

#[cfg(test)]
mod tests {
    use super::parse::Price;
    use super::*;
    use crate::domain::GeoPoint;
    use crate::domain::test_support::date;

    fn hit() -> SearchHit {
        SearchHit {
            id: "123".into(),
            name: "Search name".into(),
            location: GeoPoint { lat: 1.0, lon: 2.0 },
            bedrooms: Some(8),
            beds: Some(14),
            bathrooms: Some(5.0),
            price: Some(Price {
                amount: 7000.0,
                is_total: true,
            }),
            rating: Some(4.5),
            review_count: 10,
        }
    }

    fn query(min_guests: u32) -> SearchQuery {
        SearchQuery {
            stay: Stay::new(date("2027-01-10"), date("2027-01-17")).unwrap(),
            min_guests,
            max_guests: None,
            min_bedrooms: None,
            max_price_per_night: None,
        }
    }

    #[test]
    fn without_detail_uses_search_data_and_capacity_lower_bound() {
        let listing = to_listing(&hit(), None, &query(20));
        assert_eq!(listing.max_guests, 16);
        assert_eq!(listing.price_per_night, 1000.0);
        assert_eq!(listing.url, "https://www.airbnb.com/rooms/123");
        assert!(listing.amenities.is_empty());
        assert!(listing.description.contains("lower bound"));
    }

    #[test]
    fn detail_overrides_search_data() {
        let detail = ListingDetail {
            name: Some("Real name".into()),
            location: Some(GeoPoint { lat: 3.0, lon: 4.0 }),
            person_capacity: Some(20),
            bedrooms: Some(9),
            beds: None,
            bathrooms: None,
            amenities: vec!["Pool".into()],
            description: "desc".into(),
            rating: Some(4.9),
            review_count: Some(99),
        };
        let listing = to_listing(&hit(), Some(&detail), &query(10));
        assert_eq!(listing.name, "Real name");
        assert_eq!(listing.max_guests, 20);
        assert_eq!((listing.bedrooms, listing.beds), (9, 14));
        assert!(listing.has_pool_amenity());
        assert_eq!(listing.review_count, 99);
    }

    #[test]
    fn nightly_price_is_not_divided() {
        let nightly = SearchHit {
            price: Some(Price {
                amount: 500.0,
                is_total: false,
            }),
            ..hit()
        };
        assert_eq!(to_listing(&nightly, None, &query(2)).price_per_night, 500.0);
    }

    #[test]
    fn block_detection_requires_missing_state() {
        assert!(looks_blocked("<html>Please complete the CAPTCHA</html>"));
        assert!(!looks_blocked(
            r#"<script id="data-deferred-state-0">captcha</script>"#
        ));
        assert!(!looks_blocked("<html>ok</html>"));
    }
}
