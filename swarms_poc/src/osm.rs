//! City center / bounding box (Nominatim) and beaches (Overpass), both from
//! OpenStreetMap. Their usage policies ask for <= 1 req/s and an identifying
//! User-Agent, which `osm_client` provides.

use std::time::Duration;

use serde::Deserialize;

use crate::cache::DiskCache;
use crate::domain::{Beach, GeoPoint};
use crate::geo::haversine_km;
use crate::http::{PoliteClient, PoliteConfig};
use crate::provider::ProviderError;

const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/search";
const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";
pub const UNNAMED_BEACH: &str = "Unnamed beach";
const MIN_BEACH_RADIUS_M: f64 = 5_000.0;
const MAX_BEACH_RADIUS_M: f64 = 30_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub south: f64,
    pub north: f64,
    pub west: f64,
    pub east: f64,
}

impl BoundingBox {
    /// Half the diagonal: a radius that covers the whole box from its center.
    pub fn radius_m(&self) -> f64 {
        let sw = GeoPoint {
            lat: self.south,
            lon: self.west,
        };
        let ne = GeoPoint {
            lat: self.north,
            lon: self.east,
        };
        haversine_km(sw, ne) * 1000.0 / 2.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CityInfo {
    pub name: String,
    pub country: String,
    pub center: GeoPoint,
    pub bbox: BoundingBox,
}

pub fn osm_client(cache_dir: &std::path::Path) -> Result<PoliteClient, ProviderError> {
    let contact = std::env::var("OSM_CONTACT").unwrap_or_else(|_| "personal trip research".into());
    let cache = DiskCache::new(cache_dir.join("osm"), Duration::from_secs(30 * 24 * 3600))?;
    Ok(PoliteClient::new(PoliteConfig {
        cache: Some(cache),
        request_budget: 20,
        ..PoliteConfig::new("openstreetmap", &format!("swarms_poc/0.1 ({contact})"))
    })?)
}

#[derive(Deserialize)]
struct NominatimPlace {
    lat: String,
    lon: String,
    /// [south, north, west, east]
    boundingbox: [String; 4],
    name: Option<String>,
    #[serde(default)]
    address: NominatimAddress,
}

#[derive(Deserialize, Default)]
struct NominatimAddress {
    country: Option<String>,
}

pub fn parse_nominatim(body: &str, query: &str) -> Result<CityInfo, ProviderError> {
    let places: Vec<NominatimPlace> = serde_json::from_str(body)?;
    let place = places
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::UnknownCity(query.to_string()))?;
    let num = |s: &str| {
        s.parse::<f64>()
            .map_err(|e| ProviderError::Parse(format!("nominatim number '{s}': {e}")))
    };
    let [south, north, west, east] = &place.boundingbox;
    Ok(CityInfo {
        name: place.name.unwrap_or_else(|| query.to_string()),
        country: place.address.country.unwrap_or_default(),
        center: GeoPoint {
            lat: num(&place.lat)?,
            lon: num(&place.lon)?,
        },
        bbox: BoundingBox {
            south: num(south)?,
            north: num(north)?,
            west: num(west)?,
            east: num(east)?,
        },
    })
}

pub async fn geocode(client: &PoliteClient, city: &str) -> Result<CityInfo, ProviderError> {
    let mut url = reqwest::Url::parse(NOMINATIM_URL).expect("static URL");
    url.query_pairs_mut()
        .append_pair("q", city)
        .append_pair("format", "jsonv2")
        .append_pair("limit", "1")
        .append_pair("addressdetails", "1")
        .append_pair("accept-language", "en");
    parse_nominatim(&client.get(url.as_str(), None).await?, city)
}

#[derive(Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

#[derive(Deserialize)]
struct OverpassElement {
    lat: Option<f64>,
    lon: Option<f64>,
    center: Option<GeoPoint>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

pub fn parse_overpass_beaches(body: &str) -> Result<Vec<Beach>, ProviderError> {
    let response: OverpassResponse = serde_json::from_str(body)?;
    Ok(response
        .elements
        .into_iter()
        .filter_map(|e| {
            let location = match (e.lat, e.lon, e.center) {
                (Some(lat), Some(lon), _) => GeoPoint { lat, lon },
                (_, _, Some(center)) => center,
                _ => return None,
            };
            let name = e
                .tags
                .get("name")
                .cloned()
                .unwrap_or_else(|| UNNAMED_BEACH.to_string());
            Some(Beach { name, location })
        })
        .collect())
}

pub async fn beaches(client: &PoliteClient, city: &CityInfo) -> Result<Vec<Beach>, ProviderError> {
    let radius = city
        .bbox
        .radius_m()
        .clamp(MIN_BEACH_RADIUS_M, MAX_BEACH_RADIUS_M);
    let query = format!(
        "[out:json][timeout:25];nwr[\"natural\"=\"beach\"](around:{radius:.0},{},{});out center tags;",
        city.center.lat, city.center.lon
    );
    parse_overpass_beaches(&client.post_form(OVERPASS_URL, &[("data", &query)]).await?)
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
        assert_eq!(city.country, "Brazil");
        assert_eq!(
            city.center,
            GeoPoint {
                lat: -27.5973,
                lon: -48.5496
            }
        );
        assert_eq!(city.bbox.south, -27.84);
        assert_eq!(city.bbox.east, -48.35);
    }

    #[test]
    fn empty_nominatim_result_is_unknown_city() {
        assert!(matches!(
            parse_nominatim("[]", "Atlantis"),
            Err(ProviderError::UnknownCity(_))
        ));
    }

    #[test]
    fn parses_overpass_nodes_and_ways() {
        let body = r#"{"elements":[
            {"type":"node","lat":-27.43,"lon":-48.39,"tags":{"name":"Ingleses"}},
            {"type":"way","center":{"lat":-27.62,"lon":-48.44},"tags":{"natural":"beach"}},
            {"type":"relation","tags":{"name":"no geometry"}}
        ]}"#;
        let beaches = parse_overpass_beaches(body).unwrap();
        assert_eq!(beaches.len(), 2);
        assert_eq!(beaches[0].name, "Ingleses");
        assert_eq!(beaches[1].name, "Unnamed beach");
        assert_eq!(
            beaches[1].location,
            GeoPoint {
                lat: -27.62,
                lon: -48.44
            }
        );
    }

    #[test]
    fn bbox_radius_is_half_diagonal() {
        let bbox = BoundingBox {
            south: 0.0,
            north: 0.1,
            west: 0.0,
            east: 0.0,
        };
        assert!((bbox.radius_m() - 5559.5).abs() < 1.0);
    }
}
