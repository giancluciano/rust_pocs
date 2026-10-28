//! DuckDuckGo web search (HTML endpoint, no API key) and page text fetching.

mod guard;
mod results;
mod text;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderValue};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};

use crate::cache::DiskCache;
use crate::http::{PoliteClient, PoliteConfig};

const DDG_HTML_URL: &str = "https://html.duckduckgo.com/html/";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_REDIRECTS: usize = 5;

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
    #[error("not a valid URL: {0}")]
    InvalidUrl(String),
    #[error("refusing to fetch {url}: {reason}")]
    ForbiddenUrl { url: String, reason: String },
}

/// Pacing and limits; tests shrink the intervals.
#[derive(Clone)]
struct Settings {
    search_url: String,
    search_interval: Duration,
    page_interval: Duration,
    search_budget: u32,
    page_budget_per_host: u32,
    /// Skip the localhost/private-address check (tests talk to a local mock server).
    allow_local_urls: bool,
}

impl Settings {
    fn production() -> Self {
        Self {
            search_url: DDG_HTML_URL.to_string(),
            search_interval: Duration::from_secs(2),
            page_interval: Duration::from_secs(1),
            search_budget: 200,
            page_budget_per_host: 40,
            allow_local_urls: false,
        }
    }
}

struct Inner {
    settings: Settings,
    search: PoliteClient,
    /// One client per site, so one site's block page never stops fetches from others.
    pages: Mutex<HashMap<String, Arc<PoliteClient>>>,
    page_cache: DiskCache,
}

/// Cheap to clone; clones share one rate limiter and cache.
#[derive(Clone)]
pub struct WebSearch {
    inner: Arc<Inner>,
}

impl WebSearch {
    pub fn new(cache_dir: &Path) -> Result<Self, WebSearchError> {
        Self::with_settings(cache_dir, Settings::production())
    }

    /// Test constructor: talks to `search_url`, with near-zero delays and no address check.
    #[cfg(test)]
    fn for_test(cache_dir: &Path, search_url: &str) -> Result<Self, WebSearchError> {
        Self::with_settings(
            cache_dir,
            Settings {
                search_url: search_url.to_string(),
                search_interval: Duration::from_millis(1),
                page_interval: Duration::from_millis(1),
                allow_local_urls: true,
                ..Settings::production()
            },
        )
    }

    fn with_settings(cache_dir: &Path, settings: Settings) -> Result<Self, WebSearchError> {
        let search_cache = DiskCache::new(cache_dir.join("websearch"), CACHE_TTL)?;
        let page_cache = DiskCache::new(cache_dir.join("pages"), CACHE_TTL)?;
        let search = PoliteClient::new(search_config(&settings, search_cache))?;
        Ok(Self {
            inner: Arc::new(Inner {
                settings,
                search,
                pages: Mutex::new(HashMap::new()),
                page_cache,
            }),
        })
    }

    /// Top `max_results` organic results for `query`.
    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchHit>, WebSearchError> {
        let query = query.trim();
        if query.is_empty() || max_results == 0 {
            return Ok(Vec::new());
        }
        tracing::info!(query, "web search");
        let html = self
            .inner
            .search
            .post_form(&self.inner.settings.search_url, &[("q", query)])
            .await?;
        let hits = results::parse_results(&html, max_results)?;
        tracing::debug!(query, hits = hits.len(), "web search done");
        Ok(hits)
    }

    /// Visible text of a web page, truncated to `max_chars`.
    pub async fn fetch_text(&self, url: &str, max_chars: usize) -> Result<String, WebSearchError> {
        let parsed = if self.inner.settings.allow_local_urls {
            reqwest::Url::parse(url.trim())
                .map_err(|e| WebSearchError::InvalidUrl(format!("{url}: {e}")))?
        } else {
            guard::check_url(url)?
        };
        tracing::info!(%parsed, "fetching page");
        let client = self.page_client(&parsed)?;
        let html = client.get(parsed.as_str(), None).await?;
        Ok(text::html_to_text(&html, max_chars))
    }

    fn page_client(&self, url: &reqwest::Url) -> Result<Arc<PoliteClient>, WebSearchError> {
        let site = site_key(url);
        let mut pages = self.inner.pages.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(client) = pages.get(&site) {
            return Ok(Arc::clone(client));
        }
        let config = page_config(&self.inner.settings, &site, self.inner.page_cache.clone());
        let client = Arc::new(PoliteClient::new(config)?);
        pages.insert(site, Arc::clone(&client));
        Ok(client)
    }
}

fn site_key(url: &reqwest::Url) -> String {
    match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_string(),
    }
}

fn search_config(settings: &Settings, cache: DiskCache) -> PoliteConfig {
    let mut config = PoliteConfig::new("duckduckgo", USER_AGENT);
    add_browser_headers(&mut config);
    config.min_interval = settings.search_interval;
    config.max_jitter = settings.search_interval / 4;
    config.request_budget = settings.search_budget;
    config.block_detector = results::is_block_page;
    config.cache = Some(cache);
    config
}

fn page_config(settings: &Settings, site: &str, cache: DiskCache) -> PoliteConfig {
    let mut config = PoliteConfig::new(site, USER_AGENT);
    add_browser_headers(&mut config);
    config.min_interval = settings.page_interval;
    config.max_jitter = settings.page_interval / 4;
    config.max_attempts = 2;
    config.base_backoff = Duration::from_secs(2);
    config.request_budget = settings.page_budget_per_host;
    config.cache = Some(cache);
    if !settings.allow_local_urls {
        config.redirect = Some(vetted_redirects());
    }
    config
}

fn add_browser_headers(config: &mut PoliteConfig) {
    config.headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml;q=0.9,*/*;q=0.8"),
    );
    config
        .headers
        .insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.7"));
}

/// Every redirect hop must pass the same address check as the original URL.
fn vetted_redirects() -> Policy {
    Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match guard::check_parsed(attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(e) => attempt.error(e.to_string()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpError;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FIXTURE: &str = include_str!("websearch/fixtures/ddg_results.html");

    fn engine(server: &MockServer, dir: &Path) -> WebSearch {
        WebSearch::for_test(dir, &format!("{}/html/", server.uri())).unwrap()
    }

    #[tokio::test]
    async fn search_posts_query_and_parses_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/html/"))
            .and(body_string_contains("q=new+year+fireworks"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let web = engine(&server, dir.path());

        let hits = web.search("  new year fireworks ", 3).await.unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0].url,
            "https://www.theportugalguides.com/practical/new-years-eve-in-portugal/"
        );
    }

    #[tokio::test]
    async fn repeated_search_is_served_from_the_shared_cache() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let web = engine(&server, dir.path());
        let clone = web.clone();

        let (a, b) = tokio::join!(web.search("fireworks", 5), async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            clone.search("fireworks", 2).await
        });
        assert_eq!(a.unwrap().len(), 5);
        assert_eq!(b.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn empty_query_or_zero_results_makes_no_request() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let web = engine(&server, dir.path());
        assert!(web.search("   ", 5).await.unwrap().is_empty());
        assert!(web.search("x", 0).await.unwrap().is_empty());
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn anomaly_page_trips_the_breaker() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(202).set_body_string(
                "<div class=\"anomaly-modal__title\">Unfortunately, bots use DuckDuckGo too.</div>",
            ))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let web = engine(&server, dir.path());

        for query in ["a", "b"] {
            let err = web.search(query, 5).await.unwrap_err();
            assert!(
                matches!(err, WebSearchError::Http(HttpError::Blocked { .. })),
                "{err}"
            );
        }
    }

    #[tokio::test]
    async fn fetch_text_extracts_visible_text_and_truncates() {
        let server = MockServer::start().await;
        let page = "<html><head><title>t</title></head><body><nav>menu</nav>\
                    <main><h1>Fireworks</h1><p>Midnight show at the harbour.</p></main>\
                    <script>evil()</script></body></html>";
        Mock::given(method("GET"))
            .and(path("/event"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let web = engine(&server, dir.path());
        let url = format!("{}/event", server.uri());

        assert_eq!(
            web.fetch_text(&url, 1000).await.unwrap(),
            "Fireworks Midnight show at the harbour."
        );
        assert_eq!(web.fetch_text(&url, 9).await.unwrap(), "Fireworks");
    }

    #[tokio::test]
    async fn fetch_text_errors_for_missing_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let web = engine(&server, dir.path());
        let err = web
            .fetch_text(&format!("{}/nope", server.uri()), 100)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            WebSearchError::Http(HttpError::Status { status: 404, .. })
        ));
    }

    #[tokio::test]
    async fn fetch_text_rejects_internal_urls_in_production_mode() {
        let dir = tempfile::tempdir().unwrap();
        let web = WebSearch::new(dir.path()).unwrap();
        for url in [
            "http://127.0.0.1:9/",
            "http://localhost/admin",
            "file:///etc/passwd",
            "http://169.254.169.254/latest/meta-data/",
            "nonsense",
        ] {
            let err = web.fetch_text(url, 100).await.unwrap_err();
            assert!(
                matches!(
                    err,
                    WebSearchError::ForbiddenUrl { .. } | WebSearchError::InvalidUrl(_)
                ),
                "{url}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn new_creates_cache_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        WebSearch::new(dir.path()).unwrap();
        assert!(dir.path().join("websearch").is_dir());
        assert!(dir.path().join("pages").is_dir());
    }
}
