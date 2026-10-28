//! Agent definitions: system prompts + tools. OWNER: core agent.

use swarms_rs::agent::SwarmsAgent;
use swarms_rs::llm::provider::openai::OpenAI;

use crate::domain::{CandidateCity, TripRequest};
use crate::state::SharedState;
use crate::websearch::WebSearch;

/// Turns the free-text region into up to `max_cities` candidate cities.
/// Reports them with the `submit_candidates` tool.
pub fn region_planner(llm: &OpenAI, state: SharedState, max_cities: usize) -> SwarmsAgent<OpenAI> {
    let _ = (llm, state, max_cities);
    todo!("core agent")
}

/// Checks the web for a New Year's Eve fireworks show in one city.
/// Tools: `web_search`, `read_page`, `submit_fireworks`.
pub fn fireworks_researcher(
    llm: &OpenAI,
    city: &CandidateCity,
    request: &TripRequest,
    web: WebSearch,
    state: SharedState,
) -> SwarmsAgent<OpenAI> {
    let _ = (llm, city, request, web, state);
    todo!("core agent")
}

/// Picks up to `per_city` listings per city with a reason. Tool: `submit_picks`.
pub fn stay_curator(llm: &OpenAI, state: SharedState, per_city: usize) -> SwarmsAgent<OpenAI> {
    let _ = (llm, state, per_city);
    todo!("core agent")
}

/// Writes a short paragraph per city from the curator's summary and the
/// fireworks evidence. Tools: `get_city_facts`, `submit_city_notes`.
pub fn travel_writer(llm: &OpenAI, state: SharedState) -> SwarmsAgent<OpenAI> {
    let _ = (llm, state);
    todo!("core agent")
}
