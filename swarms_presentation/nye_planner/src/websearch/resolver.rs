//! DNS resolver for page fetches: a public-looking host name must not lead to
//! an internal address (DNS rebinding, `evil.example` -> 127.0.0.1). Every
//! connection, redirect hops included, goes through the client's resolver.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::guard;

#[derive(Debug, thiserror::Error)]
enum ResolveError {
    #[error("could not resolve {host}: {source}")]
    Lookup {
        host: String,
        source: std::io::Error,
    },
    #[error("{host} only resolves to private or local addresses")]
    NoPublicAddress { host: String },
}

/// Resolves with the system resolver and drops every internal address.
pub struct PublicOnlyResolver;

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            // The port is replaced by reqwest with the URL's port.
            let found = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|source| ResolveError::Lookup {
                    host: host.clone(),
                    source,
                })?;
            let public = guard::public_addrs(found);
            if public.is_empty() {
                tracing::warn!(%host, "blocked: no public address");
                return Err(ResolveError::NoPublicAddress { host }.into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpError, PoliteClient, PoliteConfig};
    use std::sync::Arc;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn localhost_has_no_public_address() {
        let name: Name = "localhost".parse().unwrap();
        let err = PublicOnlyResolver.resolve(name).await.err().unwrap();
        assert!(err.to_string().contains("private or local"), "{err}");
    }

    #[tokio::test]
    async fn client_with_resolver_refuses_a_name_pointing_at_loopback() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("secret"))
            .mount(&server)
            .await;
        let port = server.address().port();
        let client = PoliteClient::new(PoliteConfig {
            max_attempts: 1,
            dns_resolver: Some(Arc::new(PublicOnlyResolver)),
            ..PoliteConfig::new("test", "test-agent")
        })
        .unwrap();

        let err = client
            .get(&format!("http://localhost:{port}/"), None)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HttpError::Network(m) if m.contains("private or local")),
            "{err}"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
