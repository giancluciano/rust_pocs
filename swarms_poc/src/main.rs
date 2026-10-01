use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::{Parser, ValueEnum};
use swarms_rs::llm::provider::openai::OpenAI;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use swarms_poc::domain::Stay;
use swarms_poc::export::write_report;
use swarms_poc::pipeline::{self, RunConfig};
use swarms_poc::provider::ListingProvider;
use swarms_poc::provider::airbnb::{AirbnbConfig, AirbnbProvider};
use swarms_poc::provider::fixture::FixtureProvider;
use swarms_poc::scoring::Weights;
use swarms_poc::state::ResearchContext;

const DEFAULT_MODEL: &str = "deepseek-chat";

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Source {
    /// Offline sample data from --fixtures-dir
    Fixture,
    /// Scrape airbnb.com (rate limited, cached)
    Airbnb,
}

/// Find rentals for a big group: one place for everyone, or two places close together.
#[derive(Debug, Parser)]
struct Args {
    /// City to search, e.g. "Florianópolis"
    #[arg(long)]
    city: String,
    /// Number of people in the group
    #[arg(long, value_parser = clap::value_parser!(u32).range(2..=50))]
    people: u32,
    /// Check-in date (YYYY-MM-DD)
    #[arg(long)]
    checkin: NaiveDate,
    /// Check-out date (YYYY-MM-DD)
    #[arg(long)]
    checkout: NaiveDate,
    /// Max straight-line distance between the two places of a pair, in meters
    #[arg(long, default_value_t = 500.0)]
    max_pair_distance_m: f64,
    /// Number of options to put up for vote
    #[arg(long, default_value_t = 10)]
    top: usize,
    /// Where listings come from
    #[arg(long, value_enum, default_value_t = Source::Fixture)]
    source: Source,
    #[arg(long, default_value = "fixtures")]
    fixtures_dir: PathBuf,
    /// [airbnb] Result pages fetched per search (18 listings each)
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=5))]
    max_pages: u32,
    /// [airbnb] Listing pages fetched per run (capacity, amenities, description)
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=200))]
    max_listing_details: u32,
    /// [airbnb] Milliseconds between requests; never below 1000 (1 req/s)
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u64).range(1000..))]
    request_interval_ms: u64,
    /// [airbnb] HTTP response cache
    #[arg(long, default_value = "cache")]
    cache_dir: PathBuf,
    #[arg(long, default_value = "output")]
    output_dir: PathBuf,
}

fn deepseek_client() -> Result<OpenAI> {
    let base_url = env::var("DEEPSEEK_BASE_URL").context("DEEPSEEK_BASE_URL is not set")?;
    let api_key = env::var("DEEPSEEK_API_KEY").context("DEEPSEEK_API_KEY is not set")?;
    let model = env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    Ok(OpenAI::from_url(base_url, api_key).set_model(model))
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "swarms_poc=info,warn".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_line_number(true)
                .with_file(true),
        )
        .init();

    let args = Args::parse();
    let stay = Stay::new(args.checkin, args.checkout).map_err(anyhow::Error::msg)?;
    let llm = deepseek_client()?;
    let provider: Arc<dyn ListingProvider> = match args.source {
        Source::Fixture => Arc::new(FixtureProvider::load(&args.fixtures_dir, &args.city)?),
        Source::Airbnb => {
            let config = AirbnbConfig {
                max_pages: args.max_pages,
                max_listing_details: args.max_listing_details,
                min_interval: Duration::from_millis(args.request_interval_ms),
            };
            Arc::new(AirbnbProvider::connect(&args.city, config, &args.cache_dir).await?)
        }
    };

    let ctx = Arc::new(ResearchContext {
        provider,
        stay,
        group_size: args.people,
    });
    let config = RunConfig {
        max_pair_distance_m: args.max_pair_distance_m,
        top: args.top,
        weights: Weights::default(),
    };

    let report = pipeline::run(ctx, &config, &llm).await?;
    let (options_csv, listings_csv) = write_report(&args.output_dir, &report, args.people, &stay)?;

    println!(
        "\n{} options for {} people in {} ({} nights):\n",
        report.options.len(),
        args.people,
        report.destination.city,
        stay.nights()
    );
    for r in &report.options {
        let o = &r.option;
        let main = o.main_house();
        let beach = main
            .distance_beach_km
            .map_or("n/a".into(), |d| format!("{d:.1} km"));
        println!(
            "#{:<2} score {:>5.1} | R$ {:>4.0}/person/night | main house: {} (sleeps {}, pool {}, beach {beach})",
            r.rank,
            r.score,
            o.price_per_person_per_night(args.people),
            main.listing.name,
            main.realistic_capacity,
            if main.has_pool { "yes" } else { "no" },
        );
        if let (Some(s), Some(m)) = (o.sleeping_house(), o.pair_distance_m) {
            println!(
                "     + sleeping house {m:.0} m away: {} (sleeps {})",
                s.listing.name, s.realistic_capacity
            );
        }
        if let Some(pitch) = &r.pitch {
            println!("     {pitch}");
        }
    }
    println!(
        "\nWrote {} and {}",
        options_csv.display(),
        listings_csv.display()
    );
    Ok(())
}
