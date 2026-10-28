//! A deliberately polite HTTP client:
//! - one request at a time per site, at most one per `min_interval` (+ jitter)
//! - a cookie session, optionally opened with a warm-up page like a browser
//! - backoff that honours `Retry-After` and pauses *all* callers
//! - circuit breaker: once the site pushes back (403, repeated 429, captcha),
//!   every further call fails fast instead of hammering it
//! - request budget per run and an optional disk cache
//!
//! It does not rotate identities, use proxies or solve captchas.

use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use reqwest::{RequestBuilder, StatusCode};
use tokio::sync::{Mutex, OnceCell};
use tokio::time::Instant;

use crate::cache::DiskCache;

const MAX_BACKOFF: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum HttpError {
    #[error("{site} is pushing back ({reason}); stopped sending requests. Try again later.")]
    Blocked { site: String, reason: String },
    #[error("request budget for {site} exhausted ({budget} requests)")]
    BudgetExhausted { site: String, budget: u32 },
    #[error("HTTP {status} from {url}")]
    Status { status: u16, url: String },
    #[error("network error: {0}")]
    Network(String),
}

pub struct PoliteConfig {
    pub site: String,
    pub min_interval: Duration,
    pub max_jitter: Duration,
    pub max_attempts: u32,
    pub base_backoff: Duration,
    pub timeout: Duration,
    pub headers: HeaderMap,
    pub request_budget: u32,
    pub warmup_url: Option<String>,
    /// Returns true if a 200 response is actually a block/captcha page.
    pub block_detector: fn(&str) -> bool,
    pub cache: Option<DiskCache>,
}

impl PoliteConfig {
    pub fn new(site: impl Into<String>, user_agent: &str) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_str(user_agent).unwrap_or(HeaderValue::from_static("Mozilla/5.0")),
        );
        Self {
            site: site.into(),
            min_interval: Duration::from_secs(1),
            max_jitter: Duration::from_millis(400),
            max_attempts: 3,
            base_backoff: Duration::from_secs(5),
            timeout: Duration::from_secs(30),
            headers,
            request_budget: 500,
            warmup_url: None,
            block_detector: |_| false,
            cache: None,
        }
    }
}

pub struct PoliteClient {
    config: PoliteConfig,
    client: reqwest::Client,
    next_slot: Mutex<Instant>,
    blocked: StdMutex<Option<HttpError>>,
    requests_made: AtomicU32,
    warmed_up: OnceCell<()>,
}

impl PoliteClient {
    pub fn new(config: PoliteConfig) -> Result<Self, HttpError> {
        let client = reqwest::Client::builder()
            .default_headers(config.headers.clone())
            .cookie_store(true)
            .gzip(true)
            .brotli(true)
            .timeout(config.timeout)
            .build()
            .map_err(|e| HttpError::Network(e.to_string()))?;
        Ok(Self {
            config,
            client,
            next_slot: Mutex::new(Instant::now()),
            blocked: StdMutex::new(None),
            requests_made: AtomicU32::new(0),
            warmed_up: OnceCell::new(),
        })
    }

    pub fn requests_made(&self) -> u32 {
        self.requests_made.load(Ordering::Relaxed)
    }

    pub async fn get(&self, url: &str, referer: Option<&str>) -> Result<String, HttpError> {
        let key = format!("GET {url}");
        self.cached(&key, || {
            let req = self.client.get(url);
            match referer {
                Some(r) => req.header(reqwest::header::REFERER, r),
                None => req,
            }
        })
        .await
    }

    pub async fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String, HttpError> {
        let key = format!("POST {url} {form:?}");
        self.cached(&key, || self.client.post(url).form(form)).await
    }

    async fn cached(
        &self,
        key: &str,
        build: impl Fn() -> RequestBuilder,
    ) -> Result<String, HttpError> {
        if let Some(body) = self.config.cache.as_ref().and_then(|c| c.get(key)) {
            tracing::debug!(site = %self.config.site, key, "cache hit");
            return Ok(body);
        }
        self.warm_up().await?;
        let body = self.send_with_retries(&build).await?;
        if let Some(cache) = &self.config.cache {
            cache.put(key, &body);
        }
        Ok(body)
    }

    /// Visit the home page once so the session has the cookies a browser would.
    async fn warm_up(&self) -> Result<(), HttpError> {
        let Some(url) = self.config.warmup_url.clone() else {
            return Ok(());
        };
        self.warmed_up
            .get_or_try_init(|| async {
                tracing::info!(site = %self.config.site, "opening session");
                self.send_with_retries(&|| self.client.get(&url))
                    .await
                    .map(|_| ())
            })
            .await
            .map(|_| ())
    }

    fn check_blocked(&self) -> Result<(), HttpError> {
        match self
            .blocked
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn trip_breaker(&self, reason: String) -> HttpError {
        let err = HttpError::Blocked {
            site: self.config.site.clone(),
            reason,
        };
        tracing::error!(error = %err, "circuit breaker tripped");
        *self.blocked.lock().unwrap_or_else(|p| p.into_inner()) = Some(err.clone());
        err
    }

    fn take_budget(&self) -> Result<(), HttpError> {
        let made = self.requests_made.fetch_add(1, Ordering::Relaxed);
        if made >= self.config.request_budget {
            return Err(HttpError::BudgetExhausted {
                site: self.config.site.clone(),
                budget: self.config.request_budget,
            });
        }
        Ok(())
    }

    /// Waits for this caller's slot. The lock is held while sleeping so
    /// callers are served strictly one at a time.
    async fn wait_turn(&self) {
        let mut next = self.next_slot.lock().await;
        tokio::time::sleep_until(*next).await;
        let jitter_ms = fastrand::u64(0..=self.config.max_jitter.as_millis() as u64);
        *next = Instant::now() + self.config.min_interval + Duration::from_millis(jitter_ms);
    }

    /// Pushes the next slot for *every* caller at least `delay` into the future.
    async fn pause_all(&self, delay: Duration) {
        let mut next = self.next_slot.lock().await;
        *next = (*next).max(Instant::now() + delay);
    }

    fn backoff(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        retry_after
            .unwrap_or(self.config.base_backoff * 2u32.pow(attempt.saturating_sub(1)))
            .min(MAX_BACKOFF)
    }

    async fn send_with_retries(
        &self,
        build: &impl Fn() -> RequestBuilder,
    ) -> Result<String, HttpError> {
        let mut last_error = HttpError::Network("no attempt made".into());
        for attempt in 1..=self.config.max_attempts {
            self.check_blocked()?;
            self.take_budget()?;
            self.wait_turn().await;

            let response = match build().send().await {
                Ok(r) => r,
                Err(e) => {
                    last_error = HttpError::Network(e.to_string());
                    self.pause_all(self.backoff(attempt, None)).await;
                    continue;
                }
            };
            let status = response.status();
            let url = response.url().to_string();
            let retry_after = parse_retry_after(response.headers());
            tracing::debug!(site = %self.config.site, %url, %status, attempt, "response");

            match status {
                s if s.is_success() => {
                    let body = response
                        .text()
                        .await
                        .map_err(|e| HttpError::Network(e.to_string()))?;
                    if (self.config.block_detector)(&body) {
                        return Err(self.trip_breaker("challenge/captcha page".into()));
                    }
                    return Ok(body);
                }
                StatusCode::FORBIDDEN => {
                    return Err(self.trip_breaker("HTTP 403 Forbidden".into()));
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    if attempt == self.config.max_attempts {
                        return Err(self.trip_breaker("repeated HTTP 429".into()));
                    }
                    let wait = self.backoff(attempt, retry_after);
                    tracing::warn!(site = %self.config.site, ?wait, "rate limited, backing off");
                    self.pause_all(wait).await;
                    last_error = HttpError::Status { status: 429, url };
                }
                s if s.is_server_error() => {
                    self.pause_all(self.backoff(attempt, retry_after)).await;
                    last_error = HttpError::Status {
                        status: s.as_u16(),
                        url,
                    };
                }
                s => {
                    return Err(HttpError::Status {
                        status: s.as_u16(),
                        url,
                    });
                }
            }
        }
        Err(last_error)
    }
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn config(site: &str) -> PoliteConfig {
        PoliteConfig {
            min_interval: Duration::from_millis(100),
            max_jitter: Duration::ZERO,
            base_backoff: Duration::from_millis(10),
            ..PoliteConfig::new(site, "test-agent")
        }
    }

    #[tokio::test]
    async fn requests_are_spaced_by_min_interval() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = PoliteClient::new(config("test")).unwrap();

        let start = std::time::Instant::now();
        for _ in 0..3 {
            client
                .get(&format!("{}/x", server.uri()), None)
                .await
                .unwrap();
        }
        // 3 requests => at least 2 full intervals between them
        assert!(start.elapsed() >= Duration::from_millis(200));
    }

    #[tokio::test]
    async fn concurrent_callers_share_the_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = std::sync::Arc::new(PoliteClient::new(config("test")).unwrap());
        let url = format!("{}/x", server.uri());

        let start = std::time::Instant::now();
        let (a, b, c) = tokio::join!(
            client.get(&url, None),
            client.get(&url, None),
            client.get(&url, None)
        );
        assert!(a.is_ok() && b.is_ok() && c.is_ok());
        assert!(start.elapsed() >= Duration::from_millis(200));
    }

    #[tokio::test]
    async fn retries_after_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(path("/x"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let client = PoliteClient::new(config("test")).unwrap();
        assert_eq!(
            client
                .get(&format!("{}/x", server.uri()), None)
                .await
                .unwrap(),
            "ok"
        );
        assert_eq!(client.requests_made(), 2);
    }

    #[tokio::test]
    async fn forbidden_trips_breaker_and_stops_all_requests() {
        let server = MockServer::start().await;
        Mock::given(path("/x"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let client = PoliteClient::new(config("test")).unwrap();
        let url = format!("{}/x", server.uri());

        assert!(matches!(
            client.get(&url, None).await,
            Err(HttpError::Blocked { .. })
        ));
        assert!(matches!(
            client.get(&url, None).await,
            Err(HttpError::Blocked { .. })
        ));
        assert_eq!(
            client.requests_made(),
            1,
            "no request after the breaker trips"
        );
    }

    #[tokio::test]
    async fn captcha_page_trips_breaker() {
        let server = MockServer::start().await;
        Mock::given(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("please solve the captcha"))
            .mount(&server)
            .await;
        let client = PoliteClient::new(PoliteConfig {
            block_detector: |b| b.contains("captcha"),
            ..config("test")
        })
        .unwrap();
        assert!(matches!(
            client.get(&format!("{}/x", server.uri()), None).await,
            Err(HttpError::Blocked { .. })
        ));
    }

    #[tokio::test]
    async fn not_found_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(path("/x"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let client = PoliteClient::new(config("test")).unwrap();
        assert!(matches!(
            client.get(&format!("{}/x", server.uri()), None).await,
            Err(HttpError::Status { status: 404, .. })
        ));
    }

    #[tokio::test]
    async fn budget_is_enforced() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = PoliteClient::new(PoliteConfig {
            request_budget: 1,
            ..config("test")
        })
        .unwrap();
        client
            .get(&format!("{}/a", server.uri()), None)
            .await
            .unwrap();
        assert!(matches!(
            client.get(&format!("{}/b", server.uri()), None).await,
            Err(HttpError::BudgetExhausted { .. })
        ));
    }

    #[tokio::test]
    async fn cache_avoids_second_request_and_warmup_runs_once() {
        let server = MockServer::start().await;
        Mock::given(path("/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("body"))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = PoliteClient::new(PoliteConfig {
            cache: Some(DiskCache::new(dir.path(), Duration::from_secs(60)).unwrap()),
            warmup_url: Some(format!("{}/", server.uri())),
            ..config("test")
        })
        .unwrap();
        let url = format!("{}/x", server.uri());
        assert_eq!(client.get(&url, None).await.unwrap(), "body");
        assert_eq!(client.get(&url, None).await.unwrap(), "body");
        assert_eq!(client.requests_made(), 2); // warm-up + one real request
    }
}
