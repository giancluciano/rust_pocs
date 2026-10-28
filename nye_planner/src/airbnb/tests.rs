use base64::Engine;
use chrono::NaiveDate;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::parse::Price;
use super::*;
use crate::domain::GeoPoint;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Five nights over New Year's Eve.
fn stay() -> Stay {
    Stay::new(date("2026-12-28"), date("2027-01-02")).unwrap()
}

fn hit(id: &str, price: Option<Price>) -> SearchHit {
    SearchHit {
        id: id.into(),
        name: "Search name".into(),
        location: GeoPoint { lat: 1.0, lon: 2.0 },
        bedrooms: Some(8),
        beds: Some(14),
        bathrooms: Some(5.0),
        price,
        rating: Some(4.5),
        review_count: 10,
    }
}

fn total(amount: f64) -> Option<Price> {
    Some(Price {
        amount,
        is_total: true,
    })
}

fn detail(capacity: Option<u32>) -> ListingDetail {
    ListingDetail {
        name: Some("Real name".into()),
        location: Some(GeoPoint { lat: 3.0, lon: 4.0 }),
        person_capacity: capacity,
        bedrooms: Some(9),
        beds: None,
        bathrooms: None,
        amenities: vec!["Pool".into()],
        description: "desc".into(),
        rating: Some(4.9),
        review_count: Some(99),
    }
}

#[test]
fn without_detail_uses_search_data_and_capacity_lower_bound() {
    let listing = to_listing(BASE_URL, &hit("123", total(7000.0)), 1400.0, None, 20);
    assert_eq!(listing.max_guests, 16);
    assert_eq!(listing.price_per_night, 1400.0);
    assert_eq!(listing.currency, "BRL");
    assert_eq!(listing.url, "https://www.airbnb.com/rooms/123");
    assert!(listing.amenities.is_empty());
    assert!(listing.description.contains("lower bound"));
}

#[test]
fn detail_overrides_search_data() {
    let d = detail(Some(20));
    let listing = to_listing(BASE_URL, &hit("123", total(7000.0)), 1400.0, Some(&d), 10);
    assert_eq!(listing.name, "Real name");
    assert_eq!(listing.max_guests, 20);
    assert_eq!((listing.bedrooms, listing.beds), (9, 14));
    assert!(listing.has_pool());
    assert_eq!(listing.review_count, 99);
    assert_eq!(listing.location, GeoPoint { lat: 3.0, lon: 4.0 });
}

#[test]
fn total_price_is_divided_by_nights_and_nightly_is_not() {
    assert_eq!(
        nightly_price(&hit("a", total(7000.0)), &stay()),
        Some(1400.0)
    );
    let nightly = Some(Price {
        amount: 500.0,
        is_total: false,
    });
    assert_eq!(nightly_price(&hit("a", nightly), &stay()), Some(500.0));
    assert_eq!(nightly_price(&hit("a", None), &stay()), None);
}

#[test]
fn hits_are_deduped_priced_and_sorted_cheapest_first() {
    let hits = vec![
        hit("dear", total(9000.0)),
        hit("cheap", total(2000.0)),
        hit("dear", total(1.0)),
        hit("no-price", None),
        hit("mid", total(5000.0)),
    ];
    let ids: Vec<_> = priced_unique(hits, &stay())
        .into_iter()
        .map(|(h, p)| (h.id, p))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("cheap".to_string(), 400.0),
            ("mid".to_string(), 1000.0),
            ("dear".to_string(), 1800.0),
        ]
    );
}

#[test]
fn only_known_capacity_below_the_group_is_rejected() {
    assert!(
        fits_group(None, 20),
        "no detail page: capacity unknown, keep"
    );
    assert!(fits_group(Some(&detail(None)), 20));
    assert!(fits_group(Some(&detail(Some(20))), 20));
    assert!(!fits_group(Some(&detail(Some(8))), 10));
}

#[test]
fn block_detection_requires_missing_state() {
    assert!(looks_blocked("<html>Please complete the CAPTCHA</html>"));
    assert!(!looks_blocked(
        r#"<script id="data-deferred-state-0">captcha</script>"#
    ));
    assert!(!looks_blocked("<html>ok</html>"));
}

#[test]
fn search_url_carries_dates_guests_and_bounding_box() {
    let info = CityInfo {
        name: "Bombinhas".into(),
        bbox: BoundingBox {
            south: -27.2,
            north: -27.1,
            west: -48.6,
            east: -48.4,
        },
    };
    let url = search_url(BASE_URL, &info, &stay(), 24, Some("c1")).unwrap();
    assert!(url.starts_with("https://www.airbnb.com/s/Bombinhas/homes?"));
    for expected in [
        "checkin=2026-12-28",
        "checkout=2027-01-02",
        "adults=16",
        "ne_lat=-27.1",
        "sw_lng=-48.6",
        "currency=BRL",
        "cursor=c1",
    ] {
        assert!(url.contains(expected), "{url} should contain {expected}");
    }
    assert!(!url.contains("cursor") || url.contains("cursor=c1"));
    let first = search_url(BASE_URL, &info, &stay(), 4, None).unwrap();
    assert!(first.contains("adults=4") && !first.contains("cursor"));
}

#[test]
fn invalid_base_url_is_a_parse_error() {
    let info = CityInfo {
        name: "X".into(),
        bbox: BoundingBox {
            south: 0.0,
            north: 0.0,
            west: 0.0,
            east: 0.0,
        },
    };
    assert!(matches!(
        search_url("not a url", &info, &stay(), 2, None),
        Err(AirbnbError::Parse(_))
    ));
}

// ---- HTTP-level tests against a local mock of Airbnb and Nominatim ----

fn encoded_id(id: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("DemandStayListing:{id}"))
}

fn html_page(state: Value) -> String {
    format!(
        r#"<html><script id="data-deferred-state-0" type="application/json">{state}</script></html>"#
    )
}

fn search_result(id: &str, total_price: u32) -> Value {
    json!({
        "title": "Home in Bombinhas",
        "nameLocalized": {"localizedStringWithTranslationPreference": format!("House {id}")},
        "avgRatingLocalized": "4.80 (10)",
        "structuredDisplayPrice": {"primaryLine": {"price": format!("R${total_price}"), "qualifier": "total"}},
        "structuredContent": {"primaryLine": [{"body": "3 bedrooms"}, {"body": "5 beds"}, {"body": "2 baths"}]},
        "demandStayListing": {
            "id": encoded_id(id),
            "location": {"coordinate": {"latitude": -27.14, "longitude": -48.5}}
        }
    })
}

fn search_page(results: &[(&str, u32)], cursors: &[&str]) -> String {
    let results: Vec<Value> = results
        .iter()
        .map(|(id, p)| search_result(id, *p))
        .collect();
    html_page(
        json!({"niobeClientData": [["StaysSearch", {"data": {"presentation": {"staysSearch": {"results": {
            "searchResults": results,
            "paginationInfo": {"pageCursors": cursors}
        }}}}}]]}),
    )
}

fn listing_page(capacity: u32) -> String {
    html_page(
        json!({"niobeClientData": [["StaysPdpSections", {"data": {"node": {"pdpPresentation": {
            "personCapacity": capacity,
            "title": {"content": {"localizedStringWithTranslationPreference": format!("Detail for {capacity}")}},
            "descriptions": {"longDescriptionHtml": {"localizedStringWithTranslationPreference": "Nice &amp; big"}},
            "overview": {"items": ["3 bedrooms", "5 beds", "2 baths"]},
            "amenities": {"seeAllAmenitiesGroups": [
                {"title": "Facilities", "amenities": [{"title": "Pool", "available": true}]}
            ]}
        }}}}]]}),
    )
}

fn nominatim_body() -> &'static str {
    r#"[{"name":"Bombinhas","boundingbox":["-27.2","-27.1","-48.6","-48.4"]}]"#
}

fn test_search(server: &MockServer, dir: &Path, max_pages: u32, max_details: u32) -> AirbnbSearch {
    let site = Site {
        airbnb_url: server.uri(),
        nominatim_url: format!("{}/nominatim/search", server.uri()),
        osm_interval: Duration::ZERO,
        max_jitter: Duration::ZERO,
    };
    let config = AirbnbConfig {
        max_pages,
        max_listing_details: max_details,
        min_interval: Duration::ZERO,
    };
    AirbnbSearch::with_site(config, dir, site).unwrap()
}

async fn mount_basics(server: &MockServer) {
    Mock::given(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>home</html>"))
        .mount(server)
        .await;
    Mock::given(path("/nominatim/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(nominatim_body()))
        .mount(server)
        .await;
}

async fn mount_listing(server: &MockServer, id: &str, capacity: u32) {
    Mock::given(path(format!("/rooms/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(listing_page(capacity)))
        .mount(server)
        .await;
}

#[tokio::test]
async fn search_city_returns_cheapest_first_and_drops_small_homes() {
    let server = MockServer::start().await;
    mount_basics(&server).await;
    // Page 2 (cursor c1) repeats "102" and adds "104"; it must be matched first.
    Mock::given(path("/s/Bombinhas/homes"))
        .and(query_param("cursor", "c1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(search_page(&[("102", 5000), ("104", 3000)], &[])),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/s/Bombinhas/homes"))
        .and(query_param("adults", "10"))
        .and(query_param("checkin", "2026-12-28"))
        .and(query_param("room_types[]", "Entire home/apt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(search_page(
            &[("101", 9000), ("102", 5000), ("103", 2000)],
            &["c0", "c1"],
        )))
        .expect(1)
        .mount(&server)
        .await;
    mount_listing(&server, "103", 6).await; // cheapest but too small for 10
    mount_listing(&server, "104", 12).await;
    mount_listing(&server, "102", 10).await;
    // "101" is the 4th cheapest: beyond max_listing_details, no page fetched.

    let dir = tempfile::tempdir().unwrap();
    let search = test_search(&server, dir.path(), 2, 3);
    let listings = search
        .search_city("Bombinhas, Santa Catarina, Brazil", &stay(), 10)
        .await
        .unwrap();

    let summary: Vec<_> = listings
        .iter()
        .map(|l| (l.id.as_str(), l.price_per_night, l.max_guests))
        .collect();
    assert_eq!(
        summary,
        vec![("104", 600.0, 12), ("102", 1000.0, 10), ("101", 1800.0, 10)]
    );
    assert_eq!(listings[0].name, "Detail for 12");
    assert!(listings[0].has_pool());
    assert_eq!(listings[2].name, "House 101", "no detail page: search data");
    assert_eq!(listings[0].currency, "BRL");
    assert_eq!(listings[0].url, format!("{}/rooms/104", server.uri()));
}

#[tokio::test]
async fn second_city_reuses_the_session_and_disk_cache() {
    let server = MockServer::start().await;
    Mock::given(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>home</html>"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/nominatim/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(nominatim_body()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/s/Bombinhas/homes"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(search_page(&[("101", 1000)], &[])),
        )
        .expect(1)
        .mount(&server)
        .await;
    mount_listing(&server, "101", 8).await;

    let dir = tempfile::tempdir().unwrap();
    let search = test_search(&server, dir.path(), 1, 1);
    let first = search.search_city("Bombinhas", &stay(), 4).await.unwrap();
    let second = search.search_city("Bombinhas", &stay(), 4).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 1);
    // warm-up + one search + one listing page; nothing for the second city
    assert_eq!(search.http.requests_made(), 3);
}

#[tokio::test]
async fn unknown_city_is_reported() {
    let server = MockServer::start().await;
    Mock::given(path("/nominatim/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let search = test_search(&server, dir.path(), 1, 1);
    let result = search.search_city("Atlantis", &stay(), 4).await;
    assert!(
        matches!(result, Err(AirbnbError::UnknownCity(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn blocked_search_is_an_http_error() {
    let server = MockServer::start().await;
    mount_basics(&server).await;
    Mock::given(method("GET"))
        .and(path("/s/Bombinhas/homes"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let search = test_search(&server, dir.path(), 1, 1);
    let result = search.search_city("Bombinhas", &stay(), 4).await;
    assert!(
        matches!(result, Err(AirbnbError::Http(HttpError::Blocked { .. }))),
        "{result:?}"
    );
}

#[tokio::test]
async fn unparseable_search_page_is_a_parse_error() {
    let server = MockServer::start().await;
    mount_basics(&server).await;
    Mock::given(path("/s/Bombinhas/homes"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>no state</html>"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let search = test_search(&server, dir.path(), 1, 1);
    let result = search.search_city("Bombinhas", &stay(), 4).await;
    assert!(matches!(result, Err(AirbnbError::Parse(_))), "{result:?}");
}

#[tokio::test]
async fn missing_listing_page_keeps_the_search_data() {
    let server = MockServer::start().await;
    mount_basics(&server).await;
    Mock::given(path("/s/Bombinhas/homes"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(search_page(&[("101", 1000)], &[])),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rooms/101"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let search = test_search(&server, dir.path(), 1, 1);
    let listings = search.search_city("Bombinhas", &stay(), 4).await.unwrap();
    assert_eq!(listings.len(), 1);
    assert_eq!(listings[0].name, "House 101");
}

#[test]
fn new_enforces_the_one_second_floor_and_creates_cache_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let search = AirbnbSearch::new(
        AirbnbConfig {
            max_pages: 2,
            max_listing_details: 6,
            min_interval: Duration::from_millis(10),
        },
        dir.path(),
    )
    .unwrap();
    assert_eq!(search.config.min_interval, Duration::from_secs(1));
    assert!(dir.path().join("airbnb").is_dir() && dir.path().join("osm").is_dir());
}
