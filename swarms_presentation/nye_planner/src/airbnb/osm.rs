//! City name -> bounding box, via OpenStreetMap's Nominatim. Its usage policy
//! asks for <= 1 req/s and an identifying User-Agent, which `osm_client` provides.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::AirbnbError;
use crate::cache::DiskCache;
use crate::http::{PoliteClient, PoliteConfig};

pub const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/search";
const CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// One geocoding request per city, with room for the cities of a whole run.
const REQUEST_BUDGET: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub south: f64,
    pub north: f64,
    pub west: f64,
    pub east: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CityInfo {
    pub name: String,
    pub bbox: BoundingBox,
}

pub fn osm_client(
    cache_dir: &Path,
    min_interval: Duration,
    max_jitter: Duration,
) -> Result<PoliteClient, AirbnbError> {
    let contact = std::env::var("OSM_CONTACT").unwrap_or_else(|_| "personal trip research".into());
    let cache = DiskCache::new(cache_dir.join("osm"), CACHE_TTL)?;
    Ok(PoliteClient::new(PoliteConfig {
        min_interval,
        max_jitter,
        cache: Some(cache),
        request_budget: REQUEST_BUDGET,
        ..PoliteConfig::new("openstreetmap", &format!("nye_planner/0.1 ({contact})"))
    })?)
}

#[derive(Deserialize)]
struct NominatimPlace {
    /// [south, north, west, east]
    boundingbox: [String; 4],
    name: Option<String>,
}

pub fn parse_nominatim(body: &str, query: &str) -> Result<CityInfo, AirbnbError> {
    let places: Vec<NominatimPlace> = serde_json::from_str(body)?;
    let place = places
        .into_iter()
        .next()
        .ok_or_else(|| AirbnbError::UnknownCity(query.to_string()))?;
    let num = |s: &str| {
        s.parse::<f64>()
            .map_err(|e| AirbnbError::Parse(format!("nominatim number '{s}': {e}")))
    };
    let [south, north, west, east] = &place.boundingbox;
    Ok(CityInfo {
        name: place
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| query.to_string()),
        bbox: BoundingBox {
            south: num(south)?,
            north: num(north)?,
            west: num(west)?,
            east: num(east)?,
        },
    })
}

pub fn geocode_url(base: &str, city: &str) -> Result<String, AirbnbError> {
    let mut url = reqwest::Url::parse(base)
        .map_err(|e| AirbnbError::Parse(format!("geocoder URL '{base}': {e}")))?;
    url.query_pairs_mut()
        .append_pair("q", city)
        .append_pair("format", "jsonv2")
        .append_pair("limit", "1")
        .append_pair("accept-language", "en");
    Ok(url.to_string())
}

pub async fn geocode(
    client: &PoliteClient,
    base_url: &str,
    city: &str,
) -> Result<CityInfo, AirbnbError> {
    let url = geocode_url(base_url, city)?;
    parse_nominatim(&client.get(&url, None).await?, city)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nominatim_result() {
        let body = r#"[{"lat":"-27.5973","lon":"-48.5496","name":"Florianópolis",
            "boundingbox":["-27.84","-27.38","-48.62","-48.35"],
            "address":{"country":"Brazil"}}]"#;
        let city = parse_nominatim(body, "florianopolis").unwrap();
        assert_eq!(city.name, "Florianópolis");
        assert_eq!(city.bbox.south, -27.84);
        assert_eq!(city.bbox.north, -27.38);
        assert_eq!(city.bbox.west, -48.62);
        assert_eq!(city.bbox.east, -48.35);
    }

    #[test]
    fn missing_name_falls_back_to_the_query() {
        let body = r#"[{"boundingbox":["1","2","3","4"]}]"#;
        assert_eq!(
            parse_nominatim(body, "Somewhere").unwrap().name,
            "Somewhere"
        );
    }

    #[test]
    fn empty_nominatim_result_is_unknown_city() {
        assert!(matches!(
            parse_nominatim("[]", "Atlantis"),
            Err(AirbnbError::UnknownCity(_))
        ));
    }

    #[test]
    fn bad_numbers_and_json_are_parse_errors() {
        let body = r#"[{"boundingbox":["x","2","3","4"]}]"#;
        assert!(matches!(
            parse_nominatim(body, "q"),
            Err(AirbnbError::Parse(_))
        ));
        assert!(matches!(
            parse_nominatim("not json", "q"),
            Err(AirbnbError::Parse(_))
        ));
    }

    #[test]
    fn geocode_url_encodes_the_query() {
        let url = geocode_url(NOMINATIM_URL, "Bombinhas, Santa Catarina, Brazil").unwrap();
        assert!(url.starts_with("https://nominatim.openstreetmap.org/search?q=Bombinhas"));
        assert!(url.contains("limit=1"));
        assert!(!url.contains(' '));
    }
}
