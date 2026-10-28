//! A deliberately polite HTTP client:
//! - one request at a time per site, at most one per `min_interval` (+ jitter)
//! - a cookie session, optionally opened with a warm-up page like a browser
//! - backoff that honours `Retry-After` and pauses *all* callers
//! - circuit breaker: once the site pushes back (403, repeated 429, captcha),
//!   every further call fails fast instead of hammering it
//! - request budget per run and an optional disk cache
//!
//! It does not rotate identities, use proxies or solve captchas.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue, RETRY_AFTER};
use reqwest::{RequestBuilder, Response, StatusCode};
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
    #[error("response from {url} is larger than the {limit} byte limit")]
    BodyTooLarge { url: String, limit: usize },
    #[error("{url} returned {content_type}, which is not an accepted content type")]
    ContentType { url: String, content_type: String },
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
    /// Custom redirect policy (e.g. to vet every hop); reqwest's default if `None`.
    pub redirect: Option<reqwest::redirect::Policy>,
    /// Abort (and never cache) responses whose decoded body exceeds this many bytes.
    pub max_body_bytes: Option<usize>,
    /// Accepted `Content-Type` prefixes; a missing header is accepted. Any if `None`.
    pub allowed_content_types: Option<&'static [&'static str]>,
    /// Custom DNS resolver (e.g. to refuse internal addresses); the system one if `None`.
    pub dns_resolver: Option<Arc<dyn reqwest::dns::Resolve>>,
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
            redirect: None,
            max_body_bytes: None,
            allowed_content_types: None,
            dns_resolver: None,
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
    pub fn new(mut config: PoliteConfig) -> Result<Self, HttpError> {
        let mut builder = reqwest::Client::builder()
            .default_headers(config.headers.clone())
            .cookie_store(true)
            .gzip(true)
            .brotli(true)
            .timeout(config.timeout);
        if let Some(policy) = config.redirect.take() {
            builder = builder.redirect(policy);
        }
        if let Some(resolver) = config.dns_resolver.take() {
            builder = builder.dns_resolver2(resolver);
        }
        let client = builder
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

    /// False once the circuit breaker tripped or the request budget ran out.
    pub fn is_available(&self) -> bool {
        self.check_blocked().is_ok() && self.requests_made() < self.config.request_budget
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

    fn check_content_type(&self, response: &Response) -> Result<(), HttpError> {
        let Some(allowed) = self.config.allowed_content_types else {
            return Ok(());
        };
        let Some(value) = response.headers().get(CONTENT_TYPE) else {
            return Ok(());
        };
        let content_type = String::from_utf8_lossy(value.as_bytes())
            .trim()
            .to_ascii_lowercase();
        if allowed
            .iter()
            .any(|prefix| content_type.starts_with(prefix))
        {
            return Ok(());
        }
        Err(HttpError::ContentType {
            url: response.url().to_string(),
            content_type,
        })
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
                    last_error = network_error(&e);
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
                    self.check_content_type(&response)?;
                    let body = read_body(response, self.config.max_body_bytes).await?;
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

/// Reads the decoded body chunk by chunk, giving up as soon as it passes `limit`.
async fn read_body(mut response: Response, limit: Option<usize>) -> Result<String, HttpError> {
    let too_large = |response: &Response| HttpError::BodyTooLarge {
        url: response.url().to_string(),
        limit: limit.unwrap_or(usize::MAX),
    };
    if let (Some(limit), Some(declared)) = (limit, response.content_length())
        && declared > limit as u64
    {
        return Err(too_large(&response));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| network_error(&e))? {
        if limit.is_some_and(|limit| body.len() + chunk.len() > limit) {
            return Err(too_large(&response));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// `reqwest::Error` hides its cause (a refused redirect, a blocked DNS name) behind
/// a generic message, so include the whole source chain.
fn network_error(error: &reqwest::Error) -> HttpError {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    HttpError::Network(message)
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

    #[tokio::test]
    async fn body_over_the_cap_is_rejected_and_not_cached() {
        let server = MockServer::start().await;
        Mock::given(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(101)))
            .expect(2)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = PoliteClient::new(PoliteConfig {
            max_body_bytes: Some(100),
            cache: Some(DiskCache::new(dir.path(), Duration::from_secs(60)).unwrap()),
            ..config("test")
        })
        .unwrap();
        let url = format!("{}/big", server.uri());

        for _ in 0..2 {
            assert!(matches!(
                client.get(&url, None).await,
                Err(HttpError::BodyTooLarge { limit: 100, .. })
            ));
        }
        // both calls reached the server: the aborted body never entered the cache
        assert_eq!(client.requests_made(), 2);
    }

    #[tokio::test]
    async fn body_within_the_cap_is_returned() {
        let server = MockServer::start().await;
        Mock::given(path("/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("y".repeat(100)))
            .mount(&server)
            .await;
        let client = PoliteClient::new(PoliteConfig {
            max_body_bytes: Some(100),
            ..config("test")
        })
        .unwrap();
        let body = client
            .get(&format!("{}/ok", server.uri()), None)
            .await
            .unwrap();
        assert_eq!(body.len(), 100);
    }

    #[tokio::test]
    async fn disallowed_content_type_is_rejected() {
        let server = MockServer::start().await;
        Mock::given(path("/bin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/pdf")
                    .set_body_bytes(vec![0u8; 10]),
            )
            .mount(&server)
            .await;
        let client = PoliteClient::new(PoliteConfig {
            allowed_content_types: Some(&["text/html", "text/plain"]),
            ..config("test")
        })
        .unwrap();
        let err = client
            .get(&format!("{}/bin", server.uri()), None)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HttpError::ContentType { content_type, .. } if content_type == "application/pdf"),
            "{err}"
        );
        assert!(
            client.is_available(),
            "a wrong type must not trip the breaker"
        );
    }

    #[tokio::test]
    async fn allowed_content_type_matches_by_prefix_and_missing_header_passes() {
        let server = MockServer::start().await;
        Mock::given(path("/html"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw("<p>hi</p>", "Text/HTML; charset=UTF-8"),
            )
            .mount(&server)
            .await;
        Mock::given(path("/none"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"raw".to_vec()))
            .mount(&server)
            .await;
        let client = PoliteClient::new(PoliteConfig {
            allowed_content_types: Some(&["text/html"]),
            ..config("test")
        })
        .unwrap();
        assert_eq!(
            client
                .get(&format!("{}/html", server.uri()), None)
                .await
                .unwrap(),
            "<p>hi</p>"
        );
        assert_eq!(
            client
                .get(&format!("{}/none", server.uri()), None)
                .await
                .unwrap(),
            "raw"
        );
    }
}
