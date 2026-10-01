use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::domain::Stay;
use crate::enrich::EnrichedListing;
use crate::options::OptionKind;
use crate::pipeline::{RankedOption, Report};
use crate::provider::fixture::slugify;

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Whole reais: nobody votes on centavos.
fn reais(x: f64) -> u64 {
    x.max(0.0).round() as u64
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// One row per voting option, with human headers and no internal ids.
/// All prices are in BRL.
#[derive(Debug, Serialize)]
struct OptionRow {
    #[serde(rename = "Rank")]
    rank: usize,
    #[serde(rename = "Setup")]
    setup: &'static str,
    #[serde(rename = "Main house")]
    main_name: String,
    #[serde(rename = "Main house link")]
    main_url: String,
    #[serde(rename = "Main house sleeps")]
    main_sleeps: u32,
    #[serde(rename = "Main house pool")]
    main_pool: &'static str,
    #[serde(rename = "Main house to beach (km)")]
    main_beach_km: Option<f64>,
    #[serde(rename = "Nearest beach")]
    nearest_beach: String,
    #[serde(rename = "Main house to center (km)")]
    main_center_km: f64,
    #[serde(rename = "Sleeping house")]
    sleeping_name: String,
    #[serde(rename = "Sleeping house link")]
    sleeping_url: String,
    #[serde(rename = "Sleeping house sleeps")]
    sleeping_sleeps: Option<u32>,
    #[serde(rename = "Walk between houses (m)")]
    walk_m: Option<u32>,
    #[serde(rename = "Total sleeps")]
    total_sleeps: u32,
    #[serde(rename = "Bedrooms")]
    bedrooms: u32,
    #[serde(rename = "Beds")]
    beds: u32,
    #[serde(rename = "Price per night (R$)")]
    price_per_night: u64,
    #[serde(rename = "Total stay (R$)")]
    total_stay: u64,
    #[serde(rename = "Per person per night (R$)")]
    per_person_night: u64,
    #[serde(rename = "Per person total (R$)")]
    per_person_total: u64,
    #[serde(rename = "Rating")]
    rating: Option<f64>,
    #[serde(rename = "Reviews")]
    reviews: u32,
    #[serde(rename = "Why vote for it")]
    pitch: String,
    #[serde(rename = "Watch out for")]
    watch_out: String,
    #[serde(rename = "Votes")]
    votes: String,
}

fn option_row(r: &RankedOption, group_size: u32, nights: u32) -> OptionRow {
    let o = &r.option;
    let main = o.main_house();
    let sleeping = o.sleeping_house();
    let nights = f64::from(nights);
    let people = f64::from(group_size.max(1));
    OptionRow {
        rank: r.rank,
        setup: match o.kind {
            OptionKind::Single => "One house for everyone",
            OptionKind::Pair => "Main house + sleeping house",
        },
        main_name: main.listing.name.clone(),
        main_url: main.listing.url.clone(),
        main_sleeps: main.realistic_capacity,
        main_pool: yes_no(main.has_pool),
        main_beach_km: main.distance_beach_km.map(round1),
        nearest_beach: main
            .nearest_beach
            .clone()
            .filter(|name| name != crate::osm::UNNAMED_BEACH)
            .unwrap_or_default(),
        main_center_km: round1(main.distance_center_km),
        sleeping_name: sleeping.map(|s| s.listing.name.clone()).unwrap_or_default(),
        sleeping_url: sleeping.map(|s| s.listing.url.clone()).unwrap_or_default(),
        sleeping_sleeps: sleeping.map(|s| s.realistic_capacity),
        walk_m: o.pair_distance_m.map(|m| m.round() as u32),
        total_sleeps: o.capacity(),
        bedrooms: o.bedrooms(),
        beds: o.beds(),
        price_per_night: reais(o.price_per_night()),
        total_stay: reais(o.price_per_night() * nights),
        per_person_night: reais(o.price_per_night() / people),
        per_person_total: reais(o.price_per_night() * nights / people),
        rating: o.avg_rating().map(round2),
        reviews: o.review_count(),
        pitch: r.pitch.clone().unwrap_or_default(),
        watch_out: o.red_flags().join("; "),
        votes: String::new(),
    }
}

/// Every listing found, for whoever wants to dig deeper. Prices in BRL.
#[derive(Debug, Serialize)]
struct ListingRow<'a> {
    name: &'a str,
    url: &'a str,
    lat: f64,
    lon: f64,
    max_guests: u32,
    realistic_capacity: u32,
    bedrooms: u32,
    beds: u32,
    bathrooms: f32,
    price_per_night_brl: u64,
    rating: Option<f32>,
    review_count: u32,
    distance_center_km: f64,
    nearest_beach: &'a str,
    distance_beach_km: Option<f64>,
    has_pool: bool,
    vetted: bool,
    red_flags: String,
}

fn listing_row(e: &EnrichedListing) -> ListingRow<'_> {
    let l = &e.listing;
    ListingRow {
        name: &l.name,
        url: &l.url,
        lat: l.location.lat,
        lon: l.location.lon,
        max_guests: l.max_guests,
        realistic_capacity: e.realistic_capacity,
        bedrooms: l.bedrooms,
        beds: l.beds,
        bathrooms: l.bathrooms,
        price_per_night_brl: reais(l.price_per_night),
        rating: l.rating,
        review_count: l.review_count,
        distance_center_km: round2(e.distance_center_km),
        nearest_beach: e.nearest_beach.as_deref().unwrap_or(""),
        distance_beach_km: e.distance_beach_km.map(round2),
        has_pool: e.has_pool,
        vetted: e.vetted,
        red_flags: e.red_flags.join("; "),
    }
}

fn write_rows<T: Serialize>(path: &Path, rows: impl IntoIterator<Item = T>) -> Result<()> {
    let mut writer =
        csv::Writer::from_path(path).with_context(|| format!("creating {}", path.display()))?;
    for row in rows {
        writer.serialize(row)?;
    }
    writer.flush()?;
    Ok(())
}

/// Writes `<slug>_<checkin>_options.csv` and `<slug>_<checkin>_listings.csv`.
/// Returns (options_path, listings_path).
pub fn write_report(
    dir: &Path,
    report: &Report,
    group_size: u32,
    stay: &Stay,
) -> Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let prefix = format!("{}_{}", slugify(&report.destination.city), stay.checkin());
    let options_path = dir.join(format!("{prefix}_options.csv"));
    let listings_path = dir.join(format!("{prefix}_listings.csv"));

    write_rows(
        &options_path,
        report
            .options
            .iter()
            .map(|r| option_row(r, group_size, stay.nights())),
    )?;
    write_rows(&listings_path, report.listings.iter().map(listing_row))?;
    Ok((options_path, listings_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::{date, listing};
    use crate::domain::{Destination, GeoPoint};
    use crate::options::GroupOption;

    fn report() -> Report {
        let dest = Destination {
            city: "Florianópolis".into(),
            country: "BR".into(),
            center: GeoPoint { lat: 0.0, lon: 0.0 },
            beaches: vec![],
        };
        let main = EnrichedListing {
            has_pool: true,
            red_flags: vec!["no towels".into()],
            ..EnrichedListing::new(listing("main-id", 0.0, 0.0, 14, 1500.0), &dest)
        };
        let sleeping = EnrichedListing::new(listing("sleep-id", 0.001, 0.0, 6, 500.0), &dest);
        Report {
            destination: dest,
            listings: vec![main.clone(), sleeping.clone()],
            options: vec![RankedOption {
                rank: 1,
                option: GroupOption {
                    id: "P-main-id+sleep-id".into(),
                    kind: OptionKind::Pair,
                    listings: vec![main, sleeping],
                    pair_distance_m: Some(111.2),
                },
                score: 55.555,
                pitch: Some("Nice, with a comma".into()),
            }],
        }
    }

    #[test]
    fn options_csv_is_shareable() {
        let stay = Stay::new(date("2027-01-10"), date("2027-01-15")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (opts, _) = write_report(dir.path(), &report(), 20, &stay).unwrap();
        assert!(opts.ends_with("florianopolis_2027-01-10_options.csv"));

        let text = std::fs::read_to_string(opts).unwrap();
        assert!(
            !text.contains("main-id") || text.contains("/main-id"),
            "ids only inside links"
        );
        let mut reader = csv::Reader::from_reader(text.as_bytes());
        let headers = reader.headers().unwrap().clone();
        assert!(headers.iter().all(|h| !h.to_lowercase().contains("id")));
        assert!(
            headers
                .iter()
                .all(|h| !h.to_lowercase().contains("currency"))
        );
        let row = reader.records().next().unwrap().unwrap();
        let col = |name: &str| row[headers.iter().position(|h| h == name).unwrap()].to_string();

        assert_eq!(col("Setup"), "Main house + sleeping house");
        assert_eq!(col("Main house"), "Listing main-id");
        assert_eq!(col("Main house pool"), "yes");
        assert_eq!(col("Main house sleeps"), "14");
        assert_eq!(col("Sleeping house sleeps"), "6");
        assert_eq!(col("Walk between houses (m)"), "111");
        assert_eq!(col("Price per night (R$)"), "2000");
        assert_eq!(col("Total stay (R$)"), "10000");
        assert_eq!(col("Per person total (R$)"), "500");
        assert_eq!(col("Why vote for it"), "Nice, with a comma");
        assert_eq!(col("Watch out for"), "Main house: no towels");
        assert_eq!(col("Votes"), "");
    }

    #[test]
    fn listings_csv_has_no_id_or_currency_columns() {
        let stay = Stay::new(date("2027-01-10"), date("2027-01-15")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (_, lists) = write_report(dir.path(), &report(), 20, &stay).unwrap();
        let text = std::fs::read_to_string(lists).unwrap();
        let header = text.lines().next().unwrap();
        assert!(header.starts_with("name,url,"));
        assert!(!header.split(',').any(|h| h == "id" || h == "currency"));
        assert_eq!(text.lines().count(), 3);
    }
}
