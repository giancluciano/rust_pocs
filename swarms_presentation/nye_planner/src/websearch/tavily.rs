//! Tavily search API: request body and JSON response parsing.

use serde::{Deserialize, Serialize};

use super::text::collapse_whitespace;
use super::{SearchHit, WebSearchError};

/// The API returns at most 20 results per request.
const MAX_RESULTS: usize = 20;

#[derive(Serialize)]
struct Request<'a> {
    query: &'a str,
    max_results: usize,
    /// "basic" costs one credit per search; "advanced" costs two.
    search_depth: &'static str,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    title: String,
    url: String,
    #[serde(default)]
    content: String,
}

/// JSON body for `query`, with `max_results` capped at the API maximum.
pub fn request_body(query: &str, max_results: usize) -> Result<String, WebSearchError> {
    let request = Request {
        query,
        max_results: max_results.min(MAX_RESULTS),
        search_depth: "basic",
    };
    serde_json::to_string(&request)
        .map_err(|e| WebSearchError::Parse(format!("Tavily request: {e}")))
}

/// Results in API order (most relevant first), at most `max_results`.
pub fn parse_results(body: &str, max_results: usize) -> Result<Vec<SearchHit>, WebSearchError> {
    let response: Response = serde_json::from_str(body)
        .map_err(|e| WebSearchError::Parse(format!("Tavily response: {e}")))?;
    Ok(response
        .results
        .into_iter()
        .filter(|r| r.url.starts_with("http"))
        .map(|r| SearchHit {
            title: r.title.trim().to_string(),
            url: r.url,
            snippet: collapse_whitespace(&r.content),
        })
        .take(max_results)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
        "query": "réveillon bombinhas fogos",
        "answer": null,
        "results": [
            {
                "title": "Réveillon 2027 em Bombinhas terá queima de fogos ",
                "url": "https://www.bombinhas.sc.gov.br/noticias/reveillon",
                "content": "A prefeitura confirmou   a queima de fogos\nna praia.",
                "score": 0.93,
                "raw_content": null
            },
            {
                "title": "Second",
                "url": "https://example.com/second",
                "score": 0.5
            },
            {
                "title": "Not a web link",
                "url": "ftp://example.com/file",
                "content": "skipped"
            }
        ],
        "response_time": 1.2
    }"#;

    #[test]
    fn parses_results_and_collapses_whitespace() {
        let hits = parse_results(BODY, 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].title,
            "Réveillon 2027 em Bombinhas terá queima de fogos"
        );
        assert_eq!(
            hits[0].url,
            "https://www.bombinhas.sc.gov.br/noticias/reveillon"
        );
        assert_eq!(
            hits[0].snippet,
            "A prefeitura confirmou a queima de fogos na praia."
        );
        assert_eq!(hits[1].snippet, "");
    }

    #[test]
    fn respects_max_results() {
        assert_eq!(parse_results(BODY, 1).unwrap().len(), 1);
    }

    #[test]
    fn missing_results_means_no_results() {
        assert!(parse_results(r#"{"query":"x"}"#, 5).unwrap().is_empty());
    }

    #[test]
    fn invalid_json_is_a_parse_error() {
        assert!(matches!(
            parse_results("<html>rate limited</html>", 5),
            Err(WebSearchError::Parse(_))
        ));
    }

    #[test]
    fn request_body_caps_max_results() {
        let body: serde_json::Value =
            serde_json::from_str(&request_body("réveillon fogos", 50).unwrap()).unwrap();
        assert_eq!(body["query"], "réveillon fogos");
        assert_eq!(body["max_results"], 20);
        assert_eq!(body["search_depth"], "basic");
    }
}
