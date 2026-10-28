//! Markdown report.

use std::path::{Path, PathBuf};

use crate::domain::{CityPlan, FireworksEvidence, Listing, TripRequest};

const TABLE_HEADER: &str = "| Stay | Guests | Bedrooms | Price / person / night | Rating | Pool | Why |\n\
                            |---|---:|---:|---:|---|---|---|";

/// The whole report as markdown: header with the request, one section per
/// city (fireworks evidence with source link, notes, table of stays with
/// price per person per night and links), then the cities without a show.
pub fn render_markdown(
    request: &TripRequest,
    plans: &[CityPlan],
    unconfirmed: &[FireworksEvidence],
) -> String {
    let mut blocks = vec![header(request, plans.len(), unconfirmed.len())];
    blocks.extend(plans.iter().map(|plan| city_section(plan, request.people)));
    if !unconfirmed.is_empty() {
        blocks.push(unconfirmed_section(unconfirmed));
    }
    blocks.join("\n\n") + "\n"
}

fn header(request: &TripRequest, shown: usize, unconfirmed: usize) -> String {
    let stay = &request.stay;
    let eve = stay.new_years_eve().map_or_else(String::new, |d| format!(" New Year's Eve: {d}."));
    format!(
        "# New Year's Eve fireworks: {}\n\n\
         {} to {} ({} nights), {} people.{eve}\n\n\
         {shown} cities with a confirmed fireworks show, {unconfirmed} without one.",
        request.region,
        stay.checkin(),
        stay.checkout(),
        stay.nights(),
        request.people,
    )
}

fn city_section(plan: &CityPlan, people: u32) -> String {
    let mut parts = vec![
        format!("## {}", plan.city.full_name()),
        format!("**Fireworks:** {}", evidence_text(&plan.fireworks)),
    ];
    if let Some(notes) = plan.notes.as_deref().filter(|n| !n.trim().is_empty()) {
        parts.push(notes.trim().to_string());
    }
    parts.push(stays_table(plan, people));
    parts.join("\n\n")
}

/// The summary, followed by a link to its source when there is one.
fn evidence_text(evidence: &FireworksEvidence) -> String {
    let summary = one_line(&evidence.summary);
    match (&evidence.source_url, &evidence.source_title) {
        (Some(url), Some(title)) => format!("{summary} ([{}]({url}))", link_text(title)),
        (Some(url), None) => format!("{summary} ([source]({url}))"),
        (None, _) => summary,
    }
}

fn stays_table(plan: &CityPlan, people: u32) -> String {
    if plan.stays.is_empty() {
        return "No Airbnb stays found.".to_string();
    }
    let rows = plan.stays.iter().map(|(l, reason)| stay_row(l, reason, people));
    std::iter::once(TABLE_HEADER.to_string()).chain(rows).collect::<Vec<_>>().join("\n")
}

fn stay_row(listing: &Listing, reason: &str, people: u32) -> String {
    let rating = listing
        .rating
        .map_or_else(|| "-".to_string(), |r| format!("{r:.2} ({})", listing.review_count));
    let cells = [
        format!("[{}]({})", cell(&link_text(&listing.name)), listing.url),
        listing.max_guests.to_string(),
        listing.bedrooms.to_string(),
        format!("{} {:.0}", cell(&listing.currency), listing.price_per_person_per_night(people)),
        rating,
        if listing.has_pool() { "yes" } else { "no" }.to_string(),
        cell(reason),
    ];
    format!("| {} |", cells.join(" | "))
}

fn unconfirmed_section(unconfirmed: &[FireworksEvidence]) -> String {
    let items = unconfirmed
        .iter()
        .map(|e| format!("- **{}**: {}", e.city, evidence_text(e)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("## Cities without a confirmed show\n\n{items}")
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Safe inside a table cell: one line, pipes escaped.
fn cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// Safe inside `[...]`.
fn link_text(text: &str) -> String {
    one_line(text).replace('[', "(").replace(']', ")")
}

/// `<dir>/<region-slug>_<checkin>.md`, creating `dir` if needed.
pub fn write_report(dir: &Path, request: &TripRequest, markdown: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}_{}.md", slug(&request.region), request.stay.checkin()));
    std::fs::write(&path, markdown)?;
    Ok(path)
}

/// Lowercase ASCII, accents folded, every other run of characters becomes '-'.
fn slug(text: &str) -> String {
    let folded: String = text
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| fold_accent(c).unwrap_or(c))
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = folded.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    if slug.is_empty() { "report".to_string() } else { slug }
}

/// Latin accents seen in Portuguese, Spanish, French and friends.
fn fold_accent(c: char) -> Option<char> {
    let folded = match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        _ => return None,
    };
    Some(folded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CandidateCity, GeoPoint, Stay};
    use chrono::NaiveDate;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn request(region: &str) -> TripRequest {
        TripRequest {
            region: region.into(),
            people: 8,
            stay: Stay::new(date("2026-12-30"), date("2027-01-02")).unwrap(),
        }
    }

    fn listing(name: &str) -> Listing {
        Listing {
            id: "1".into(),
            name: name.into(),
            url: "https://airbnb.com/rooms/1".into(),
            location: GeoPoint { lat: 0.0, lon: 0.0 },
            max_guests: 10,
            bedrooms: 4,
            beds: 6,
            bathrooms: 2.0,
            price_per_night: 800.0,
            currency: "BRL".into(),
            rating: Some(4.87),
            review_count: 31,
            amenities: vec!["Pool".into()],
            description: String::new(),
        }
    }

    fn evidence(city: &str, confirmed: bool) -> FireworksEvidence {
        FireworksEvidence {
            city: city.into(),
            confirmed,
            summary: format!("Summary of {city}."),
            source_url: confirmed.then(|| "https://example.com/fogos".to_string()),
            source_title: confirmed.then(|| "Prefeitura".to_string()),
        }
    }

    fn plan(name: &str, stays: Vec<(Listing, String)>, notes: Option<&str>) -> CityPlan {
        CityPlan {
            city: CandidateCity {
                name: name.into(),
                state: "Santa Catarina".into(),
                country: "Brazil".into(),
                why: String::new(),
            },
            fireworks: evidence(name, true),
            notes: notes.map(String::from),
            stays,
        }
    }

    #[test]
    fn report_has_header_sections_and_table() {
        let plans = [
            plan("Bombinhas", vec![(listing("Casa Azul"), "Pool and space".into())], Some("Great show.")),
            plan("Penha", vec![], None),
        ];
        let md = render_markdown(&request("the coast"), &plans, &[]);

        assert!(md.starts_with("# New Year's Eve fireworks: the coast"));
        assert!(md.contains("2026-12-30 to 2027-01-02 (3 nights), 8 people."));
        assert!(md.contains("New Year's Eve: 2026-12-31"));
        assert!(md.contains("## Bombinhas, Santa Catarina, Brazil"));
        assert!(md.contains("**Fireworks:** Summary of Bombinhas. ([Prefeitura](https://example.com/fogos))"));
        assert!(md.contains("Great show."));
        assert!(md.contains("| Stay | Guests | Bedrooms | Price / person / night | Rating | Pool | Why |"));
        assert!(md.contains(
            "| [Casa Azul](https://airbnb.com/rooms/1) | 10 | 4 | BRL 100 | 4.87 (31) | yes | Pool and space |"
        ));
        assert!(md.contains("## Penha, Santa Catarina, Brazil"));
        assert!(md.contains("No Airbnb stays found"));
        assert!(!md.contains("Cities without a confirmed show"));
    }

    #[test]
    fn pipes_and_newlines_are_escaped_in_cells() {
        let stays = vec![(listing("Casa | Mar"), "near|beach\nquiet".into())];
        let md = render_markdown(&request("x"), &[plan("Bombinhas", stays, None)], &[]);
        assert!(md.contains("[Casa \\| Mar]"));
        assert!(md.contains("near\\|beach quiet"));
    }

    #[test]
    fn missing_rating_and_pool_render_plainly() {
        let mut l = listing("Casa");
        l.rating = None;
        l.amenities = vec![];
        let md = render_markdown(&request("x"), &[plan("Bombinhas", vec![(l, "r".into())], None)], &[]);
        assert!(md.contains("| - | no | r |"));
    }

    #[test]
    fn unconfirmed_cities_are_listed_with_their_summary() {
        let unconfirmed = [evidence("Itapema", false)];
        let md = render_markdown(&request("x"), &[], &unconfirmed);
        assert!(md.contains("## Cities without a confirmed show"));
        assert!(md.contains("- **Itapema**: Summary of Itapema."));
        assert!(md.contains("0 cities with a confirmed fireworks show, 1 without one."));
    }

    #[test]
    fn slug_folds_accents_and_collapses_separators() {
        assert_eq!(slug("the coastline of Santa Catarina"), "the-coastline-of-santa-catarina");
        assert_eq!(slug("Litoral Paulista: São Sebastião, Ilhabela!"), "litoral-paulista-sao-sebastiao-ilhabela");
        assert_eq!(slug("  Ação -- Conceição  "), "acao-conceicao");
        assert_eq!(slug("???"), "report");
    }

    #[test]
    fn write_report_creates_the_directory_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("nested").join("out");
        let path = write_report(&out, &request("Santa Catarina"), "# hi\n").unwrap();
        assert_eq!(path, out.join("santa-catarina_2026-12-30.md"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "# hi\n");
    }
}
