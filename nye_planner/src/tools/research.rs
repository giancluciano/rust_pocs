//! Tools of the fireworks researcher.

use schemars::JsonSchema;
use serde::Deserialize;
use swarms_rs::llm::request::ToolDefinition;
use swarms_rs::structs::tool::Tool;

use super::{DONE_MARKER, ToolFailure, definition, non_blank, truncate_chars};
use crate::domain::{CandidateCity, FireworksEvidence};
use crate::state::SharedState;
use crate::websearch::{SearchHit, WebSearch};

const DEFAULT_RESULTS: usize = 5;
const MAX_RESULTS: usize = 8;
const MAX_PAGE_CHARS: usize = 6000;
const MAX_SUMMARY_CHARS: usize = 500;

// ---------------------------------------------------------------- web_search

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSearchArgs {
    /// The search query, written in the local language of the city
    pub query: String,
    /// How many results to return (default 5, at most 8)
    #[serde(default)]
    pub max_results: Option<u32>,
}

#[derive(Clone)]
pub struct WebSearchTool {
    web: WebSearch,
}

impl WebSearchTool {
    pub fn new(web: WebSearch) -> Self {
        Self { web }
    }
}

fn result_limit(requested: Option<u32>) -> usize {
    requested.map_or(DEFAULT_RESULTS, |n| n as usize).clamp(1, MAX_RESULTS)
}

impl Tool for WebSearchTool {
    type Error = ToolFailure;
    type Args = WebSearchArgs;
    type Output = Vec<SearchHit>;
    const NAME: &'static str = "web_search";

    fn definition(&self) -> ToolDefinition {
        definition::<WebSearchArgs>(
            Self::NAME,
            "Search the web. Returns the title, url and snippet of the top results.",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let query = args.query.trim();
        if query.is_empty() {
            return Err(ToolFailure::Invalid("The query is empty. Send a search query.".into()));
        }
        tracing::info!(query, "web_search called");
        Ok(self.web.search(query, result_limit(args.max_results)).await?)
    }
}

// ----------------------------------------------------------------- read_page

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadPageArgs {
    /// Full http(s) URL of a page taken from the search results
    pub url: String,
}

#[derive(Clone)]
pub struct ReadPageTool {
    web: WebSearch,
}

impl ReadPageTool {
    pub fn new(web: WebSearch) -> Self {
        Self { web }
    }
}

fn is_http_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

impl Tool for ReadPageTool {
    type Error = ToolFailure;
    type Args = ReadPageArgs;
    type Output = String;
    const NAME: &'static str = "read_page";

    fn definition(&self) -> ToolDefinition {
        definition::<ReadPageArgs>(
            Self::NAME,
            "Read the visible text of a web page (the first part of it).",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let url = args.url.trim();
        if !is_http_url(url) {
            return Err(ToolFailure::Invalid(format!(
                "'{url}' is not a valid URL. Use a full http(s) URL from the search results."
            )));
        }
        tracing::info!(url, "read_page called");
        Ok(self.web.fetch_text(url, MAX_PAGE_CHARS).await?)
    }
}

// ---------------------------------------------------------- submit_fireworks

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubmitFireworksArgs {
    /// True only if a source explicitly announces fireworks for THIS New Year's Eve
    pub confirmed: bool,
    /// One or two sentences: where and when the show is, and who announced it.
    /// If not confirmed, say what you found (for example only past years).
    pub summary: String,
    /// URL of the page that announces the show. Required when confirmed is true.
    #[serde(default)]
    pub source_url: Option<String>,
    /// Title of that page
    #[serde(default)]
    pub source_title: Option<String>,
}

/// Records the verdict for ONE city, fixed when the tool is built, so a
/// researcher cannot report on any other city.
#[derive(Clone)]
pub struct SubmitFireworksTool {
    state: SharedState,
    city: CandidateCity,
}

impl SubmitFireworksTool {
    pub fn new(state: SharedState, city: CandidateCity) -> Self {
        Self { state, city }
    }

    fn submit(&self, args: SubmitFireworksArgs) -> Result<String, ToolFailure> {
        let evidence = self.validate(args)?;
        let verdict = if evidence.confirmed { "confirmed" } else { "not confirmed" };
        let message = format!("Saved: fireworks {verdict} for {}. {DONE_MARKER}", self.city.name);
        self.state.record_fireworks(evidence);
        Ok(message)
    }

    fn validate(&self, args: SubmitFireworksArgs) -> Result<FireworksEvidence, ToolFailure> {
        let summary = truncate_chars(&args.summary, MAX_SUMMARY_CHARS);
        if summary.is_empty() {
            return Err(ToolFailure::Invalid(
                "The summary is empty. Write one or two sentences about what you found.".into(),
            ));
        }
        let source_url = non_blank(args.source_url);
        if source_url.as_deref().is_some_and(|u| !is_http_url(u)) {
            return Err(ToolFailure::Invalid(
                "source_url must be a full http(s) URL from the pages you read.".into(),
            ));
        }
        if args.confirmed && source_url.is_none() {
            return Err(ToolFailure::Invalid(
                "confirmed=true needs a source_url of the page that announces the show. \
                 Send the URL, or set confirmed=false if you have no source."
                    .into(),
            ));
        }
        Ok(FireworksEvidence {
            city: self.city.name.clone(),
            confirmed: args.confirmed,
            summary,
            source_url,
            source_title: non_blank(args.source_title),
        })
    }
}

impl Tool for SubmitFireworksTool {
    type Error = ToolFailure;
    type Args = SubmitFireworksArgs;
    type Output = String;
    const NAME: &'static str = "submit_fireworks";

    fn definition(&self) -> ToolDefinition {
        definition::<SubmitFireworksArgs>(
            Self::NAME,
            "Submit your verdict on the fireworks show of your city. Call it exactly once.",
        )
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.submit(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> (SubmitFireworksTool, SharedState) {
        let state = SharedState::default();
        let city = CandidateCity {
            name: "Bombinhas".into(),
            state: "Santa Catarina".into(),
            country: "Brazil".into(),
            why: String::new(),
        };
        state.set_candidates(vec![city.clone()]);
        (SubmitFireworksTool::new(state.clone(), city), state)
    }

    fn args(confirmed: bool, url: Option<&str>) -> SubmitFireworksArgs {
        SubmitFireworksArgs {
            confirmed,
            summary: "Show at Praia de Bombas at midnight.".into(),
            source_url: url.map(String::from),
            source_title: Some("Prefeitura".into()),
        }
    }

    #[tokio::test]
    async fn confirmed_without_source_is_rejected() {
        let (tool, state) = tool();
        let err = tool.call(args(true, None)).await.unwrap_err().to_string();
        assert!(err.contains("source_url"));
        let err = tool.call(args(true, Some("  "))).await.unwrap_err().to_string();
        assert!(err.contains("source_url"));
        assert!(state.fireworks_for("Bombinhas").is_none());
    }

    #[tokio::test]
    async fn non_http_source_is_rejected() {
        let (tool, _) = tool();
        assert!(tool.call(args(true, Some("prefeitura"))).await.is_err());
    }

    #[tokio::test]
    async fn confirmed_with_source_is_stored_for_the_bound_city() {
        let (tool, state) = tool();
        let message = tool.call(args(true, Some("https://example.com/fogos"))).await.unwrap();
        assert!(message.contains(DONE_MARKER));
        let evidence = state.fireworks_for("Bombinhas").unwrap();
        assert!(evidence.confirmed);
        assert_eq!(evidence.city, "Bombinhas");
        assert_eq!(evidence.source_url.as_deref(), Some("https://example.com/fogos"));
        assert_eq!(state.confirmed(5).len(), 1);
    }

    #[tokio::test]
    async fn unconfirmed_needs_no_source() {
        let (tool, state) = tool();
        tool.call(args(false, None)).await.unwrap();
        assert!(!state.fireworks_for("Bombinhas").unwrap().confirmed);
    }

    #[tokio::test]
    async fn empty_summary_is_rejected() {
        let (tool, _) = tool();
        let mut bad = args(false, None);
        bad.summary = "  ".into();
        assert!(tool.call(bad).await.is_err());
    }

    #[test]
    fn result_limit_is_clamped() {
        assert_eq!(result_limit(None), 5);
        assert_eq!(result_limit(Some(0)), 1);
        assert_eq!(result_limit(Some(50)), 8);
        assert_eq!(result_limit(Some(3)), 3);
    }

    #[test]
    fn only_http_urls_pass() {
        assert!(is_http_url("https://a.com"));
        assert!(is_http_url("HTTP://a.com"));
        assert!(!is_http_url("a.com"));
        assert!(!is_http_url("file:///etc/passwd"));
    }
}
