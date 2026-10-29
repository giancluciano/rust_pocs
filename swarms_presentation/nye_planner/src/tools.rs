//! swarms-rs tools. Agents return results by calling these tools, which write
//! into `SharedState`; the code never parses an agent's free-text reply.
//!
//! Tool errors are plain sentences: swarms-rs hands them to the model as the
//! tool result, so each one says what was wrong and how to retry.

mod curation;
mod planning;
mod research;

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use swarms_rs::llm::request::ToolDefinition;

pub use curation::{
    CityFactsArgs, CityNotesArgs, GetCityFactsTool, PickArg, SubmitCityNotesTool,
    SubmitPicksArgs, SubmitPicksTool,
};
pub use planning::{CityArg, SubmitCandidatesArgs, SubmitCandidatesTool};
pub use research::{
    ReadPageArgs, ReadPageTool, SeenUrls, SubmitFireworksArgs, SubmitFireworksTool,
    WebSearchArgs, WebSearchTool,
};

use crate::websearch::WebSearchError;

/// Appended to the result of a tool call that finishes an agent's job. The
/// agents list it as a stop word, so the loop ends right after the final
/// submit instead of running until `max_loops`.
pub const DONE_MARKER: &str = "<<DONE>>";

#[derive(Debug, thiserror::Error)]
pub enum ToolFailure {
    /// The arguments were wrong; the message says how to fix them.
    #[error("{0}")]
    Invalid(String),
    #[error("the web request failed ({0}). Try another query or another page.")]
    Web(#[from] WebSearchError),
    /// Search is blocked or out of budget for the rest of the run.
    #[error(
        "web search is unavailable for the rest of this run ({0}). Do not search again: \
         call submit_fireworks now with confirmed=false and summary \"Web search unavailable\"."
    )]
    SearchUnavailable(WebSearchError),
}

/// Tool definition with a flat JSON schema: nested types are inlined instead of
/// `$ref`-ed and the `$schema` key is dropped, which function-calling APIs
/// handle more reliably.
pub(crate) fn definition<T: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    let schema = SchemaSettings::draft07()
        .with(|s| s.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<T>();
    let mut parameters = serde_json::to_value(schema).unwrap_or_default();
    if let Some(object) = parameters.as_object_mut() {
        object.remove("$schema");
        object.remove("title");
    }
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
    }
}

/// At most `max` characters, with an ellipsis when something was cut.
pub(crate) fn truncate_chars(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// `None` for missing, null or blank strings.
pub(crate) fn non_blank(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_text() {
        assert_eq!(truncate_chars("  hello ", 10), "hello");
    }

    #[test]
    fn truncate_cuts_on_chars_not_bytes() {
        let out = truncate_chars("ççççççççç", 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn non_blank_filters_empty() {
        assert_eq!(non_blank(Some("  ".into())), None);
        assert_eq!(non_blank(Some(" a ".into())), Some("a".into()));
        assert_eq!(non_blank(None), None);
    }

    #[test]
    fn schemas_are_flat() {
        let def = definition::<SubmitCandidatesArgs>("submit_candidates", "test");
        let text = def.parameters.to_string();
        assert!(!text.contains("$ref"), "{text}");
        assert!(!text.contains("$schema"), "{text}");
        assert!(text.contains("\"cities\""), "{text}");
    }
}
