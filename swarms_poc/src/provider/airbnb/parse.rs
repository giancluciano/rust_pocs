//! Extracts data from the JSON Airbnb embeds in its server-rendered pages
//! (`<script id="data-deferred-state-0">`). Keys are located by name rather
//! than by fixed path, so small layout changes don't break parsing.

use base64::Engine;
use serde_json::Value;

use crate::domain::GeoPoint;
use crate::provider::ProviderError;

const STATE_SCRIPT_ID: &str = "data-deferred-state-0";
const MAX_DESCRIPTION_CHARS: usize = 1500;

#[derive(Debug, Clone, PartialEq)]
pub struct Price {
    pub amount: f64,
    /// true when `amount` is for the whole stay, false when per night
    pub is_total: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub location: GeoPoint,
    pub bedrooms: Option<u32>,
    pub beds: Option<u32>,
    pub bathrooms: Option<f32>,
    pub price: Option<Price>,
    pub rating: Option<f32>,
    pub review_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListingDetail {
    pub name: Option<String>,
    pub location: Option<GeoPoint>,
    pub person_capacity: Option<u32>,
    pub bedrooms: Option<u32>,
    pub beds: Option<u32>,
    pub bathrooms: Option<f32>,
    pub amenities: Vec<String>,
    pub description: String,
    pub rating: Option<f32>,
    pub review_count: Option<u32>,
}

pub fn has_page_state(html: &str) -> bool {
    html.contains(STATE_SCRIPT_ID)
}

pub fn page_state(html: &str) -> Result<Value, ProviderError> {
    let tag = html
        .find(&format!("id=\"{STATE_SCRIPT_ID}\""))
        .ok_or_else(|| ProviderError::Parse("embedded page state not found".into()))?;
    let start = html[tag..]
        .find('>')
        .map(|i| tag + i + 1)
        .ok_or_else(|| ProviderError::Parse("malformed state script tag".into()))?;
    let end = html[start..]
        .find("</script>")
        .map(|i| start + i)
        .ok_or_else(|| ProviderError::Parse("unterminated state script".into()))?;
    Ok(serde_json::from_str(&html[start..end])?)
}

/// Depth-first search for the first object field named `key`.
fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find(v, key)),
        _ => None,
    }
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(value, |v, k| v.get(k))?.as_str()
}

fn f64_at(value: &Value, path: &[&str]) -> Option<f64> {
    path.iter().try_fold(value, |v, k| v.get(k))?.as_f64()
}

/// "DemandStayListing:12345" base64-encoded -> "12345"
pub fn decode_listing_id(encoded: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let id = text.rsplit(':').next()?;
    id.chars()
        .all(|c| c.is_ascii_digit())
        .then(|| id.to_string())
}

/// "R$7,672" -> 7672.0, "$1,234.56" -> 1234.56 (English locale formatting).
pub fn parse_money(text: &str) -> Option<f64> {
    let digits: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().ok()
}

/// Finds "<n> <word>" in a list like ["8 bedrooms", "14 beds", "11 baths"].
/// `word` is matched as a prefix, so "bed" matches "bed"/"beds" but the
/// caller must avoid ambiguity ("bedroom" is checked separately).
fn count_of(items: &[String], matches: impl Fn(&str) -> bool) -> Option<f32> {
    items.iter().find_map(|item| {
        let lower = item.to_lowercase();
        let (number, word) = lower.trim().split_once(' ')?;
        matches(word.trim_end_matches('+')).then(|| number.trim_end_matches('+').parse().ok())?
    })
}

fn room_counts(items: &[String]) -> (Option<u32>, Option<u32>, Option<f32>) {
    let bedrooms = count_of(items, |w| w.starts_with("bedroom")).map(|n| n as u32);
    let beds = count_of(items, |w| w == "bed" || w == "beds").map(|n| n as u32);
    let baths = count_of(items, |w| w.contains("bath"));
    (bedrooms, beds, baths)
}

/// "4.92 (64)" -> (Some(4.92), 64); "New" -> (None, 0)
pub fn parse_rating(text: &str) -> (Option<f32>, u32) {
    let mut parts = text.split_whitespace();
    let rating = parts.next().and_then(|r| r.replace(',', ".").parse().ok());
    let count = parts
        .next()
        .map(|c| c.trim_matches(|ch| ch == '(' || ch == ')'))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (rating, count)
}

fn parse_price(result: &Value) -> Option<Price> {
    let line = find(result, "structuredDisplayPrice")?.get("primaryLine")?;
    let amount = ["discountedPrice", "price"]
        .iter()
        .find_map(|k| line.get(*k)?.as_str())
        .and_then(parse_money)?;
    let qualifier = line.get("qualifier").and_then(Value::as_str).unwrap_or("");
    Some(Price {
        amount,
        is_total: !qualifier.contains("night"),
    })
}

fn parse_hit(result: &Value) -> Option<SearchHit> {
    let listing = result.get("demandStayListing")?;
    let id = decode_listing_id(listing.get("id")?.as_str()?)?;
    let coordinate = find(listing, "coordinate")?;
    let location = GeoPoint {
        lat: coordinate.get("latitude")?.as_f64()?,
        lon: coordinate.get("longitude")?.as_f64()?,
    };
    let name = str_at(
        result,
        &["nameLocalized", "localizedStringWithTranslationPreference"],
    )
    .or_else(|| result.get("subtitle")?.as_str())
    .or_else(|| result.get("title")?.as_str())
    .unwrap_or("")
    .to_string();
    let lines: Vec<String> = result
        .get("structuredContent")
        .and_then(|c| c.get("primaryLine"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.get("body")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let (bedrooms, beds, bathrooms) = room_counts(&lines);
    let (rating, review_count) = result
        .get("avgRatingLocalized")
        .and_then(Value::as_str)
        .map_or((None, 0), parse_rating);

    Some(SearchHit {
        id,
        name,
        location,
        bedrooms,
        beds,
        bathrooms,
        price: parse_price(result),
        rating,
        review_count,
    })
}

pub fn parse_search(html: &str, page_index: usize) -> Result<SearchPage, ProviderError> {
    let state = page_state(html)?;
    let results = find(&state, "staysSearch")
        .and_then(|s| s.get("results"))
        .ok_or_else(|| ProviderError::Parse("search results not found".into()))?;
    let hits = results
        .get("searchResults")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_hit).collect())
        .unwrap_or_default();
    let pagination = results.get("paginationInfo");
    let next_cursor = pagination
        .and_then(|p| p.get("nextPageCursor"))
        .and_then(Value::as_str)
        .or_else(|| {
            pagination?
                .get("pageCursors")?
                .as_array()?
                .get(page_index + 1)?
                .as_str()
        })
        .map(str::to_string);
    Ok(SearchPage { hits, next_cursor })
}

pub fn strip_html(html: &str) -> String {
    let with_breaks = html
        .replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n");
    let mut text = String::with_capacity(with_breaks.len());
    let mut in_tag = false;
    for c in with_breaks.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn parse_amenities(presentation: &Value) -> Vec<String> {
    presentation
        .get("amenities")
        .and_then(|a| a.get("seeAllAmenitiesGroups"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|g| g.get("title").and_then(Value::as_str) != Some("Not included"))
        .filter_map(|g| g.get("amenities")?.as_array())
        .flatten()
        .filter(|a| a.get("available").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|a| a.get("title")?.as_str().map(str::to_string))
        .collect()
}

pub fn parse_listing(html: &str) -> Result<ListingDetail, ProviderError> {
    let state = page_state(html)?;
    let presentation = find(&state, "pdpPresentation")
        .ok_or_else(|| ProviderError::Parse("listing details not found".into()))?;

    let overview: Vec<String> = presentation
        .get("overview")
        .and_then(|o| o.get("items"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let (bedrooms, beds, bathrooms) = room_counts(&overview);

    let description = str_at(
        presentation,
        &[
            "descriptions",
            "longDescriptionHtml",
            "localizedStringWithTranslationPreference",
        ],
    )
    .map(strip_html)
    .unwrap_or_default()
    .chars()
    .take(MAX_DESCRIPTION_CHARS)
    .collect();

    let location = presentation.get("location").and_then(|l| {
        Some(GeoPoint {
            lat: f64_at(l, &["latitude"])?,
            lon: f64_at(l, &["longitude"])?,
        })
    });

    let rating_stats = find(presentation, "overallRatingStats");
    let rating = rating_stats
        .and_then(|s| s.get("ratingAverage"))
        .and_then(Value::as_f64)
        .map(|r| r as f32);
    let review_count = rating_stats
        .and_then(|s| s.get("ratingCount"))
        .and_then(|c| c.as_u64().or_else(|| c.as_str()?.parse().ok()))
        .map(|c| c as u32);

    Ok(ListingDetail {
        name: str_at(
            presentation,
            &[
                "title",
                "content",
                "localizedStringWithTranslationPreference",
            ],
        )
        .map(str::to_string),
        location,
        person_capacity: presentation
            .get("personCapacity")
            .and_then(Value::as_u64)
            .or_else(|| find(&state, "personCapacity")?.as_u64())
            .map(|c| c as u32),
        bedrooms,
        beds,
        bathrooms,
        amenities: parse_amenities(presentation),
        description,
        rating,
        review_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(state: &str) -> String {
        format!(
            r#"<html><script id="{STATE_SCRIPT_ID}" data-deferred-state-0="true" type="application/json">{state}</script></html>"#
        )
    }

    // base64("DemandStayListing:1750878132099498117")
    const ENCODED_ID: &str = "RGVtYW5kU3RheUxpc3Rpbmc6MTc1MDg3ODEzMjA5OTQ5ODExNw==";

    fn search_state() -> String {
        format!(
            r#"{{"niobeClientData":[["StaysSearch",{{"data":{{"presentation":{{"staysSearch":{{"results":{{
            "searchResults":[{{
                "avgRatingLocalized":"4.92 (64)",
                "title":"Home in Florianópolis",
                "nameLocalized":{{"localizedStringWithTranslationPreference":"Beach house with pool"}},
                "structuredDisplayPrice":{{"primaryLine":{{"discountedPrice":"R$7,672","originalPrice":"R$14,610","qualifier":"total"}}}},
                "structuredContent":{{"primaryLine":[{{"body":"8 bedrooms"}},{{"body":"14 beds"}},{{"body":"11 baths"}}]}},
                "demandStayListing":{{"id":"{ENCODED_ID}","location":{{"coordinate":{{"latitude":-27.43,"longitude":-48.39}}}}}}
            }},{{"title":"broken result without listing"}}],
            "paginationInfo":{{"pageCursors":["c0","c1","c2"]}}
            }}}}}}}}}}]]}}"#
        )
    }

    #[test]
    fn decodes_listing_ids() {
        assert_eq!(
            decode_listing_id(ENCODED_ID).as_deref(),
            Some("1750878132099498117")
        );
        assert_eq!(decode_listing_id("not base64!"), None);
    }

    #[test]
    fn parses_money_and_ratings() {
        assert_eq!(parse_money("R$7,672"), Some(7672.0));
        assert_eq!(parse_money("$1,234.56"), Some(1234.56));
        assert_eq!(parse_money("free"), None);
        assert_eq!(parse_rating("4.92 (64)"), (Some(4.92), 64));
        assert_eq!(parse_rating("New"), (None, 0));
    }

    #[test]
    fn room_counts_handle_plus_and_singular() {
        let items = vec![
            "16+ guests".into(),
            "1 bedroom".into(),
            "1 bed".into(),
            "1.5 baths".into(),
        ];
        assert_eq!(room_counts(&items), (Some(1), Some(1), Some(1.5)));
    }

    #[test]
    fn parses_search_page() {
        let parsed = parse_search(&page(&search_state()), 0).unwrap();
        assert_eq!(parsed.hits.len(), 1, "broken results are skipped");
        let hit = &parsed.hits[0];
        assert_eq!(hit.id, "1750878132099498117");
        assert_eq!(hit.name, "Beach house with pool");
        assert_eq!(
            hit.location,
            GeoPoint {
                lat: -27.43,
                lon: -48.39
            }
        );
        assert_eq!(
            (hit.bedrooms, hit.beds, hit.bathrooms),
            (Some(8), Some(14), Some(11.0))
        );
        assert_eq!(
            hit.price,
            Some(Price {
                amount: 7672.0,
                is_total: true
            })
        );
        assert_eq!((hit.rating, hit.review_count), (Some(4.92), 64));
        assert_eq!(parsed.next_cursor.as_deref(), Some("c1"));

        let last = parse_search(&page(&search_state()), 2).unwrap();
        assert_eq!(last.next_cursor, None);
    }

    #[test]
    fn missing_state_is_a_parse_error() {
        assert!(matches!(
            parse_search("<html></html>", 0),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn parses_listing_page() {
        let state = r#"{"niobeClientData":[["StaysPdpSections",{"data":{"node":{"personCapacity":16,"pdpPresentation":{
            "personCapacity":16,
            "title":{"content":{"localizedStringWithTranslationPreference":"Luxurious ranch"}},
            "descriptions":{"longDescriptionHtml":{"localizedStringWithTranslationPreference":"Big pool &amp; court.<br />Sleeps 16 in <b>beds</b>."}},
            "overview":{"items":["16+ guests","8 bedrooms","14 beds","11 baths"]},
            "location":{"latitude":-19.72,"longitude":-44.13},
            "quality":{"listingRatingStats":{"overallRatingStats":{"ratingAverage":4.8,"ratingCount":"12"}}},
            "amenities":{"seeAllAmenitiesGroups":[
                {"title":"Parking and facilities","amenities":[{"title":"Pool","available":true},{"title":"Gym","available":false}]},
                {"title":"Not included","amenities":[{"title":"Smoke alarm","available":true}]}
            ]}
        }}}}]]}"#;
        let detail = parse_listing(&page(state)).unwrap();
        assert_eq!(detail.name.as_deref(), Some("Luxurious ranch"));
        assert_eq!(detail.person_capacity, Some(16));
        assert_eq!(
            (detail.bedrooms, detail.beds, detail.bathrooms),
            (Some(8), Some(14), Some(11.0))
        );
        assert_eq!(detail.amenities, vec!["Pool".to_string()]);
        assert_eq!(detail.description, "Big pool & court.\nSleeps 16 in beds.");
        assert_eq!(
            detail.location,
            Some(GeoPoint {
                lat: -19.72,
                lon: -44.13
            })
        );
        assert_eq!((detail.rating, detail.review_count), (Some(4.8), Some(12)));
    }
}
