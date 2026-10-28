//! Brave Search API: request URL and JSON response parsing.

use reqwest::Url;
use scraper::Html;
use serde::Deserialize;

use super::text::collapse_whitespace;
use super::{SearchHit, WebSearchError};

/// The API returns at most 20 results per request.
const MAX_COUNT: usize = 20;

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    web: Option<WebResults>,
}

#[derive(Deserialize)]
struct WebResults {
    #[serde(default)]
    results: Vec<WebResult>,
}

#[derive(Deserialize)]
struct WebResult {
    title: String,
    url: String,
    #[serde(default)]
    description: String,
}

/// `base?q=<query>&count=<n>`, with `n` capped at the API maximum.
pub fn search_url(base: &str, query: &str, max_results: usize) -> Result<String, WebSearchError> {
    let count = max_results.min(MAX_COUNT).to_string();
    Url::parse_with_params(base, &[("q", query), ("count", count.as_str())])
        .map(String::from)
        .map_err(|e| WebSearchError::InvalidUrl(format!("{base}: {e}")))
}

/// Web results in API order, at most `max_results`. Titles and descriptions
/// carry `<strong>` highlights and HTML entities, which are reduced to text.
pub fn parse_results(body: &str, max_results: usize) -> Result<Vec<SearchHit>, WebSearchError> {
    let response: Response = serde_json::from_str(body)
        .map_err(|e| WebSearchError::Parse(format!("Brave response: {e}")))?;
    Ok(response
        .web
        .map(|web| web.results)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.url.starts_with("http"))
        .map(|r| SearchHit {
            title: plain_text(&r.title),
            url: r.url,
            snippet: plain_text(&r.description),
        })
        .take(max_results)
        .collect())
}

fn plain_text(fragment: &str) -> String {
    let text: String = Html::parse_fragment(fragment).root_element().text().collect();
    collapse_whitespace(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
        "type": "search",
        "web": {
            "type": "search",
            "results": [
                {
                    "title": "Réveillon 2027 em <strong>Bombinhas</strong> terá queima de fogos",
                    "url": "https://www.bombinhas.sc.gov.br/noticias/reveillon",
                    "description": "A prefeitura confirmou   a <strong>queima de fogos</strong> na praia &amp; shows.",
                    "age": "2 weeks ago"
                },
                {
                    "title": "Second",
                    "url": "https://example.com/second"
                },
                {
                    "title": "Not a web link",
                    "url": "ftp://example.com/file",
                    "description": "skipped"
                }
            ]
        }
    }"#;

    #[test]
    fn parses_results_and_strips_markup() {
        let hits = parse_results(BODY, 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Réveillon 2027 em Bombinhas terá queima de fogos");
        assert_eq!(hits[0].url, "https://www.bombinhas.sc.gov.br/noticias/reveillon");
        assert_eq!(
            hits[0].snippet,
            "A prefeitura confirmou a queima de fogos na praia & shows."
        );
        assert_eq!(hits[1].snippet, "");
    }

    #[test]
    fn respects_max_results() {
        assert_eq!(parse_results(BODY, 1).unwrap().len(), 1);
    }

    #[test]
    fn missing_web_section_means_no_results() {
        assert!(parse_results(r#"{"type":"search"}"#, 5).unwrap().is_empty());
    }

    #[test]
    fn invalid_json_is_a_parse_error() {
        assert!(matches!(
            parse_results("<html>rate limited</html>", 5),
            Err(WebSearchError::Parse(_))
        ));
    }

    #[test]
    fn search_url_encodes_query_and_caps_count() {
        let url = search_url("https://api.example.com/search", "réveillon fogos", 50).unwrap();
        assert_eq!(
            url,
            "https://api.example.com/search?q=r%C3%A9veillon+fogos&count=20"
        );
    }
}
