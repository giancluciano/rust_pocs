//! New Year's Eve travel planner: a swarms-rs POC.
//!
//! Give it a region and a group size; it finds cities in that region that
//! announced a New Year's Eve fireworks show, then Airbnb stays for the group.
//!
//!   1. Region planner      one agent          region -> candidate cities
//!   2. Fireworks research  ConcurrentWorkflow one agent per city, web search
//!   3. Airbnb search       plain code         confirmed cities -> listings
//!   4. Curate and write    SequentialWorkflow curator -> travel writer
//!   5. Report              plain code         markdown file
//!
//! Agents hand results back through tools that write into `SharedState`, so
//! the code never parses an agent's free-text reply.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::Parser;
use swarms_rs::llm::provider::openai::OpenAI;
use swarms_rs::structs::agent::Agent;
use swarms_rs::structs::concurrent_workflow::ConcurrentWorkflow;
use swarms_rs::structs::sequential_workflow::SequentialWorkflow;

use nye_planner::agents;
use nye_planner::airbnb::{AirbnbConfig, AirbnbSearch};
use nye_planner::domain::{Stay, TripRequest};
use nye_planner::report;
use nye_planner::state::SharedState;
use nye_planner::websearch::WebSearch;

#[derive(Parser)]
#[command(about = "Find New Year's Eve fireworks cities and Airbnb stays for a group")]
struct Args {
    /// Where to look, e.g. "the coastline of Santa Catarina"
    #[arg(long)]
    region: String,
    /// How many people are coming
    #[arg(long)]
    people: u32,
    /// Check-in date, e.g. 2026-12-30
    #[arg(long)]
    checkin: NaiveDate,
    /// Check-out date, e.g. 2027-01-02
    #[arg(long)]
    checkout: NaiveDate,
    /// Cities the planner proposes for research
    #[arg(long, default_value_t = 15)]
    max_candidates: usize,
    /// Cities with a confirmed show kept in the report
    #[arg(long, default_value_t = 10)]
    max_cities: usize,
    /// Stays recommended per city
    #[arg(long, default_value_t = 10)]
    per_city: usize,
    #[arg(long, default_value = "output")]
    output_dir: PathBuf,
    #[arg(long, default_value = "cache")]
    cache_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    init_tracing();
    let args = Args::parse();

    let stay = Stay::new(args.checkin, args.checkout).map_err(anyhow::Error::msg)?;
    if stay.new_years_eve().is_none() {
        bail!("the stay must include the night of December 31");
    }
    let request = TripRequest { region: args.region.clone(), people: args.people, stay };

    let llm = deepseek()?;
    let state = SharedState::default();
    let web = WebSearch::new(&args.cache_dir)?;
    let airbnb = AirbnbSearch::new(
        AirbnbConfig {
            max_pages: 2,
            max_listing_details: (args.per_city * 2) as u32,
            min_interval: Duration::from_secs(1),
        },
        &args.cache_dir,
    )?;

    // 1. One agent turns the free-text region into candidate cities.
    let planner = agents::region_planner(&llm, state.clone(), args.max_candidates);
    planner.run(format!("Region: {}", request.region)).await?;
    let candidates = state.candidates();
    if candidates.is_empty() {
        bail!("the planner found no cities for '{}'", request.region);
    }
    tracing::info!(count = candidates.len(), "candidate cities");

    // 2. One researcher per city, all running in parallel.
    let researchers: Vec<Box<dyn Agent>> = candidates
        .iter()
        .map(|city| {
            Box::new(agents::fireworks_researcher(&llm, city, &request, web.clone(), state.clone()))
                as Box<dyn Agent>
        })
        .collect();
    ConcurrentWorkflow::builder()
        .name("Fireworks research")
        .agents(researchers)
        .build()
        .run("Find out whether your city announced a New Year's Eve fireworks show.")
        .await?;
    if !web.search_available() {
        tracing::warn!("web search was blocked during research: unconfirmed cities may still have a show");
    }
    let confirmed = state.confirmed(args.max_cities);
    tracing::info!(count = confirmed.len(), "cities with a confirmed show");

    if !confirmed.is_empty() {
        // 3. Plain code: Airbnb is scraped politely, one city at a time.
        for (city, _) in &confirmed {
            match airbnb.search_city(&city.full_name(), &request.stay, request.people).await {
                Ok(listings) => state.set_listings(&city.name, listings),
                Err(e) => {
                    tracing::warn!(city = %city.name, error = %e, "Airbnb search failed");
                    state.set_listings(&city.name, Vec::new());
                }
            }
        }

        // 4. The curator picks the stays, then the writer describes each city.
        //    If this fails, the report falls back to the cheapest stays.
        let curated = SequentialWorkflow::builder()
            .name("Curate and write")
            .metadata_output_dir(args.output_dir.join("metadata").to_string_lossy())
            .agents(vec![
                Box::new(agents::stay_curator(&llm, state.clone(), args.per_city)),
                Box::new(agents::travel_writer(&llm, state.clone())),
            ])
            .build()
            .run(state.curation_brief(&request, args.per_city))
            .await;
        if let Err(e) = curated {
            tracing::warn!(error = %e, "curate and write failed");
        }
    }

    // 5. Markdown report.
    let markdown = report::render_markdown(&request, &state.plans(), &state.unconfirmed());
    let path = report::write_report(&args.output_dir, &request, &markdown)?;
    println!("Report written to {}", path.display());
    Ok(())
}

/// DeepSeek through its OpenAI-compatible API.
fn deepseek() -> Result<OpenAI> {
    let base_url = std::env::var("DEEPSEEK_BASE_URL").context("DEEPSEEK_BASE_URL is not set")?;
    let api_key = std::env::var("DEEPSEEK_API_KEY").context("DEEPSEEK_API_KEY is not set")?;
    let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
    Ok(OpenAI::from_url(base_url, api_key).set_model(model))
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nye_planner=info,warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
