//! swarms-rs tools. Implemented by hand (instead of `#[tool]`) because they
//! need access to the provider and the shared state.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use swarms_rs::llm::request::ToolDefinition;
use swarms_rs::structs::tool::Tool;

use crate::enrich::{EnrichedListing, ListingReview};
use crate::provider::{ProviderError, SearchQuery};
use crate::state::{ResearchContext, SharedState};

const MAX_GUESTS_LIMIT: u32 = 50;
const MAX_SEARCH_RESULTS: usize = 40;
const MAX_PITCH_CHARS: usize = 300;

#[derive(Debug, thiserror::Error)]
pub enum ToolFailure {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("search failed: {0}")]
    Provider(#[from] ProviderError),
    #[error("internal state unavailable")]
    State,
    #[error("internal error: {0}")]
    Internal(String),
}

fn definition<T: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: serde_json::to_value(schemars::schema_for!(T))
            .expect("JSON schema is always serializable"),
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ---------------------------------------------------------------- search

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Minimum number of guests the listing must accept
    pub min_guests: u32,
    /// Optional maximum number of guests (useful to find mid-size places for splitting the group)
    #[serde(default)]
    pub max_guests: Option<u32>,
    /// Optional minimum number of bedrooms
    #[serde(default)]
    pub min_bedrooms: Option<u32>,
    /// Optional maximum price per night for the whole listing
    #[serde(default)]
    pub max_price_per_night: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ListingSummary {
    pub id: String,
    pub name: String,
    pub max_guests: u32,
    pub bedrooms: u32,
    pub beds: u32,
    pub price_per_night: f64,
    pub currency: String,
    pub lat: f64,
    pub lon: f64,
    pub distance_center_km: f64,
    pub nearest_beach: Option<String>,
    pub distance_beach_km: Option<f64>,
    pub has_pool: bool,
}

impl From<&EnrichedListing> for ListingSummary {
    fn from(e: &EnrichedListing) -> Self {
        Self {
            id: e.listing.id.clone(),
            name: e.listing.name.clone(),
            max_guests: e.listing.max_guests,
            bedrooms: e.listing.bedrooms,
            beds: e.listing.beds,
            price_per_night: e.listing.price_per_night,
            currency: e.listing.currency.clone(),
            lat: e.listing.location.lat,
            lon: e.listing.location.lon,
            distance_center_km: round2(e.distance_center_km),
            nearest_beach: e.nearest_beach.clone(),
            distance_beach_km: e.distance_beach_km.map(round2),
            has_pool: e.has_pool,
        }
    }
}

#[derive(Clone)]
pub struct SearchListingsTool {
    ctx: Arc<ResearchContext>,
    state: SharedState,
}

impl SearchListingsTool {
    pub fn new(ctx: Arc<ResearchContext>, state: SharedState) -> Self {
        Self { ctx, state }
    }

    fn validate(args: &SearchArgs) -> Result<(), ToolFailure> {
        if args.min_guests == 0 || args.min_guests > MAX_GUESTS_LIMIT {
            return Err(ToolFailure::InvalidArgs(format!(
                "min_guests must be between 1 and {MAX_GUESTS_LIMIT}"
            )));
        }
        if args.max_guests.is_some_and(|max| max < args.min_guests) {
            return Err(ToolFailure::InvalidArgs(
                "max_guests must be >= min_guests".to_string(),
            ));
        }
        if args
            .max_price_per_night
            .is_some_and(|p| p.is_nan() || p <= 0.0)
        {
            return Err(ToolFailure::InvalidArgs(
                "max_price_per_night must be positive".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn search(&self, args: SearchArgs) -> Result<Vec<ListingSummary>, ToolFailure> {
        Self::validate(&args)?;
        let query = SearchQuery {
            stay: self.ctx.stay,
            min_guests: args.min_guests,
            max_guests: args.max_guests,
            min_bedrooms: args.min_bedrooms,
            max_price_per_night: args.max_price_per_night,
        };
        tracing::info!(?query, "search_listings called");
        let destination = self.ctx.provider.destination();
        let enriched: Vec<EnrichedListing> = self
            .ctx
            .provider
            .search(&query)
            .await?
            .into_iter()
            .take(MAX_SEARCH_RESULTS)
            .map(|l| EnrichedListing::new(l, destination))
            .collect();

        let mut state = self.state.lock().map_err(|_| ToolFailure::State)?;
        for e in &enriched {
            state
                .found
                .entry(e.id().to_string())
                .or_insert_with(|| e.clone());
        }
        Ok(enriched.iter().map(ListingSummary::from).collect())
    }
}

impl Tool for SearchListingsTool {
    type Error = ToolFailure;
    type Args = SearchArgs;
    type Output = Vec<ListingSummary>;
    const NAME: &'static str = "search_listings";

    fn definition(&self) -> ToolDefinition {
        definition::<SearchArgs>(
            Self::NAME,
            "Search available vacation rentals in the trip city for the trip dates. \
             Returns listings with capacity, price, coordinates, distance to city center, \
             distance to nearest beach and pool flag.",
        )
    }

    fn call(
        &self,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send + Sync {
        // swarms-rs requires a Sync future; HTTP futures are not Sync, so the
        // work runs in its own task and we only hold the (Sync) JoinHandle.
        let tool = self.clone();
        let handle = tokio::spawn(async move { tool.search(args).await });
        async move {
            handle
                .await
                .map_err(|e| ToolFailure::Internal(e.to_string()))?
        }
    }
}

// ---------------------------------------------------------------- vetting

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReviewsArgs {
    /// One review per listing
    pub reviews: Vec<ListingReview>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SubmitResult {
    pub accepted: usize,
    pub rejected_ids: Vec<String>,
}

pub struct SubmitReviewsTool {
    valid_ids: Arc<BTreeSet<String>>,
    state: SharedState,
}

impl SubmitReviewsTool {
    pub fn new(valid_ids: Arc<BTreeSet<String>>, state: SharedState) -> Self {
        Self { valid_ids, state }
    }

    pub fn submit(&self, args: ReviewsArgs) -> Result<SubmitResult, ToolFailure> {
        let (valid, rejected): (Vec<_>, Vec<_>) = args
            .reviews
            .into_iter()
            .partition(|r| self.valid_ids.contains(&r.id));
        let mut state = self.state.lock().map_err(|_| ToolFailure::State)?;
        let accepted = valid.len();
        for review in valid {
            state.reviews.insert(review.id.clone(), review);
        }
        Ok(SubmitResult {
            accepted,
            rejected_ids: rejected.into_iter().map(|r| r.id).collect(),
        })
    }
}

impl Tool for SubmitReviewsTool {
    type Error = ToolFailure;
    type Args = ReviewsArgs;
    type Output = SubmitResult;
    const NAME: &'static str = "submit_reviews";

    fn definition(&self) -> ToolDefinition {
        definition::<ReviewsArgs>(
            Self::NAME,
            "Submit your review of the listings: realistic sleeping capacity, whether a \
             usable pool is confirmed, and red flags. Call once with all listings.",
        )
    }

    fn call(
        &self,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send + Sync {
        let result = self.submit(args);
        async move { result }
    }
}

// ---------------------------------------------------------------- pitches

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OptionPitch {
    /// Option id exactly as provided
    pub option_id: String,
    /// One sentence (max ~30 words) for the group vote
    pub pitch: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PitchesArgs {
    /// One pitch per option
    pub pitches: Vec<OptionPitch>,
}

pub struct SubmitPitchesTool {
    valid_ids: Arc<BTreeSet<String>>,
    state: SharedState,
}

impl SubmitPitchesTool {
    pub fn new(valid_ids: Arc<BTreeSet<String>>, state: SharedState) -> Self {
        Self { valid_ids, state }
    }

    pub fn submit(&self, args: PitchesArgs) -> Result<SubmitResult, ToolFailure> {
        let (valid, rejected): (Vec<_>, Vec<_>) = args
            .pitches
            .into_iter()
            .partition(|p| self.valid_ids.contains(&p.option_id));
        let mut state = self.state.lock().map_err(|_| ToolFailure::State)?;
        let accepted = valid.len();
        for p in valid {
            let pitch: String = p.pitch.trim().chars().take(MAX_PITCH_CHARS).collect();
            state.pitches.insert(p.option_id, pitch);
        }
        Ok(SubmitResult {
            accepted,
            rejected_ids: rejected.into_iter().map(|p| p.option_id).collect(),
        })
    }
}

impl Tool for SubmitPitchesTool {
    type Error = ToolFailure;
    type Args = PitchesArgs;
    type Output = SubmitResult;
    const NAME: &'static str = "submit_pitches";

    fn definition(&self) -> ToolDefinition {
        definition::<PitchesArgs>(
            Self::NAME,
            "Submit a one-sentence pitch for each option in the group vote. Call once with all options.",
        )
    }

    fn call(
        &self,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send + Sync {
        let result = self.submit(args);
        async move { result }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::Stay;
    use crate::domain::test_support::date;
    use crate::provider::fixture::FixtureProvider;

    fn ctx() -> Arc<ResearchContext> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        Arc::new(ResearchContext {
            provider: Arc::new(FixtureProvider::load(&dir, "florianopolis").unwrap()),
            stay: Stay::new(date("2027-01-10"), date("2027-01-17")).unwrap(),
            group_size: 20,
        })
    }

    fn args(min_guests: u32) -> SearchArgs {
        SearchArgs {
            min_guests,
            max_guests: None,
            min_bedrooms: None,
            max_price_per_night: None,
        }
    }

    #[tokio::test]
    async fn search_records_found_listings_without_duplicates() {
        let state = SharedState::default();
        let tool = SearchListingsTool::new(ctx(), state.clone());
        let big = tool.search(args(20)).await.unwrap();
        assert!(big.iter().all(|s| s.max_guests >= 20));
        tool.search(args(20)).await.unwrap();
        assert_eq!(state.lock().unwrap().found.len(), big.len());
    }

    #[tokio::test]
    async fn search_rejects_bad_args() {
        let tool = SearchListingsTool::new(ctx(), SharedState::default());
        assert!(tool.search(args(0)).await.is_err());
        assert!(
            tool.search(SearchArgs {
                max_guests: Some(5),
                ..args(10)
            })
            .await
            .is_err()
        );
        assert!(
            tool.search(SearchArgs {
                max_price_per_night: Some(-1.0),
                ..args(10)
            })
            .await
            .is_err()
        );
    }

    #[test]
    fn tool_definition_has_object_schema() {
        let tool = SearchListingsTool::new(ctx(), SharedState::default());
        let def = Tool::definition(&tool);
        assert_eq!(def.name, "search_listings");
        assert_eq!(def.parameters["type"], "object");
        assert!(def.parameters["properties"]["min_guests"].is_object());
    }

    #[test]
    fn reviews_for_unknown_ids_are_rejected() {
        let state = SharedState::default();
        let ids = Arc::new(BTreeSet::from(["fx-001".to_string()]));
        let tool = SubmitReviewsTool::new(ids, state.clone());
        let review = |id: &str| ListingReview {
            id: id.into(),
            realistic_capacity: 18,
            pool_confirmed: true,
            red_flags: vec![],
        };
        let result = tool
            .submit(ReviewsArgs {
                reviews: vec![review("fx-001"), review("made-up")],
            })
            .unwrap();
        assert_eq!(
            result,
            SubmitResult {
                accepted: 1,
                rejected_ids: vec!["made-up".into()]
            }
        );
        assert!(state.lock().unwrap().reviews.contains_key("fx-001"));
    }

    #[test]
    fn pitches_are_trimmed_and_capped() {
        let state = SharedState::default();
        let ids = Arc::new(BTreeSet::from(["S-fx-001".to_string()]));
        let tool = SubmitPitchesTool::new(ids, state.clone());
        let long = format!("  {}  ", "x".repeat(1000));
        tool.submit(PitchesArgs {
            pitches: vec![OptionPitch {
                option_id: "S-fx-001".into(),
                pitch: long,
            }],
        })
        .unwrap();
        assert_eq!(
            state.lock().unwrap().pitches["S-fx-001"].len(),
            MAX_PITCH_CHARS
        );
    }
}
