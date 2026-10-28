//! URL vetting for `fetch_text`: the URL comes from an LLM, so it must not be
//! able to point the program at local or internal services.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

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
        || !host.contains('.') // single-label names resolve through search domains / intranet DNS
        || host == "localhost"
        || [".localhost", ".local", ".internal", ".localdomain", ".lan"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
}

/// Keeps only the addresses a public web page may be served from.
pub fn public_addrs(addrs: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    addrs
        .into_iter()
        .filter(|addr| !is_internal_ip(addr.ip()))
        .collect()
}

fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_internal_v4(v4),
        IpAddr::V6(v6) => is_internal_v6(v6),
    }
}

fn is_internal_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b)) // carrier-grade NAT
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (18..=19).contains(&b)) // benchmarking
        || a >= 240 // reserved, includes 255.255.255.255
}

fn is_internal_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_internal_v4(v4);
    }
    if let Some(v4) = embedded_v4(ip) {
        return is_internal_v4(v4);
    }
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
}

/// The IPv4 address carried by NAT64 (64:ff9b::/96) and 6to4 (2002::/16) addresses.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let v4 = |hi: u16, lo: u16| {
        let [a, b] = hi.to_be_bytes();
        let [c, d] = lo.to_be_bytes();
        Ipv4Addr::new(a, b, c, d)
    };
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        Some(v4(s[6], s[7]))
    } else if s[0] == 0x2002 {
        Some(v4(s[1], s[2]))
    } else {
        None
    }
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
            "https://198.20.0.1/",
            "https://[64:ff9b::808:808]/",
            "https://[2002:5db8:d822::1]/",
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
            "http://192.0.0.8/",
            "http://198.18.0.1/",
            "http://198.19.255.254/",
            "http://240.0.0.1/",
            "http://255.255.255.255/",
            "http://[64:ff9b::7f00:1]/",
            "http://[64:ff9b::a9fe:a9fe]/",
            "http://[2002:7f00:1::]/",
            "http://[2002:c0a8:101::1]/",
            "http://intranet/",
            "http://intranet./",
            "http://wiki:8080/",
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

    #[test]
    fn public_addrs_drops_internal_addresses() {
        let addr = |s: &str| s.parse::<SocketAddr>().unwrap();
        let found = [
            addr("127.0.0.1:0"),
            addr("93.184.216.34:0"),
            addr("[::1]:0"),
            addr("10.0.0.5:0"),
            addr("[2606:2800:220:1:248:1893:25c8:1946]:0"),
            addr("[64:ff9b::a00:1]:0"),
        ];
        assert_eq!(
            public_addrs(found),
            [
                addr("93.184.216.34:0"),
                addr("[2606:2800:220:1:248:1893:25c8:1946]:0")
            ]
        );
        assert!(public_addrs([addr("169.254.169.254:0")]).is_empty());
        assert!(public_addrs([]).is_empty());
    }
}
