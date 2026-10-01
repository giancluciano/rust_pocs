//! Orchestration:
//!   [Single-Place Searcher ‖ Split Searcher] -> enrich (code) -> Vetting Agent
//!   -> build + score options (code) -> Presenter Agent -> report
//!
//! Agents hand data over through tools into `SharedState`. When an agent fails
//! or skips its tool, the pipeline degrades gracefully instead of aborting.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use swarms_rs::llm::provider::openai::OpenAI;
use swarms_rs::structs::agent::Agent;

use crate::agents;
use crate::domain::Destination;
use crate::enrich::EnrichedListing;
use crate::options::{GroupOption, OptionKind, pair_options, single_options};
use crate::scoring::{Weights, score_options};
use crate::state::{ResearchContext, SharedState, snapshot};
use crate::tools::{SearchArgs, SearchListingsTool, SubmitPitchesTool, SubmitReviewsTool};

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub max_pair_distance_m: f64,
    pub top: usize,
    pub weights: Weights,
}

#[derive(Debug, Clone)]
pub struct RankedOption {
    pub rank: usize,
    pub option: GroupOption,
    pub score: f64,
    pub pitch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub destination: Destination,
    pub listings: Vec<EnrichedListing>,
    pub options: Vec<RankedOption>,
}

pub async fn run(ctx: Arc<ResearchContext>, config: &RunConfig, llm: &OpenAI) -> Result<Report> {
    let state = SharedState::default();

    search_phase(&ctx, &state, llm).await?;
    let found: Vec<EnrichedListing> = snapshot(&state).found.into_values().collect();
    tracing::info!(count = found.len(), "search phase done");

    let listings = vetting_phase(&found, &state, llm).await;
    let options = rank_options(&listings, ctx.group_size, config);
    if options.is_empty() {
        tracing::warn!("no single place or nearby pair fits the group");
    }
    let options = presenter_phase(options, ctx.group_size, &state, llm).await;

    Ok(Report {
        destination: ctx.provider.destination().clone(),
        listings,
        options,
    })
}

const AGENT_ATTEMPTS: u32 = 2;

/// swarms-rs 0.2.1 panics (instead of returning an error) when the model sends
/// malformed tool-call JSON. Each attempt runs in its own task so that panic
/// becomes an error we can retry or fall back from.
async fn run_agent(agent: &dyn Agent, task: &str) -> Result<String, String> {
    let name = agent.name();
    let mut last_error = String::new();
    for attempt in 1..=AGENT_ATTEMPTS {
        let agent = agent.clone_box();
        let task = task.to_string();
        let outcome = tokio::spawn(async move { agent.run(task).await }).await;
        last_error = match outcome {
            Ok(Ok(output)) => return Ok(output),
            Ok(Err(e)) => e.to_string(),
            Err(join_error) => format!("agent panicked: {join_error}"),
        };
        tracing::warn!(agent = %name, attempt, error = %last_error, "agent attempt failed");
    }
    Err(last_error)
}

async fn search_phase(ctx: &Arc<ResearchContext>, state: &SharedState, llm: &OpenAI) -> Result<()> {
    let task = format!(
        "City: {}. Group size: {} people. Check-in {} / check-out {} ({} nights).",
        ctx.provider.destination().city,
        ctx.group_size,
        ctx.stay.checkin(),
        ctx.stay.checkout(),
        ctx.stay.nights(),
    );
    let single = agents::single_searcher(llm, SearchListingsTool::new(ctx.clone(), state.clone()));
    let split = agents::split_searcher(llm, SearchListingsTool::new(ctx.clone(), state.clone()));

    tracing::info!("search phase: running single + split searchers concurrently");
    let (single_res, split_res) = tokio::join!(run_agent(&single, &task), run_agent(&split, &task));
    for (name, res) in [
        ("single searcher", single_res),
        ("split searcher", split_res),
    ] {
        match res {
            Ok(summary) => tracing::debug!(agent = name, %summary, "agent finished"),
            Err(e) => tracing::warn!(agent = name, error = %e, "agent failed"),
        }
    }

    if snapshot(state).found.is_empty() {
        tracing::warn!("searchers found nothing; falling back to a broad search");
        SearchListingsTool::new(ctx.clone(), state.clone())
            .search(SearchArgs {
                min_guests: 2,
                max_guests: None,
                min_bedrooms: None,
                max_price_per_night: None,
            })
            .await?;
    }
    Ok(())
}

#[derive(Serialize)]
struct VettingInput<'a> {
    id: &'a str,
    name: &'a str,
    max_guests: u32,
    bedrooms: u32,
    beds: u32,
    bathrooms: f32,
    amenities: &'a [String],
    description: &'a str,
}

async fn vetting_phase(
    found: &[EnrichedListing],
    state: &SharedState,
    llm: &OpenAI,
) -> Vec<EnrichedListing> {
    if found.is_empty() {
        return vec![];
    }
    let ids: BTreeSet<String> = found.iter().map(|l| l.id().to_string()).collect();
    let input: Vec<VettingInput> = found
        .iter()
        .map(|e| VettingInput {
            id: &e.listing.id,
            name: &e.listing.name,
            max_guests: e.listing.max_guests,
            bedrooms: e.listing.bedrooms,
            beds: e.listing.beds,
            bathrooms: e.listing.bathrooms,
            amenities: &e.listing.amenities,
            description: &e.listing.description,
        })
        .collect();
    let task = format!(
        "Review these listings:\n{}",
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );

    let vetter = agents::vetter(llm, SubmitReviewsTool::new(Arc::new(ids), state.clone()));
    tracing::info!(listings = found.len(), "vetting phase");
    if let Err(e) = run_agent(&vetter, &task).await {
        tracing::warn!(error = %e, "vetting agent failed; using host-declared capacity");
    }

    let reviews = snapshot(state).reviews;
    tracing::info!(
        reviewed = reviews.len(),
        total = found.len(),
        "vetting phase done"
    );
    found
        .iter()
        .map(|l| {
            reviews
                .get(l.id())
                .map_or_else(|| l.clone(), |r| l.with_review(r))
        })
        .collect()
}

/// A house may appear in at most this many options, so the vote has real
/// alternatives instead of one great house paired ten different ways.
const MAX_OPTIONS_PER_HOUSE: usize = 2;

/// Keeps the best-scored options (input must be sorted) while no house
/// exceeds `max_per_house` appearances.
fn diversify(sorted: Vec<(GroupOption, f64)>, max_per_house: usize) -> Vec<(GroupOption, f64)> {
    let mut uses: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    sorted
        .into_iter()
        .filter(|(option, _)| {
            let fits = option
                .listings
                .iter()
                .all(|l| uses.get(l.id()).copied().unwrap_or(0) < max_per_house);
            if fits {
                for l in &option.listings {
                    *uses.entry(l.id().to_string()).or_default() += 1;
                }
            }
            fits
        })
        .collect()
}

/// Deterministic: builds singles + nearby pairs, scores, keeps the top N.
pub fn rank_options(
    listings: &[EnrichedListing],
    group_size: u32,
    config: &RunConfig,
) -> Vec<RankedOption> {
    let options: Vec<GroupOption> = single_options(listings, group_size)
        .into_iter()
        .chain(pair_options(
            listings,
            group_size,
            config.max_pair_distance_m,
        ))
        .collect();
    let scores = score_options(&options, group_size, &config.weights);

    let mut scored: Vec<(GroupOption, f64)> = options.into_iter().zip(scores).collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
    diversify(scored, MAX_OPTIONS_PER_HOUSE)
        .into_iter()
        .take(config.top)
        .enumerate()
        .map(|(i, (option, score))| RankedOption {
            rank: i + 1,
            option,
            score,
            pitch: None,
        })
        .collect()
}

#[derive(Serialize)]
struct HouseInput<'a> {
    name: &'a str,
    sleeps: u32,
    pool: bool,
    beach_km: Option<f64>,
    center_km: f64,
}

impl<'a> From<&'a EnrichedListing> for HouseInput<'a> {
    fn from(e: &'a EnrichedListing) -> Self {
        let round1 = |x: f64| (x * 10.0).round() / 10.0;
        Self {
            name: &e.listing.name,
            sleeps: e.realistic_capacity,
            pool: e.has_pool,
            beach_km: e.distance_beach_km.map(round1),
            center_km: round1(e.distance_center_km),
        }
    }
}

#[derive(Serialize)]
struct PresenterInput<'a> {
    option_id: &'a str,
    kind: OptionKind,
    main_house: HouseInput<'a>,
    sleeping_house: Option<HouseInput<'a>>,
    walk_between_houses_m: Option<f64>,
    price_per_person_per_night_brl: f64,
    avg_rating: Option<f64>,
    red_flags: Vec<String>,
}

async fn presenter_phase(
    options: Vec<RankedOption>,
    group_size: u32,
    state: &SharedState,
    llm: &OpenAI,
) -> Vec<RankedOption> {
    if options.is_empty() {
        return options;
    }
    let ids: BTreeSet<String> = options.iter().map(|o| o.option.id.clone()).collect();
    let input: Vec<PresenterInput> = options
        .iter()
        .map(|r| {
            let o = &r.option;
            PresenterInput {
                option_id: &o.id,
                kind: o.kind,
                main_house: o.main_house().into(),
                sleeping_house: o.sleeping_house().map(HouseInput::from),
                walk_between_houses_m: o.pair_distance_m.map(f64::round),
                price_per_person_per_night_brl: o.price_per_person_per_night(group_size).round(),
                avg_rating: o.avg_rating().map(|r| (r * 100.0).round() / 100.0),
                red_flags: o.red_flags(),
            }
        })
        .collect();
    let task = format!(
        "Group of {group_size} people. Options:\n{}",
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );

    let presenter = agents::presenter(llm, SubmitPitchesTool::new(Arc::new(ids), state.clone()));
    tracing::info!(options = options.len(), "presenter phase");
    if let Err(e) = run_agent(&presenter, &task).await {
        tracing::warn!(error = %e, "presenter agent failed; options will have no pitch");
    }

    let pitches = snapshot(state).pitches;
    options
        .into_iter()
        .map(|r| RankedOption {
            pitch: pitches.get(&r.option.id).cloned(),
            ..r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::Stay;
    use crate::domain::test_support::date;
    use crate::provider::fixture::FixtureProvider;
    use crate::provider::{ListingProvider, SearchQuery};

    async fn fixture_listings() -> Vec<EnrichedListing> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let provider = FixtureProvider::load(&dir, "florianopolis").unwrap();
        let query = SearchQuery {
            stay: Stay::new(date("2027-01-10"), date("2027-01-17")).unwrap(),
            min_guests: 1,
            max_guests: None,
            min_bedrooms: None,
            max_price_per_night: None,
        };
        provider
            .search(&query)
            .await
            .unwrap()
            .into_iter()
            .map(|l| EnrichedListing::new(l, provider.destination()))
            .collect()
    }

    fn config() -> RunConfig {
        RunConfig {
            max_pair_distance_m: 500.0,
            top: 50,
            weights: Weights::default(),
        }
    }

    #[tokio::test]
    async fn fixture_yields_singles_and_nearby_pairs_ranked() {
        let ranked = rank_options(&fixture_listings().await, 20, &config());
        let ids: Vec<&str> = ranked.iter().map(|r| r.option.id.as_str()).collect();

        assert!(ids.contains(&"S-fx-001"));
        assert!(ids.contains(&"P-fx-005+fx-006"));
        assert!(ids.contains(&"P-fx-015+fx-016"));
        // fx-014 is booked on these dates
        assert!(ids.iter().all(|id| !id.contains("fx-014")));
        // ranks are 1..n and scores descending
        assert!(ranked.iter().enumerate().all(|(i, r)| r.rank == i + 1));
        assert!(ranked.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[tokio::test]
    async fn vetting_can_remove_a_single_option() {
        let listings: Vec<EnrichedListing> = fixture_listings()
            .await
            .into_iter()
            .map(|l| {
                if l.id() == "fx-002" {
                    EnrichedListing {
                        realistic_capacity: 16,
                        ..l
                    }
                } else {
                    l
                }
            })
            .collect();
        let ranked = rank_options(&listings, 20, &config());
        assert!(ranked.iter().all(|r| r.option.id != "S-fx-002"));
    }

    #[tokio::test]
    async fn no_house_appears_in_more_than_two_options() {
        let ranked = rank_options(&fixture_listings().await, 20, &config());
        let mut counts = std::collections::HashMap::new();
        for r in &ranked {
            for l in &r.option.listings {
                *counts.entry(l.id().to_string()).or_insert(0) += 1;
            }
        }
        assert!(
            counts.values().all(|&n| n <= MAX_OPTIONS_PER_HOUSE),
            "{counts:?}"
        );
    }

    #[tokio::test]
    async fn top_limits_results() {
        let ranked = rank_options(
            &fixture_listings().await,
            20,
            &RunConfig { top: 3, ..config() },
        );
        assert_eq!(ranked.len(), 3);
    }
}
