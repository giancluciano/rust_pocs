//! Tools of the stay curator and the travel writer.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use swarms_rs::llm::request::ToolDefinition;
use swarms_rs::structs::tool::Tool;

use super::{DONE_MARKER, ToolFailure, definition, truncate_chars};
use crate::domain::{CandidateCity, Listing, StayPick};
use crate::state::SharedState;

const MAX_REASON_CHARS: usize = 200;
const MAX_NOTES_CHARS: usize = 600;
const MAX_IDS_IN_ERROR: usize = 20;

/// The city the model named, or an error listing the valid ones.
fn listed_city(state: &SharedState, input: &str) -> Result<CandidateCity, ToolFailure> {
    let listed = state.listed_cities();
    let known = state.find_city(input).filter(|c| {
        listed.iter().any(|l| l.name.to_lowercase() == c.name.to_lowercase())
    });
    known.ok_or_else(|| {
        let names: Vec<&str> = listed.iter().map(|c| c.name.as_str()).collect();
        ToolFailure::Invalid(format!(
            "Unknown city '{}'. Use one of these names exactly: {}.",
            input.trim(),
            names.join(", ")
        ))
    })
}

fn names_or_none(names: &[String]) -> String {
    if names.is_empty() { "none".to_string() } else { names.join(", ") }
}

// -------------------------------------------------------------- submit_picks

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PickArg {
    /// The `id=` value of a listing in the brief, copied exactly
    pub listing_id: String,
    /// One short sentence: why this stay suits the group
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubmitPicksArgs {
    /// City name exactly as in the brief, e.g. "Bombinhas"
    pub city: String,
    /// The best stays for the whole group, best first
    pub picks: Vec<PickArg>,
}

/// Records the curator's picks for one city.
#[derive(Clone)]
pub struct SubmitPicksTool {
    state: SharedState,
    per_city: usize,
}

impl SubmitPicksTool {
    pub fn new(state: SharedState, per_city: usize) -> Self {
        Self { state, per_city }
    }

    fn submit(&self, args: SubmitPicksArgs) -> Result<String, ToolFailure> {
        let city = listed_city(&self.state, &args.city)?;
        let listings = self.state.listings_for(&city.name).unwrap_or_default();
        let picks = self.validate(&city, &listings, args.picks)?;
        let count = picks.len();
        self.state.record_picks(&city.name, picks);

        let remaining = self.state.cities_without_picks();
        let saved = format!("Saved {count} picks for {}.", city.name);
        if remaining.is_empty() {
            let done = self.picked_cities();
            Ok(format!("{saved} All cities have picks ({}). {DONE_MARKER}", done.join(", ")))
        } else {
            Ok(format!("{saved} Cities still waiting for picks: {}.", remaining.join(", ")))
        }
    }

    fn validate(
        &self,
        city: &CandidateCity,
        listings: &[Listing],
        picks: Vec<PickArg>,
    ) -> Result<Vec<StayPick>, ToolFailure> {
        if listings.is_empty() {
            return Err(ToolFailure::Invalid(format!(
                "{} has no listings, so there is nothing to pick. Skip this city.",
                city.name
            )));
        }
        if picks.is_empty() {
            return Err(ToolFailure::Invalid("picks is empty. Pick at least one stay.".into()));
        }
        let unknown: Vec<&str> = picks
            .iter()
            .map(|p| p.listing_id.trim())
            .filter(|id| !listings.iter().any(|l| l.id == *id))
            .collect();
        if !unknown.is_empty() {
            let valid: Vec<&str> =
                listings.iter().take(MAX_IDS_IN_ERROR).map(|l| l.id.as_str()).collect();
            return Err(ToolFailure::Invalid(format!(
                "These ids are not listings of {}: {}. Valid ids: {}.",
                city.name,
                unknown.join(", "),
                valid.join(", ")
            )));
        }
        Ok(self.dedupe_and_cap(picks))
    }

    fn dedupe_and_cap(&self, picks: Vec<PickArg>) -> Vec<StayPick> {
        let mut kept: Vec<StayPick> = Vec::new();
        for pick in picks {
            let id = pick.listing_id.trim().to_string();
            if kept.len() < self.per_city && !kept.iter().any(|k| k.listing_id == id) {
                kept.push(StayPick {
                    listing_id: id,
                    reason: truncate_chars(&pick.reason, MAX_REASON_CHARS),
                });
            }
        }
        kept
    }

    fn picked_cities(&self) -> Vec<String> {
        self.state
            .listed_cities()
            .into_iter()
            .filter(|c| self.state.listings_for(&c.name).is_some_and(|l| !l.is_empty()))
            .map(|c| c.name)
            .collect()
    }
}

impl Tool for SubmitPicksTool {
    type Error = ToolFailure;
    type Args = SubmitPicksArgs;
    type Output = String;
    const NAME: &'static str = "submit_picks";

    fn definition(&self) -> ToolDefinition {
        definition::<SubmitPicksArgs>(
            Self::NAME,
            "Submit the picked stays of one city. Call it once per city that has listings.",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.submit(args)
    }
}

// ------------------------------------------------------------ get_city_facts

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CityFactsArgs {
    /// City name, e.g. "Bombinhas"
    pub city: String,
}

/// Read-only: what the writer may state about a city.
#[derive(Clone)]
pub struct GetCityFactsTool {
    state: SharedState,
}

impl GetCityFactsTool {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }

    fn facts(&self, args: CityFactsArgs) -> Result<Value, ToolFailure> {
        let city = listed_city(&self.state, &args.city)?;
        let people = self.state.people();
        let fireworks = self.state.fireworks_for(&city.name).map(|e| {
            json!({
                "confirmed": e.confirmed,
                "summary": e.summary,
                "source_url": e.source_url,
                "source_title": e.source_title,
            })
        });
        let stays: Vec<Value> = self
            .state
            .stays_for(&city.name)
            .iter()
            .map(|(l, reason)| stay_facts(l, reason, people))
            .collect();
        Ok(json!({ "city": city.full_name(), "fireworks": fireworks, "stays": stays }))
    }
}

fn stay_facts(listing: &Listing, reason: &str, people: u32) -> Value {
    json!({
        "name": listing.name,
        "price_per_person_per_night": listing.price_per_person_per_night(people).round(),
        "currency": listing.currency,
        "capacity": listing.max_guests,
        "bedrooms": listing.bedrooms,
        "rating": listing.rating,
        "pool": listing.has_pool(),
        "why_picked": reason,
    })
}

impl Tool for GetCityFactsTool {
    type Error = ToolFailure;
    type Args = CityFactsArgs;
    type Output = Value;
    const NAME: &'static str = "get_city_facts";

    fn definition(&self) -> ToolDefinition {
        definition::<CityFactsArgs>(
            Self::NAME,
            "Get the fireworks evidence and the picked stays of one city. \
             Everything you write must come from this data.",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.facts(args)
    }
}

// -------------------------------------------------------- submit_city_notes

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CityNotesArgs {
    /// City name, e.g. "Bombinhas"
    pub city: String,
    /// Two or three sentences: the New Year's Eve show (where and when, as the
    /// source says) and what the stays are like. At most 600 characters.
    pub notes: String,
}

/// Records the writer's paragraph for one city.
#[derive(Clone)]
pub struct SubmitCityNotesTool {
    state: SharedState,
}

impl SubmitCityNotesTool {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }

    fn submit(&self, args: CityNotesArgs) -> Result<String, ToolFailure> {
        let city = listed_city(&self.state, &args.city)?;
        let notes = truncate_chars(&args.notes, MAX_NOTES_CHARS);
        if notes.is_empty() {
            return Err(ToolFailure::Invalid("notes is empty. Write two or three sentences.".into()));
        }
        self.state.record_notes(&city.name, notes);

        let remaining = self.state.cities_without_notes();
        let saved = format!("Saved notes for {}.", city.name);
        if remaining.is_empty() {
            Ok(format!("{saved} All cities have notes. {DONE_MARKER}"))
        } else {
            Ok(format!("{saved} Cities still waiting for notes: {}.", names_or_none(&remaining)))
        }
    }
}

impl Tool for SubmitCityNotesTool {
    type Error = ToolFailure;
    type Args = CityNotesArgs;
    type Output = String;
    const NAME: &'static str = "submit_city_notes";

    fn definition(&self) -> ToolDefinition {
        definition::<CityNotesArgs>(
            Self::NAME,
            "Submit the short travel notes of one city. Call it once per city.",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.submit(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FireworksEvidence, GeoPoint, Stay, TripRequest};
    use chrono::NaiveDate;

    fn listing(id: &str, price: f64) -> Listing {
        Listing {
            id: id.into(),
            name: format!("House {id}"),
            url: format!("https://airbnb.com/rooms/{id}"),
            location: GeoPoint { lat: 0.0, lon: 0.0 },
            max_guests: 8,
            bedrooms: 3,
            beds: 4,
            bathrooms: 2.0,
            price_per_night: price,
            currency: "BRL".into(),
            rating: Some(4.9),
            review_count: 20,
            amenities: vec![],
            description: String::new(),
        }
    }

    fn city(name: &str) -> CandidateCity {
        CandidateCity {
            name: name.into(),
            state: "SC".into(),
            country: "Brazil".into(),
            why: String::new(),
        }
    }

    /// Bombinhas with 3 listings, Penha without, Itapema with 1.
    fn state() -> SharedState {
        let state = SharedState::default();
        state.set_candidates(vec![city("Bombinhas"), city("Penha"), city("Itapema")]);
        state.set_listings(
            "Bombinhas",
            vec![listing("1", 800.0), listing("2", 400.0), listing("3", 600.0)],
        );
        state.set_listings("Penha", vec![]);
        state.set_listings("Itapema", vec![listing("9", 500.0)]);
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let request = TripRequest {
            region: "SC".into(),
            people: 8,
            stay: Stay::new(d("2026-12-30"), d("2027-01-02")).unwrap(),
        };
        state.set_group(request.people, 2);
        state
    }

    fn picks(city: &str, ids: &[&str]) -> SubmitPicksArgs {
        SubmitPicksArgs {
            city: city.into(),
            picks: ids
                .iter()
                .map(|id| PickArg { listing_id: (*id).into(), reason: "fits".into() })
                .collect(),
        }
    }

    #[tokio::test]
    async fn picks_are_stored_and_reported() {
        let state = state();
        let tool = SubmitPicksTool::new(state.clone(), 2);
        let message = tool.call(picks("bombinhas", &["2", "3"])).await.unwrap();
        assert!(message.contains("Itapema"), "still waiting: {message}");
        assert!(!message.contains(DONE_MARKER));

        let stays = state.stays_for("Bombinhas");
        let ids: Vec<&str> = stays.iter().map(|(l, _)| l.id.as_str()).collect();
        assert_eq!(ids, ["2", "3"]);
    }

    #[tokio::test]
    async fn last_city_with_listings_ends_the_run() {
        let state = state();
        let tool = SubmitPicksTool::new(state, 2);
        tool.call(picks("Bombinhas", &["1"])).await.unwrap();
        let message = tool.call(picks("Itapema", &["9"])).await.unwrap();
        assert!(message.contains(DONE_MARKER));
    }

    #[tokio::test]
    async fn unknown_city_lists_the_valid_ones() {
        let tool = SubmitPicksTool::new(state(), 2);
        let err = tool.call(picks("Floripa", &["1"])).await.unwrap_err().to_string();
        assert!(err.contains("Bombinhas") && err.contains("Itapema"), "{err}");
    }

    #[tokio::test]
    async fn city_without_a_listing_search_is_unknown() {
        let state = SharedState::default();
        state.set_candidates(vec![city("Bombinhas")]);
        let tool = SubmitPicksTool::new(state, 2);
        assert!(tool.call(picks("Bombinhas", &["1"])).await.is_err());
    }

    #[tokio::test]
    async fn ids_from_another_city_are_rejected_and_nothing_is_stored() {
        let state = state();
        let tool = SubmitPicksTool::new(state.clone(), 2);
        let err = tool.call(picks("Bombinhas", &["1", "9"])).await.unwrap_err().to_string();
        assert!(err.contains('9') && err.contains("Valid ids"), "{err}");
        assert!(state.cities_without_picks().contains(&"Bombinhas".to_string()));
    }

    #[tokio::test]
    async fn empty_picks_and_listing_less_cities_are_rejected() {
        let tool = SubmitPicksTool::new(state(), 2);
        assert!(tool.call(picks("Bombinhas", &[])).await.is_err());
        let err = tool.call(picks("Penha", &["1"])).await.unwrap_err().to_string();
        assert!(err.contains("no listings"), "{err}");
    }

    #[tokio::test]
    async fn picks_are_capped_and_deduped() {
        let state = state();
        let tool = SubmitPicksTool::new(state.clone(), 2);
        tool.call(picks("Bombinhas", &["3", "3", "1", "2"])).await.unwrap();
        let ids: Vec<String> =
            state.stays_for("Bombinhas").into_iter().map(|(l, _)| l.id).collect();
        assert_eq!(ids, ["3", "1"]);
    }

    #[tokio::test]
    async fn city_facts_have_evidence_and_stays() {
        let state = state();
        state.record_fireworks(FireworksEvidence {
            city: "Bombinhas".into(),
            confirmed: true,
            summary: "Show at midnight".into(),
            source_url: Some("https://example.com".into()),
            source_title: None,
        });
        let tool = GetCityFactsTool::new(state);
        let facts = tool.call(CityFactsArgs { city: "Bombinhas".into() }).await.unwrap();
        assert_eq!(facts["fireworks"]["summary"], "Show at midnight");
        // No picks yet: the fallback stays (cheapest first, per_city = 2).
        assert_eq!(facts["stays"].as_array().unwrap().len(), 2);
        assert_eq!(facts["stays"][0]["price_per_person_per_night"], 50.0);
        assert_eq!(facts["stays"][0]["capacity"], 8);
    }

    #[tokio::test]
    async fn city_facts_reject_unknown_cities() {
        let tool = GetCityFactsTool::new(state());
        assert!(tool.call(CityFactsArgs { city: "Nowhere".into() }).await.is_err());
    }

    #[tokio::test]
    async fn notes_are_capped_stored_and_end_the_run_when_complete() {
        let state = state();
        let tool = SubmitCityNotesTool::new(state.clone());
        let long = "x".repeat(900);
        for name in ["Bombinhas", "Penha"] {
            let args = CityNotesArgs { city: name.into(), notes: long.clone() };
            let message = tool.call(args).await.unwrap();
            assert!(!message.contains(DONE_MARKER));
        }
        let last = CityNotesArgs { city: "itapema".into(), notes: "Short.".into() };
        assert!(tool.call(last).await.unwrap().contains(DONE_MARKER));

        let plans = state.plans();
        assert_eq!(plans[0].notes.as_ref().unwrap().chars().count(), 600);
        assert_eq!(plans[2].notes.as_deref(), Some("Short."));
    }

    #[tokio::test]
    async fn notes_reject_blank_text_and_unknown_cities() {
        let tool = SubmitCityNotesTool::new(state());
        let blank = CityNotesArgs { city: "Bombinhas".into(), notes: " ".into() };
        assert!(tool.call(blank).await.is_err());
        let unknown = CityNotesArgs { city: "Nowhere".into(), notes: "x".into() };
        assert!(tool.call(unknown).await.is_err());
    }
}
