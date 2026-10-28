//! DuckDuckGo web search (HTML endpoint, no API key). OWNER: web-search agent.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WebSearchError {
    #[error(transparent)]
    Http(#[from] crate::http::HttpError),
    #[error("could not understand the results page: {0}")]
    Parse(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Cheap to clone; clones share one rate limiter and cache.
#[derive(Clone)]
pub struct WebSearch {
    _private: (),
}

impl WebSearch {
    pub fn new(cache_dir: &Path) -> Result<Self, WebSearchError> {
        let _ = cache_dir;
        todo!("web-search agent")
    }

    /// Top `max_results` organic results for `query`.
    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchHit>, WebSearchError> {
        let _ = (query, max_results);
        todo!("web-search agent")
    }

    /// Visible text of a web page, truncated to `max_chars`.
    pub async fn fetch_text(&self, url: &str, max_chars: usize) -> Result<String, WebSearchError> {
        let _ = (url, max_chars);
        todo!("web-search agent")
    }
}
