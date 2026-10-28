//! Parsing of DuckDuckGo's no-JS HTML results page.

use reqwest::Url;
use scraper::{ElementRef, Html, Selector};

use super::text::collapse_whitespace;
use super::{SearchHit, WebSearchError};

const DDG_ORIGIN: &str = "https://duckduckgo.com/";

/// True if a 200 response is DuckDuckGo's "anomaly"/captcha page rather than results.
pub fn is_block_page(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    !lower.contains("result__a") && (lower.contains("anomaly") || lower.contains("captcha"))
}

/// Organic results in page order, at most `max_results`; ads are skipped.
pub fn parse_results(html: &str, max_results: usize) -> Result<Vec<SearchHit>, WebSearchError> {
    let doc = Html::parse_document(html);
    let result = selector(".result")?;
    let link = selector("a.result__a")?;
    let snippet = selector(".result__snippet")?;

    let hits: Vec<SearchHit> = doc
        .select(&result)
        .filter(|block| !has_class(block, "result--ad"))
        .filter_map(|block| parse_block(&block, &link, &snippet))
        .take(max_results)
        .collect();

    if hits.is_empty() && !is_no_results_page(&doc)? {
        return Err(WebSearchError::Parse(
            "no result blocks and no \"no results\" notice".into(),
        ));
    }
    Ok(hits)
}

fn parse_block(block: &ElementRef<'_>, link: &Selector, snippet: &Selector) -> Option<SearchHit> {
    let anchor = block.select(link).next()?;
    let url = resolve_link(anchor.value().attr("href")?)?;
    let title = collapse_whitespace(&anchor.text().collect::<String>());
    if title.is_empty() {
        return None;
    }
    let snippet = block
        .select(snippet)
        .next()
        .map(|s| collapse_whitespace(&s.text().collect::<String>()))
        .unwrap_or_default();
    Some(SearchHit {
        title,
        url,
        snippet,
    })
}

/// Turns a result href into the real target URL; `None` for ads and non-http(s) links.
fn resolve_link(href: &str) -> Option<String> {
    let base = Url::parse(DDG_ORIGIN).ok()?;
    let url = base.join(href.trim()).ok()?;
    if is_ddg_host(&url) {
        return match url.path() {
            "/l/" => redirect_target(&url),
            _ => None, // y.js ad clicks and other DDG-internal links
        };
    }
    is_web_url(&url).then(|| url.to_string())
}

fn redirect_target(url: &Url) -> Option<String> {
    let (_, target) = url.query_pairs().find(|(k, _)| k == "uddg")?;
    let target = Url::parse(&target).ok()?;
    (is_web_url(&target) && !is_ddg_host(&target)).then(|| target.to_string())
}

fn is_ddg_host(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|h| h == "duckduckgo.com" || h.ends_with(".duckduckgo.com"))
}

fn is_web_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn has_class(el: &ElementRef<'_>, class: &str) -> bool {
    el.value().classes().any(|c| c == class)
}

fn is_no_results_page(doc: &Html) -> Result<bool, WebSearchError> {
    Ok(doc.select(&selector(".no-results")?).next().is_some()
        || doc.select(&selector("#links")?).next().is_some())
}

fn selector(css: &str) -> Result<Selector, WebSearchError> {
    Selector::parse(css).map_err(|e| WebSearchError::Parse(format!("bad selector {css}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/ddg_results.html");

    #[test]
    fn parses_organic_results_and_skips_ads() {
        let hits = parse_results(FIXTURE, 10).unwrap();
        assert_eq!(hits.len(), 5, "6 blocks, 1 of them an ad");
        assert!(
            hits.iter()
                .all(|h| !h.url.contains("y.js") && !h.title.contains("Cheap Hotels"))
        );
        assert_eq!(
            hits[0].title,
            "New Year's Eve in Portugal 2026 | The Portugal Guides"
        );
        assert_eq!(
            hits[0].url,
            "https://www.theportugalguides.com/practical/new-years-eve-in-portugal/"
        );
        assert!(
            hits[0]
                .snippet
                .starts_with("New Year's Eve in Portugal 2026 into 2027")
        );
    }

    #[test]
    fn decodes_redirect_links() {
        let hits = parse_results(FIXTURE, 10).unwrap();
        assert_eq!(
            hits[1].url,
            "https://cooltouroporto.com/blog/new-years-eve-portugal-guide"
        );
        // relative redirect with an encoded query string in the target
        assert_eq!(
            hits[2].url,
            "https://www.lisbon.example/nye?year=2026&lang=en"
        );
    }

    #[test]
    fn trims_and_collapses_whitespace() {
        let hits = parse_results(FIXTURE, 10).unwrap();
        assert_eq!(hits[2].title, "Lisbon NYE 2026");
        assert_eq!(hits[2].snippet, "Fireworks over the Tagus at midnight.");
    }

    #[test]
    fn honours_max_results() {
        assert_eq!(parse_results(FIXTURE, 2).unwrap().len(), 2);
        assert_eq!(parse_results(FIXTURE, 0).unwrap().len(), 0);
        assert_eq!(parse_results(FIXTURE, 100).unwrap().len(), 5);
    }

    #[test]
    fn max_results_counts_organic_results_only() {
        // the ad comes first in the fixture; asking for 1 must still yield a real hit
        let hits = parse_results(FIXTURE, 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].url.starts_with("https://www.theportugalguides.com"));
    }

    #[test]
    fn empty_results_page_is_ok_but_unknown_markup_is_an_error() {
        let none =
            r#"<div id="links" class="results"><div class="no-results">No results.</div></div>"#;
        assert_eq!(parse_results(none, 5).unwrap(), vec![]);
        assert!(matches!(
            parse_results("<html><body>something else</body></html>", 5),
            Err(WebSearchError::Parse(_))
        ));
    }

    #[test]
    fn non_web_and_ddg_internal_links_are_dropped() {
        let html = r#"<div id="links">
            <div class="result"><a class="result__a" href="javascript:alert(1)">bad</a></div>
            <div class="result"><a class="result__a" href="//duckduckgo.com/l/?uddg=file%3A%2F%2F%2Fetc%2Fpasswd">bad</a></div>
            <div class="result"><a class="result__a" href="//duckduckgo.com/l/?rut=nothing">bad</a></div>
            <div class="result"><a class="result__a" href="https://ok.example/">fine</a></div>
        </div>"#;
        let hits = parse_results(html, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://ok.example/");
        assert_eq!(hits[0].snippet, "");
    }

    #[test]
    fn detects_block_pages() {
        assert!(is_block_page(
            "<div class=\"anomaly-modal__modal\">Unfortunately, bots use DuckDuckGo too</div>"
        ));
        assert!(is_block_page("please complete the CAPTCHA"));
        assert!(!is_block_page(FIXTURE));
        assert!(!is_block_page(
            "<div class=\"no-results\">No results.</div>"
        ));
    }
}
