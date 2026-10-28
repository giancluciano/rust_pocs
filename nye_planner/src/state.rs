//! Shared state the agents write into through tools.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::domain::{
    CandidateCity, CityPlan, FireworksEvidence, Listing, StayPick, TripRequest,
};

/// Used when the report is built before `curation_brief` told us the real value.
const DEFAULT_PER_CITY: usize = 5;
const BRIEF_DESCRIPTION_CHARS: usize = 200;
const RESEARCH_UNFINISHED: &str =
    "Research did not finish: no fireworks information was reported for this city.";
const FALLBACK_FITS: &str = "Cheapest option that fits the group";
const FALLBACK_NO_FIT: &str = "Cheapest option available (capacity is below the group size)";

/// Cities are matched case-insensitively everywhere.
fn city_key(name: &str) -> String {
    name.trim().to_lowercase()
}

#[derive(Debug, Default)]
struct PlanState {
    candidates: Vec<CandidateCity>,
    evidence: HashMap<String, FireworksEvidence>,
    listings: HashMap<String, Vec<Listing>>,
    picks: HashMap<String, Vec<StayPick>>,
    notes: HashMap<String, String>,
    /// Remembered from `curation_brief`; the tools and the fallback need them.
    people: u32,
    per_city: usize,
}

impl PlanState {
    /// Accepts "Bombinhas", "bombinhas ", "Bombinhas, Santa Catarina, Brazil"
    /// and "Bombinhas, SC".
    fn find_candidate(&self, input: &str) -> Option<&CandidateCity> {
        let key = city_key(input);
        let head = city_key(input.split(',').next().unwrap_or(input));
        self.candidates
            .iter()
            .find(|c| city_key(&c.name) == key)
            .or_else(|| self.candidates.iter().find(|c| city_key(&c.full_name()) == key))
            .or_else(|| self.candidates.iter().find(|c| city_key(&c.name) == head))
    }

    fn per_city(&self) -> usize {
        if self.per_city == 0 { DEFAULT_PER_CITY } else { self.per_city }
    }

    /// Candidates that went through `set_listings`, in planner order.
    fn listed_cities(&self) -> impl Iterator<Item = &CandidateCity> {
        self.candidates
            .iter()
            .filter(|c| self.listings.contains_key(&city_key(&c.name)))
    }

    fn evidence_or_missing(&self, city: &CandidateCity) -> FireworksEvidence {
        self.evidence
            .get(&city_key(&city.name))
            .cloned()
            .unwrap_or_else(|| missing_evidence(city))
    }

    /// Curator picks resolved to listings; the cheapest fitting listings if
    /// the curator picked nothing usable.
    fn stays(&self, key: &str) -> Vec<(Listing, String)> {
        let listings = self.listings.get(key).map(Vec::as_slice).unwrap_or_default();
        let picked: Vec<(Listing, String)> = self
            .picks
            .get(key)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|pick| {
                listings
                    .iter()
                    .find(|l| l.id == pick.listing_id)
                    .map(|l| (l.clone(), pick.reason.clone()))
            })
            .collect();
        if picked.is_empty() {
            fallback_stays(listings, self.people, self.per_city())
        } else {
            picked
        }
    }
}

fn missing_evidence(city: &CandidateCity) -> FireworksEvidence {
    FireworksEvidence {
        city: city.name.clone(),
        confirmed: false,
        summary: RESEARCH_UNFINISHED.to_string(),
        source_url: None,
        source_title: None,
    }
}

fn fallback_stays(listings: &[Listing], people: u32, per_city: usize) -> Vec<(Listing, String)> {
    let fitting: Vec<&Listing> = listings.iter().filter(|l| l.max_guests >= people).collect();
    let (pool, reason) = if fitting.is_empty() {
        (listings.iter().collect(), FALLBACK_NO_FIT)
    } else {
        (fitting, FALLBACK_FITS)
    };
    let mut sorted = pool;
    sorted.sort_by(|a, b| a.price_per_night.total_cmp(&b.price_per_night));
    sorted
        .into_iter()
        .take(per_city)
        .map(|l| (l.clone(), reason.to_string()))
        .collect()
}

/// Cheap to clone; every clone sees the same state.
#[derive(Debug, Clone, Default)]
pub struct SharedState(Arc<Mutex<PlanState>>);

impl SharedState {
    /// A panic in another thread must not take the whole run down.
    fn lock(&self) -> MutexGuard<'_, PlanState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Cities proposed by the region planner, in its order.
    pub fn candidates(&self) -> Vec<CandidateCity> {
        self.lock().candidates.clone()
    }

    pub fn set_candidates(&self, cities: Vec<CandidateCity>) {
        self.lock().candidates = cities;
    }

    /// The candidate a free-text city name refers to.
    pub fn find_city(&self, input: &str) -> Option<CandidateCity> {
        self.lock().find_candidate(input).cloned()
    }

    pub fn record_fireworks(&self, evidence: FireworksEvidence) {
        let key = city_key(&evidence.city);
        self.lock().evidence.insert(key, evidence);
    }

    pub fn fireworks_for(&self, city: &str) -> Option<FireworksEvidence> {
        self.lock().evidence.get(&city_key(city)).cloned()
    }

    /// Up to `max` cities with a confirmed fireworks show, in planner order.
    pub fn confirmed(&self, max: usize) -> Vec<(CandidateCity, FireworksEvidence)> {
        let state = self.lock();
        state
            .candidates
            .iter()
            .filter_map(|c| {
                state
                    .evidence
                    .get(&city_key(&c.name))
                    .filter(|e| e.confirmed)
                    .map(|e| (c.clone(), e.clone()))
            })
            .take(max)
            .collect()
    }

    /// Researched cities without a confirmed show, for the report's "not found"
    /// list. Cities whose researcher never reported are included.
    pub fn unconfirmed(&self) -> Vec<FireworksEvidence> {
        let state = self.lock();
        state
            .candidates
            .iter()
            .map(|c| state.evidence_or_missing(c))
            .filter(|e| !e.confirmed)
            .collect()
    }

    pub fn set_listings(&self, city: &str, listings: Vec<Listing>) {
        self.lock().listings.insert(city_key(city), listings);
    }

    pub fn listings_for(&self, city: &str) -> Option<Vec<Listing>> {
        self.lock().listings.get(&city_key(city)).cloned()
    }

    /// Cities that went through `set_listings`, in planner order.
    pub fn listed_cities(&self) -> Vec<CandidateCity> {
        self.lock().listed_cities().cloned().collect()
    }

    pub fn record_picks(&self, city: &str, picks: Vec<StayPick>) {
        self.lock().picks.insert(city_key(city), picks);
    }

    pub fn record_notes(&self, city: &str, notes: String) {
        self.lock().notes.insert(city_key(city), notes);
    }

    /// Cities with listings the curator has not submitted picks for yet.
    pub fn cities_without_picks(&self) -> Vec<String> {
        let state = self.lock();
        state
            .listed_cities()
            .filter(|c| {
                let key = city_key(&c.name);
                state.listings.get(&key).is_some_and(|l| !l.is_empty())
                    && !state.picks.contains_key(&key)
            })
            .map(|c| c.name.clone())
            .collect()
    }

    /// Cities the writer has not submitted notes for yet.
    pub fn cities_without_notes(&self) -> Vec<String> {
        let state = self.lock();
        state
            .listed_cities()
            .filter(|c| !state.notes.contains_key(&city_key(&c.name)))
            .map(|c| c.name.clone())
            .collect()
    }

    /// Group size remembered from `curation_brief` (0 before it ran).
    pub fn people(&self) -> u32 {
        self.lock().people
    }

    /// The stays shown for a city: curator picks, or the cheapest fitting ones.
    pub fn stays_for(&self, city: &str) -> Vec<(Listing, String)> {
        self.lock().stays(&city_key(city))
    }

    /// Text handed to the curator: per city, the candidate listings (id, price
    /// per person per night, capacity, rooms, rating, pool, short description).
    pub fn curation_brief(&self, request: &TripRequest, per_city: usize) -> String {
        let mut state = self.lock();
        state.people = request.people;
        state.per_city = per_city;
        let header = brief_header(request, per_city);
        let sections: Vec<String> = state
            .listed_cities()
            .map(|c| {
                let listings = state.listings.get(&city_key(&c.name)).map(Vec::as_slice);
                city_brief(c, listings.unwrap_or_default(), request.people, per_city)
            })
            .collect();
        format!("{header}\n\n{}", sections.join("\n\n"))
    }

    /// Final per-city results for the report: every city passed to
    /// `set_listings` (possibly with no listings), in planner order.
    pub fn plans(&self) -> Vec<CityPlan> {
        let state = self.lock();
        state
            .listed_cities()
            .map(|c| {
                let key = city_key(&c.name);
                CityPlan {
                    city: c.clone(),
                    fireworks: state.evidence_or_missing(c),
                    notes: state.notes.get(&key).cloned(),
                    stays: state.stays(&key),
                }
            })
            .collect()
    }
}

fn brief_header(request: &TripRequest, per_city: usize) -> String {
    let stay = &request.stay;
    let eve = stay
        .new_years_eve()
        .map_or_else(|| "not inside the stay".to_string(), |d| d.to_string());
    format!(
        "Group: {} people. Stay: {} to {} ({} nights). New Year's Eve: {}.\n\
         Pick up to {} stays per city that suit the whole group. Prices are per person per night.",
        request.people,
        stay.checkin(),
        stay.checkout(),
        stay.nights(),
        eve,
        per_city,
    )
}

fn city_brief(city: &CandidateCity, listings: &[Listing], people: u32, per_city: usize) -> String {
    let title = format!("## {} (use city = \"{}\")", city.full_name(), city.name);
    if listings.is_empty() {
        return format!("{title}\nNo Airbnb listings were found for this city.");
    }
    let mut shown: Vec<&Listing> = listings.iter().collect();
    // Stable: listings that fit the group come first, scraper order is kept.
    shown.sort_by_key(|l| l.max_guests < people);
    let lines: Vec<String> = shown
        .into_iter()
        .take(per_city * 2)
        .map(|l| listing_line(l, people))
        .collect();
    format!("{title}\n{}", lines.join("\n"))
}

fn listing_line(l: &Listing, people: u32) -> String {
    let rating = l.rating.map_or_else(
        || "no rating".to_string(),
        |r| format!("{r:.2} ({} reviews)", l.review_count),
    );
    format!(
        "- id={} | {} {:.0}/person/night | sleeps {} | {} bedrooms, {} beds | rating {} | pool: {} | {}",
        l.id,
        l.currency,
        l.price_per_person_per_night(people),
        l.max_guests,
        l.bedrooms,
        l.beds,
        rating,
        if l.has_pool() { "yes" } else { "no" },
        short_description(&l.description),
    )
}

fn short_description(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(BRIEF_DESCRIPTION_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GeoPoint, Stay};
    use chrono::NaiveDate;

    fn city(name: &str) -> CandidateCity {
        CandidateCity {
            name: name.into(),
            state: "Santa Catarina".into(),
            country: "Brazil".into(),
            why: String::new(),
        }
    }

    fn evidence(city: &str, confirmed: bool) -> FireworksEvidence {
        FireworksEvidence {
            city: city.into(),
            confirmed,
            summary: format!("summary {city}"),
            source_url: confirmed.then(|| "https://example.com/show".to_string()),
            source_title: None,
        }
    }

    fn listing(id: &str, price: f64, guests: u32) -> Listing {
        Listing {
            id: id.into(),
            name: format!("House {id}"),
            url: format!("https://airbnb.com/rooms/{id}"),
            location: GeoPoint { lat: 0.0, lon: 0.0 },
            max_guests: guests,
            bedrooms: 3,
            beds: 5,
            bathrooms: 2.0,
            price_per_night: price,
            currency: "BRL".into(),
            rating: Some(4.8),
            review_count: 12,
            amenities: vec!["Pool".into()],
            description: "Nice  house\nnear the beach".into(),
        }
    }

    fn request(people: u32) -> TripRequest {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        TripRequest {
            region: "Santa Catarina".into(),
            people,
            stay: Stay::new(d("2026-12-30"), d("2027-01-02")).unwrap(),
        }
    }

    fn state_with(names: &[&str]) -> SharedState {
        let state = SharedState::default();
        state.set_candidates(names.iter().map(|n| city(n)).collect());
        state
    }

    #[test]
    fn confirmed_keeps_planner_order_and_respects_max() {
        let state = state_with(&["Bombinhas", "Itapema", "Balneario Camboriu"]);
        state.record_fireworks(evidence("Balneario Camboriu", true));
        state.record_fireworks(evidence("Bombinhas", true));
        state.record_fireworks(evidence("Itapema", false));

        let all: Vec<String> = state.confirmed(10).into_iter().map(|(c, _)| c.name).collect();
        assert_eq!(all, ["Bombinhas", "Balneario Camboriu"]);
        assert_eq!(state.confirmed(1).len(), 1);
        assert_eq!(state.confirmed(1)[0].0.name, "Bombinhas");
    }

    #[test]
    fn evidence_lookup_ignores_case() {
        let state = state_with(&["Bombinhas"]);
        state.record_fireworks(evidence("bombinhas", true));
        assert_eq!(state.confirmed(5).len(), 1);
    }

    #[test]
    fn unconfirmed_includes_candidates_without_evidence() {
        let state = state_with(&["Bombinhas", "Itapema", "Penha"]);
        state.record_fireworks(evidence("Bombinhas", true));
        state.record_fireworks(evidence("Itapema", false));

        let unconfirmed = state.unconfirmed();
        assert_eq!(unconfirmed.len(), 2);
        assert_eq!(unconfirmed[0].city, "Itapema");
        assert_eq!(unconfirmed[1].city, "Penha");
        assert!(unconfirmed[1].summary.contains("did not finish"));
        assert!(!unconfirmed[1].confirmed);
    }

    #[test]
    fn find_city_accepts_full_names() {
        let state = state_with(&["Bombinhas"]);
        assert!(state.find_city("  bombinhas ").is_some());
        assert!(state.find_city("Bombinhas, Santa Catarina, Brazil").is_some());
        assert!(state.find_city("Bombinhas, SC").is_some());
        assert!(state.find_city("Itapema").is_none());
    }

    #[test]
    fn plans_cover_listed_cities_in_planner_order() {
        let state = state_with(&["Bombinhas", "Itapema", "Penha"]);
        state.set_listings("Penha", vec![]);
        state.set_listings("Bombinhas", vec![listing("1", 300.0, 8)]);
        let names: Vec<String> = state.plans().into_iter().map(|p| p.city.name).collect();
        assert_eq!(names, ["Bombinhas", "Penha"]);
        assert!(state.plans()[1].stays.is_empty());
    }

    #[test]
    fn plans_resolve_picks_and_drop_unknown_ids() {
        let state = state_with(&["Bombinhas"]);
        state.set_listings("Bombinhas", vec![listing("1", 300.0, 8), listing("2", 200.0, 8)]);
        state.record_picks(
            "Bombinhas",
            vec![
                StayPick { listing_id: "2".into(), reason: "best".into() },
                StayPick { listing_id: "nope".into(), reason: "ghost".into() },
            ],
        );
        state.record_notes("Bombinhas", "Fireworks at the beach.".into());

        let plan = &state.plans()[0];
        assert_eq!(plan.stays.len(), 1);
        assert_eq!(plan.stays[0].0.id, "2");
        assert_eq!(plan.stays[0].1, "best");
        assert_eq!(plan.notes.as_deref(), Some("Fireworks at the beach."));
        assert_eq!(plan.fireworks.city, "Bombinhas");
    }

    #[test]
    fn plans_fall_back_to_cheapest_fitting_listings() {
        let state = state_with(&["Bombinhas"]);
        state.set_listings(
            "Bombinhas",
            vec![
                listing("big-expensive", 900.0, 10),
                listing("small-cheap", 100.0, 2),
                listing("big-cheap", 400.0, 10),
                listing("big-mid", 500.0, 10),
            ],
        );
        let _ = state.curation_brief(&request(8), 2);

        let stays = state.plans().remove(0).stays;
        let ids: Vec<&str> = stays.iter().map(|(l, _)| l.id.as_str()).collect();
        assert_eq!(ids, ["big-cheap", "big-mid"]);
        assert_eq!(stays[0].1, "Cheapest option that fits the group");
    }

    #[test]
    fn plans_fall_back_when_all_picked_ids_are_unknown() {
        let state = state_with(&["Bombinhas"]);
        state.set_listings("Bombinhas", vec![listing("1", 300.0, 8)]);
        state.record_picks(
            "Bombinhas",
            vec![StayPick { listing_id: "ghost".into(), reason: "x".into() }],
        );
        assert_eq!(state.plans()[0].stays[0].0.id, "1");
    }

    #[test]
    fn pending_cities_track_picks_and_notes() {
        let state = state_with(&["Bombinhas", "Itapema", "Penha"]);
        state.set_listings("Bombinhas", vec![listing("1", 300.0, 8)]);
        state.set_listings("Itapema", vec![listing("2", 300.0, 8)]);
        state.set_listings("Penha", vec![]);
        assert_eq!(state.cities_without_picks(), ["Bombinhas", "Itapema"]);
        assert_eq!(state.cities_without_notes(), ["Bombinhas", "Itapema", "Penha"]);

        state.record_picks("bombinhas", vec![]);
        state.record_notes("Penha", "n".into());
        assert_eq!(state.cities_without_picks(), ["Itapema"]);
        assert_eq!(state.cities_without_notes(), ["Bombinhas", "Itapema"]);
    }

    #[test]
    fn brief_lists_header_listings_and_empty_cities() {
        let state = state_with(&["Bombinhas", "Penha"]);
        let many: Vec<Listing> = (0..10).map(|i| listing(&i.to_string(), 400.0, 8)).collect();
        state.set_listings("Bombinhas", many);
        state.set_listings("Penha", vec![]);

        let brief = state.curation_brief(&request(8), 2);
        assert!(brief.contains("8 people"));
        assert!(brief.contains("3 nights"));
        assert!(brief.contains("2026-12-31"));
        assert!(brief.contains("Bombinhas, Santa Catarina, Brazil"));
        assert_eq!(brief.matches("- id=").count(), 4, "per_city * 2 listings");
        assert!(brief.contains("BRL 50/person/night"));
        assert!(brief.contains("pool: yes"));
        assert!(brief.contains("Nice house near the beach"));
        assert!(brief.contains("No Airbnb listings were found"));
    }

    #[test]
    fn brief_shows_fitting_listings_first() {
        let state = state_with(&["Bombinhas"]);
        state.set_listings(
            "Bombinhas",
            vec![listing("small", 100.0, 2), listing("big", 400.0, 10), listing("huge", 500.0, 12)],
        );
        let brief = state.curation_brief(&request(8), 1);
        assert!(brief.contains("id=big") && brief.contains("id=huge"));
        assert!(!brief.contains("id=small"));
    }

    #[test]
    fn poisoned_lock_does_not_panic() {
        let state = state_with(&["Bombinhas"]);
        let clone = state.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.0.lock();
            panic!("poison the lock");
        })
        .join();
        assert_eq!(state.candidates().len(), 1);
    }
}
