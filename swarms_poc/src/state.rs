//! Shared state the agents write to through their tools. Code reads it after
//! each phase instead of parsing free-form LLM text.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::domain::Stay;
use crate::enrich::{EnrichedListing, ListingReview};
use crate::provider::ListingProvider;

pub struct ResearchContext {
    pub provider: Arc<dyn ListingProvider>,
    pub stay: Stay,
    pub group_size: u32,
}

#[derive(Debug, Default, Clone)]
pub struct ResearchState {
    /// Every listing any searcher found, keyed by id (deduplicated).
    pub found: BTreeMap<String, EnrichedListing>,
    pub reviews: BTreeMap<String, ListingReview>,
    /// Option id -> one-line pitch for the group vote.
    pub pitches: BTreeMap<String, String>,
}

pub type SharedState = Arc<Mutex<ResearchState>>;

/// Clone of the current state; tolerates a poisoned lock since the data is
/// append-only and still usable.
pub fn snapshot(state: &SharedState) -> ResearchState {
    state.lock().unwrap_or_else(|p| p.into_inner()).clone()
}
