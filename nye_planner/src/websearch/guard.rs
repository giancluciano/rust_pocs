//! URL vetting for `fetch_text`: the URL comes from an LLM, so it must not be
//! able to point the program at local or internal services.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use reqwest::Url;

use super::WebSearchError;

/// Parses `raw` and rejects anything that is not a public http(s) URL.
pub fn check_url(raw: &str) -> Result<Url, WebSearchError> {
    let url =
        Url::parse(raw.trim()).map_err(|e| WebSearchError::InvalidUrl(format!("{raw}: {e}")))?;
    check_parsed(&url)?;
    Ok(url)
}

pub fn check_parsed(url: &Url) -> Result<(), WebSearchError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(forbidden(url, "only http and https URLs are allowed"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(forbidden(url, "credentials in URLs are not allowed"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| forbidden(url, "URL has no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    match host.parse::<IpAddr>() {
        Ok(ip) if is_internal_ip(ip) => {
            Err(forbidden(url, "private or local addresses are not allowed"))
        }
        Ok(_) => Ok(()),
        Err(_) if is_internal_name(&host) => {
            Err(forbidden(url, "local host names are not allowed"))
        }
        Err(_) => Ok(()),
    }
}

fn forbidden(url: &Url, reason: &str) -> WebSearchError {
    WebSearchError::ForbiddenUrl {
        url: url.to_string(),
        reason: reason.to_string(),
    }
}

fn is_internal_name(host: &str) -> bool {
    host.is_empty()
        || host == "localhost"
        || [".localhost", ".local", ".internal", ".localdomain", ".lan"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
}

fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_internal_v4(v4),
        IpAddr::V6(v6) => is_internal_v6(v6),
    }
}

fn is_internal_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b)) // carrier-grade NAT
}

fn is_internal_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_internal_v4(v4);
    }
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_http_and_https() {
        for u in [
            "https://example.com/a?b=1",
            "http://example.com",
            "https://93.184.216.34/",
            "https://[2606:2800:220:1:248:1893:25c8:1946]/",
            "https://sub.example.co.uk:8443/x",
        ] {
            assert!(check_url(u).is_ok(), "{u} should be accepted");
        }
    }

    #[test]
    fn rejects_other_schemes() {
        for u in [
            "file:///etc/passwd",
            "ftp://example.com/x",
            "javascript:alert(1)",
            "data:text/html,hi",
        ] {
            assert!(check_url(u).is_err(), "{u} should be rejected");
        }
    }

    #[test]
    fn rejects_local_and_private_hosts() {
        for u in [
            "http://localhost/",
            "http://LOCALHOST:8080/",
            "http://localhost./",
            "http://foo.localhost/",
            "http://printer.local/",
            "http://db.internal/",
            "http://127.0.0.1/",
            "http://127.1/",
            "http://2130706433/",
            "http://0x7f.0.0.1/",
            "http://0.0.0.0/",
            "http://10.1.2.3/",
            "http://172.16.0.1/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::]/",
            "http://[fe80::1]/",
            "http://[fd00::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:10.0.0.1]/",
        ] {
            assert!(
                matches!(check_url(u), Err(WebSearchError::ForbiddenUrl { .. })),
                "{u} should be forbidden"
            );
        }
    }

    #[test]
    fn rejects_credentials_and_garbage() {
        assert!(matches!(
            check_url("https://user:pw@example.com/"),
            Err(WebSearchError::ForbiddenUrl { .. })
        ));
        assert!(matches!(
            check_url("not a url"),
            Err(WebSearchError::InvalidUrl(_))
        ));
        assert!(matches!(check_url(""), Err(WebSearchError::InvalidUrl(_))));
    }
}
