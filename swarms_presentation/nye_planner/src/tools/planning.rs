//! Tool of the region planner.

use schemars::JsonSchema;
use serde::Deserialize;
use swarms_rs::llm::request::ToolDefinition;
use swarms_rs::structs::tool::Tool;

use super::{DONE_MARKER, ToolFailure, definition, non_blank, truncate_chars};
use crate::domain::CandidateCity;
use crate::state::SharedState;

const MAX_WHY_CHARS: usize = 200;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CityArg {
    /// City name only, e.g. "Bombinhas"
    pub name: String,
    /// State or province, e.g. "Santa Catarina"
    #[serde(default)]
    pub state: Option<String>,
    /// Country, e.g. "Brazil"
    #[serde(default)]
    pub country: Option<String>,
    /// One short line on why this city is a likely New Year's Eve destination
    #[serde(default)]
    pub why: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubmitCandidatesArgs {
    /// Real cities in the region, best known New Year's Eve destinations first
    pub cities: Vec<CityArg>,
}

/// Records the planner's candidate cities. One call is expected.
#[derive(Clone)]
pub struct SubmitCandidatesTool {
    state: SharedState,
    max_cities: usize,
}

impl SubmitCandidatesTool {
    pub fn new(state: SharedState, max_cities: usize) -> Self {
        Self { state, max_cities }
    }

    fn submit(&self, args: SubmitCandidatesArgs) -> Result<String, ToolFailure> {
        let merged = merge_cities(&self.state.candidates(), args.cities, self.max_cities);
        if merged.is_empty() {
            return Err(ToolFailure::Invalid(
                "No valid cities. Send at least one city with a non-empty name.".into(),
            ));
        }
        let names: Vec<&str> = merged.iter().map(|c| c.name.as_str()).collect();
        let message = format!("Saved {} cities: {}. {DONE_MARKER}", merged.len(), names.join(", "));
        self.state.set_candidates(merged);
        Ok(message)
    }
}

/// Existing cities first, then new ones; case-insensitive dedupe by name, capped at `max`.
fn merge_cities(existing: &[CandidateCity], new: Vec<CityArg>, max: usize) -> Vec<CandidateCity> {
    let incoming = new.into_iter().filter_map(|c| {
        let name = c.name.trim().to_string();
        (!name.is_empty()).then(|| CandidateCity {
            name,
            state: non_blank(c.state).unwrap_or_default(),
            country: non_blank(c.country).unwrap_or_default(),
            why: truncate_chars(&non_blank(c.why).unwrap_or_default(), MAX_WHY_CHARS),
        })
    });
    let mut merged: Vec<CandidateCity> = Vec::new();
    for city in existing.iter().cloned().chain(incoming) {
        let seen = merged.iter().any(|m| m.name.to_lowercase() == city.name.to_lowercase());
        if !seen && merged.len() < max {
            merged.push(city);
        }
    }
    merged
}

impl Tool for SubmitCandidatesTool {
    type Error = ToolFailure;
    type Args = SubmitCandidatesArgs;
    type Output = String;
    const NAME: &'static str = "submit_candidates";

    fn definition(&self) -> ToolDefinition {
        definition::<SubmitCandidatesArgs>(
            Self::NAME,
            "Submit the candidate cities for the region. Call it once with the full list.",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.submit(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(name: &str) -> CityArg {
        CityArg {
            name: name.into(),
            state: Some("Santa Catarina".into()),
            country: Some("Brazil".into()),
            why: Some("Big show".into()),
        }
    }

    #[tokio::test]
    async fn dedupes_case_insensitively_and_stores_in_order() {
        let state = SharedState::default();
        let tool = SubmitCandidatesTool::new(state.clone(), 10);
        let args = SubmitCandidatesArgs {
            cities: vec![arg("Bombinhas"), arg("bombinhas "), arg("Itapema")],
        };
        let message = tool.call(args).await.unwrap();
        let names: Vec<String> = state.candidates().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Bombinhas", "Itapema"]);
        assert!(message.contains(DONE_MARKER));
        assert_eq!(state.candidates()[0].country, "Brazil");
    }

    #[tokio::test]
    async fn caps_at_the_planner_maximum() {
        let state = SharedState::default();
        let tool = SubmitCandidatesTool::new(state.clone(), 2);
        let args = SubmitCandidatesArgs { cities: vec![arg("A"), arg("B"), arg("C")] };
        tool.call(args).await.unwrap();
        assert_eq!(state.candidates().len(), 2);
    }

    #[tokio::test]
    async fn rejects_empty_lists_and_blank_names() {
        let state = SharedState::default();
        let tool = SubmitCandidatesTool::new(state.clone(), 5);
        let empty = SubmitCandidatesArgs { cities: vec![] };
        assert!(tool.call(empty).await.is_err());
        let blank = SubmitCandidatesArgs { cities: vec![arg("  ")] };
        let err = tool.call(blank).await.unwrap_err().to_string();
        assert!(err.contains("non-empty name"));
        assert!(state.candidates().is_empty());
    }

    #[tokio::test]
    async fn second_call_adds_without_duplicates() {
        let state = SharedState::default();
        let tool = SubmitCandidatesTool::new(state.clone(), 5);
        tool.call(SubmitCandidatesArgs { cities: vec![arg("A")] }).await.unwrap();
        tool.call(SubmitCandidatesArgs { cities: vec![arg("a"), arg("B")] }).await.unwrap();
        assert_eq!(state.candidates().len(), 2);
    }

    #[test]
    fn definition_exposes_the_schema() {
        let state = SharedState::default();
        let def = Tool::definition(&SubmitCandidatesTool::new(state, 3));
        assert_eq!(def.name, "submit_candidates");
        assert!(def.parameters.to_string().contains("cities"));
    }
}
