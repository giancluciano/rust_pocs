//! Scrapes Airbnb's public search and listing pages. Ported from swarms_poc.
//!
//! One `AirbnbSearch` is shared by every city, so all Airbnb requests go
//! through a single rate limiter and cookie session (and all geocoding through
//! a single OpenStreetMap one). Volume is kept low on purpose: requests go
//! through a `PoliteClient` (>= 1 req/s with jitter, one at a time, backoff,
//! circuit breaker, request budget, disk cache), search is capped at
//! `max_pages` per city and listing pages at `max_listing_details` per city.
//! Listing pages are fetched without dates so their cache is reused across
//! searches and runs.

mod osm;
mod parse;

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderValue};

use crate::cache::DiskCache;
use crate::domain::{Listing, Stay};
use crate::http::{HttpError, PoliteClient, PoliteConfig};
use osm::{BoundingBox, CityInfo};
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
/// Politeness floor: never faster than one request per second.
const MIN_INTERVAL_FLOOR: Duration = Duration::from_secs(1);
const MAX_JITTER: Duration = Duration::from_millis(400);
/// The request budget covers this many cities per run.
const MAX_CITIES: u32 = 12;

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

impl From<serde_json::Error> for AirbnbError {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse(format!("invalid JSON: {e}"))
    }
}

/// Where the two sites live and how fast to talk to OSM; swapped in tests.
struct Site {
    airbnb_url: String,
    nominatim_url: String,
    osm_interval: Duration,
    max_jitter: Duration,
}

impl Site {
    fn live() -> Self {
        Self {
            airbnb_url: BASE_URL.to_string(),
            nominatim_url: osm::NOMINATIM_URL.to_string(),
            osm_interval: MIN_INTERVAL_FLOOR,
            max_jitter: MAX_JITTER,
        }
    }
}

pub struct AirbnbSearch {
    /// Single client => single rate limiter and cookie session for the site.
    http: PoliteClient,
    osm_http: PoliteClient,
    config: AirbnbConfig,
    site: Site,
}

fn looks_blocked(body: &str) -> bool {
    !parse::has_page_state(body) && body.to_lowercase().contains("captcha")
}

fn airbnb_client(
    config: &AirbnbConfig,
    site: &Site,
    cache: DiskCache,
) -> Result<PoliteClient, AirbnbError> {
    let per_city = config.max_pages + config.max_listing_details;
    let mut polite = PoliteConfig {
        min_interval: config.min_interval,
        max_jitter: site.max_jitter,
        // pages and listing details for every city, plus the warm-up request
        request_budget: per_city * MAX_CITIES + 1,
        block_detector: looks_blocked,
        cache: Some(cache),
        warmup_url: Some(format!("{}/", site.airbnb_url)),
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

fn search_url(
    base: &str,
    info: &CityInfo,
    stay: &Stay,
    guests: u32,
    cursor: Option<&str>,
) -> Result<String, AirbnbError> {
    let bad_base = || AirbnbError::Parse(format!("Airbnb URL '{base}' is not a valid base"));
    let mut url = reqwest::Url::parse(base).map_err(|_| bad_base())?;
    url.path_segments_mut()
        .map_err(|_| bad_base())?
        .extend(["s", info.name.as_str(), "homes"]);
    let BoundingBox {
        south,
        north,
        west,
        east,
    } = info.bbox;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("checkin", &stay.checkin().to_string())
            .append_pair("checkout", &stay.checkout().to_string())
            .append_pair("adults", &guests.min(MAX_ADULTS_FILTER).to_string())
            .append_pair("room_types[]", "Entire home/apt")
            .append_pair("ne_lat", &north.to_string())
            .append_pair("ne_lng", &east.to_string())
            .append_pair("sw_lat", &south.to_string())
            .append_pair("sw_lng", &west.to_string())
            .append_pair("search_by_map", "true")
            .append_pair("search_type", "user_map_move")
            .append_pair("currency", CURRENCY)
            .append_pair("locale", "en");
        if let Some(cursor) = cursor {
            q.append_pair("cursor", cursor);
        }
    }
    Ok(url.to_string())
}

/// Price for one night; hits without a price are unusable and give `None`.
fn nightly_price(hit: &SearchHit, stay: &Stay) -> Option<f64> {
    hit.price.as_ref().map(|p| {
        if p.is_total {
            p.amount / f64::from(stay.nights().max(1))
        } else {
            p.amount
        }
    })
}

/// Drops hits without a price and repeated ids (a listing can show up on two
/// pages), cheapest first.
fn priced_unique(hits: Vec<SearchHit>, stay: &Stay) -> Vec<(SearchHit, f64)> {
    let mut seen = HashSet::new();
    let mut priced: Vec<(SearchHit, f64)> = hits
        .into_iter()
        .filter(|hit| seen.insert(hit.id.clone()))
        .filter_map(|hit| {
            let price = nightly_price(&hit, stay);
            if price.is_none() {
                tracing::debug!(id = %hit.id, "listing has no price; skipped");
            }
            price.map(|p| (hit, p))
        })
        .collect();
    priced.sort_by(|a, b| a.1.total_cmp(&b.1));
    priced
}

/// Capacity is only known from a listing page. Without one, the search's
/// guest filter already guarantees it, so the listing stays.
fn fits_group(detail: Option<&ListingDetail>, guests: u32) -> bool {
    detail
        .and_then(|d| d.person_capacity)
        .is_none_or(|capacity| capacity >= guests)
}

/// Merges search-page data with listing-page data (listing page wins).
/// Without a listing page, capacity falls back to the search's guest filter.
fn to_listing(
    base: &str,
    hit: &SearchHit,
    price_per_night: f64,
    detail: Option<&ListingDetail>,
    guests: u32,
) -> Listing {
    let d = detail;
    Listing {
        id: hit.id.clone(),
        name: d
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| hit.name.clone()),
        url: format!("{base}/rooms/{}", hit.id),
        location: d.and_then(|d| d.location).unwrap_or(hit.location),
        max_guests: d
            .and_then(|d| d.person_capacity)
            .unwrap_or_else(|| guests.min(MAX_ADULTS_FILTER)),
        bedrooms: d.and_then(|d| d.bedrooms).or(hit.bedrooms).unwrap_or(0),
        beds: d.and_then(|d| d.beds).or(hit.beds).unwrap_or(0),
        bathrooms: d.and_then(|d| d.bathrooms).or(hit.bathrooms).unwrap_or(0.0),
        price_per_night,
        currency: CURRENCY.to_string(),
        rating: d.and_then(|d| d.rating).or(hit.rating),
        review_count: d.and_then(|d| d.review_count).unwrap_or(hit.review_count),
        amenities: d.map(|d| d.amenities.clone()).unwrap_or_default(),
        description: d
            .map(|d| d.description.clone())
            .unwrap_or_else(|| "(listing page not fetched: capacity is a lower bound)".into()),
    }
}

impl AirbnbSearch {
    /// Prepares the Airbnb session and the geocoder. No request is made yet.
    pub fn new(config: AirbnbConfig, cache_dir: &Path) -> Result<Self, AirbnbError> {
        let config = AirbnbConfig {
            min_interval: config.min_interval.max(MIN_INTERVAL_FLOOR),
            ..config
        };
        Self::with_site(config, cache_dir, Site::live())
    }

    fn with_site(config: AirbnbConfig, cache_dir: &Path, site: Site) -> Result<Self, AirbnbError> {
        let cache = DiskCache::new(cache_dir.join("airbnb"), CACHE_TTL)?;
        Ok(Self {
            http: airbnb_client(&config, &site, cache)?,
            osm_http: osm::osm_client(cache_dir, site.osm_interval, site.max_jitter)?,
            config,
            site,
        })
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
        let info = osm::geocode(&self.osm_http, &self.site.nominatim_url, city_query).await?;
        tracing::info!(city = %info.name, "city located on OpenStreetMap");

        let hits = self.search_hits(&info, stay, guests).await?;
        let priced = priced_unique(hits, stay);
        let first_page = search_url(&self.site.airbnb_url, &info, stay, guests, None)?;
        let details = self.fetch_details(&priced, &first_page).await;

        let listings: Vec<Listing> = priced
            .iter()
            .enumerate()
            .filter_map(|(i, (hit, price))| {
                let detail = details.get(i).and_then(Option::as_ref);
                fits_group(detail, guests)
                    .then(|| to_listing(&self.site.airbnb_url, hit, *price, detail, guests))
            })
            .collect();
        tracing::info!(
            city = %info.name,
            found = listings.len(),
            requests_so_far = self.http.requests_made(),
            "airbnb search done"
        );
        Ok(listings)
    }

    /// Up to `max_pages` result pages. A failure on the first page is an
    /// error; later on, the pages already read are kept.
    async fn search_hits(
        &self,
        info: &CityInfo,
        stay: &Stay,
        guests: u32,
    ) -> Result<Vec<SearchHit>, AirbnbError> {
        let mut hits = Vec::new();
        let mut cursor: Option<String> = None;
        for page_index in 0..self.config.max_pages as usize {
            let url = search_url(&self.site.airbnb_url, info, stay, guests, cursor.as_deref())?;
            let page = match self.fetch_page(&url, page_index).await {
                Ok(page) => page,
                Err(e) if page_index > 0 => {
                    tracing::warn!(error = %e, "stopping at the pages already read");
                    break;
                }
                Err(e) => return Err(e),
            };
            tracing::info!(
                page = page_index + 1,
                hits = page.hits.len(),
                "airbnb search page"
            );
            let has_hits = !page.hits.is_empty();
            hits.extend(page.hits);
            match page.next_cursor {
                Some(next) if has_hits => cursor = Some(next),
                _ => break,
            }
        }
        Ok(hits)
    }

    async fn fetch_page(
        &self,
        url: &str,
        page_index: usize,
    ) -> Result<parse::SearchPage, AirbnbError> {
        parse::parse_search(&self.http.get(url, None).await?, page_index)
    }

    /// Listing pages of the cheapest hits, aligned with the first entries of
    /// `priced`. A listing whose page is unavailable keeps its search data
    /// (`None`). Once the site blocks us or the request budget is used up, no
    /// more pages are fetched, but the search results are still returned.
    async fn fetch_details(
        &self,
        priced: &[(SearchHit, f64)],
        referer: &str,
    ) -> Vec<Option<ListingDetail>> {
        let mut details = Vec::new();
        for (hit, _) in priced.iter().take(self.config.max_listing_details as usize) {
            let url = format!("{}/rooms/{}?locale=en", self.site.airbnb_url, hit.id);
            match self.http.get(&url, Some(referer)).await {
                Ok(html) => details.push(
                    parse::parse_listing(&html)
                        .inspect_err(|e| {
                            tracing::warn!(id = %hit.id, error = %e, "could not parse listing page")
                        })
                        .ok(),
                ),
                Err(e @ (HttpError::Blocked { .. } | HttpError::BudgetExhausted { .. })) => {
                    tracing::warn!(error = %e, "no more listing pages; using search data only");
                    break;
                }
                Err(e) => {
                    tracing::warn!(id = %hit.id, error = %e, "listing page unavailable");
                    details.push(None);
                }
            }
        }
        details
    }
}

#[cfg(test)]
mod tests;
