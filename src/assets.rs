//! Images for `<img src="...">`.
//!
//! - `data:image/...` URIs are decoded (always on).
//! - `https://` URLs are fetched only when the host is in `ASSET_ALLOWED_HOSTS`. Fetches never
//!   follow redirects, refuse private/loopback addresses (checked after DNS, and the connection
//!   is pinned to the checked address), accept only `image/*` responses, and are capped in size
//!   and time.
//! - Anything else (`file://`, relative paths, blocked hosts) is skipped; the PDF still renders.

use crate::image_util::{Skip, decode_data_uri, image_sources, is_supported_image};
use futures_util::future::join_all;
use moka::future::Cache;
use std::{
    env,
    net::{IpAddr, SocketAddr},
    sync::{Arc, LazyLock},
    time::Duration,
};

/// Fetched remote images, shared by every request. Receipts repeat the same logo, so this
/// skips DNS, the TLS handshake and the download for all but the first. Concurrent requests
/// for one URL share a single fetch. Only validated successes are stored.
/// `ASSET_CACHE_BYTES` (default 256 MiB, 0 disables) bounds it by image size;
/// `ASSET_CACHE_TTL_SECS` (default 300) bounds staleness.
static IMAGE_CACHE: LazyLock<Option<Cache<String, Arc<Vec<u8>>>>> = LazyLock::new(|| {
    let num = |k: &str, d: u64| {
        env::var(k)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(d)
    };
    let bytes = num("ASSET_CACHE_BYTES", 256 * 1024 * 1024);
    (bytes > 0).then(|| {
        Cache::builder()
            .max_capacity(bytes)
            .weigher(|_, v: &Arc<Vec<u8>>| u32::try_from(v.len()).unwrap_or(u32::MAX))
            .time_to_live(Duration::from_secs(num("ASSET_CACHE_TTL_SECS", 300)))
            .build()
    })
});

#[derive(Debug, Clone)]
pub struct AssetPolicy {
    /// Lowercase hosts; `*.example.com` matches subdomains. Empty disables remote fetching.
    pub allowed_hosts: Vec<String>,
    pub max_bytes: usize,
    pub max_images: usize,
    pub timeout: Duration,
    /// Allow plain `http://` (`ASSET_ALLOW_HTTP`). Off by default.
    pub allow_http: bool,
    /// Allow private, loopback and link-local addresses (`ASSET_ALLOW_PRIVATE_IPS`). Off by
    /// default; only for trusted internal networks.
    pub allow_private_ips: bool,
}

impl Default for AssetPolicy {
    fn default() -> Self {
        Self {
            allowed_hosts: Vec::new(),
            max_bytes: 5 * 1024 * 1024,
            max_images: 20,
            timeout: Duration::from_secs(5),
            allow_http: false,
            allow_private_ips: false,
        }
    }
}

impl AssetPolicy {
    pub fn from_env() -> Self {
        let d = Self::default();
        let num = |k: &str| env::var(k).ok().and_then(|v| v.trim().parse::<u64>().ok());
        let flag = |k: &str| env::var(k).is_ok_and(|v| matches!(v.trim(), "1" | "true" | "yes"));
        Self {
            allowed_hosts: env::var("ASSET_ALLOWED_HOSTS")
                .unwrap_or_default()
                .split(',')
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect(),
            max_bytes: num("ASSET_MAX_BYTES").map_or(d.max_bytes, |v| v as usize),
            max_images: num("ASSET_MAX_IMAGES").map_or(d.max_images, |v| v as usize),
            timeout: num("ASSET_TIMEOUT_MS").map_or(d.timeout, Duration::from_millis),
            allow_http: flag("ASSET_ALLOW_HTTP"),
            allow_private_ips: flag("ASSET_ALLOW_PRIVATE_IPS"),
        }
    }

    fn host_allowed(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.allowed_hosts
            .iter()
            .any(|a| match a.strip_prefix("*.") {
                Some(domain) => host.ends_with(&format!(".{domain}")),
                None => *a == host,
            })
    }
}

/// Resolve every `<img src>` in `html` the policy allows, keyed by the exact `src` value.
pub async fn resolve_images(policy: &AssetPolicy, html: &str) -> Vec<(String, Arc<Vec<u8>>)> {
    let sources: Vec<String> = image_sources(html)
        .into_iter()
        .take(policy.max_images)
        .collect();
    let fetches = sources.into_iter().map(|src| async move {
        let result = load(policy, &src).await.and_then(|bytes| {
            if is_supported_image(&bytes) {
                Ok(bytes)
            } else {
                Err(Skip::Blocked("not a PNG, JPEG, GIF or SVG image"))
            }
        });
        match &result {
            Err(Skip::Blocked(why)) => {
                tracing::warn!(src = %truncate(&src), why, "image blocked");
            }
            Err(Skip::Failed(why)) => {
                tracing::warn!(src = %truncate(&src), why = %why, "image fetch failed");
            }
            Ok(_) | Err(Skip::Ignored) => {}
        }
        result.ok().map(|bytes| (src, bytes))
    });
    join_all(fetches).await.into_iter().flatten().collect()
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

async fn load(policy: &AssetPolicy, src: &str) -> Result<Arc<Vec<u8>>, Skip> {
    if let Some(rest) = src.strip_prefix("data:") {
        return decode_data_uri(rest, policy.max_bytes).map(Arc::new);
    }
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return fetch(policy, src).await;
    }
    Err(Skip::Ignored)
}

async fn fetch(policy: &AssetPolicy, src: &str) -> Result<Arc<Vec<u8>>, Skip> {
    if policy.allowed_hosts.is_empty() {
        return Err(Skip::Blocked(
            "remote images disabled (ASSET_ALLOWED_HOSTS unset)",
        ));
    }
    let url = reqwest::Url::parse(src).map_err(|_| Skip::Failed("invalid URL".into()))?;
    match url.scheme() {
        "https" => {}
        "http" if policy.allow_http => {}
        _ => return Err(Skip::Blocked("scheme not allowed")),
    }
    let host = url.host_str().ok_or(Skip::Blocked("no host"))?.to_string();
    if !policy.host_allowed(&host) {
        return Err(Skip::Blocked("host not in ASSET_ALLOWED_HOSTS"));
    }
    let port = url
        .port_or_known_default()
        .ok_or(Skip::Blocked("no port"))?;

    // The scheme and host checks above run on every call; the cache only skips the network.
    let Some(cache) = IMAGE_CACHE.as_ref() else {
        return fetch_pinned(policy, url, host, port).await.map(Arc::new);
    };
    cache
        .try_get_with(src.to_string(), async {
            let bytes = fetch_pinned(policy, url, host, port).await?;
            if is_supported_image(&bytes) {
                Ok(Arc::new(bytes))
            } else {
                Err(Skip::Blocked("not a PNG, JPEG, GIF or SVG image"))
            }
        })
        .await
        .map_err(|e| (*e).clone())
}

/// DNS, address checks and the download itself, pinned to the addresses that were checked.
async fn fetch_pinned(
    policy: &AssetPolicy,
    url: reqwest::Url,
    host: String,
    port: u16,
) -> Result<Vec<u8>, Skip> {
    let lookup = tokio::time::timeout(
        policy.timeout,
        tokio::net::lookup_host((host.as_str(), port)),
    )
    .await
    .map_err(|_| Skip::Failed("DNS timeout".into()))?
    .map_err(|e| Skip::Failed(format!("DNS: {e}")))?;
    let addrs: Vec<SocketAddr> = lookup.collect();
    if addrs.is_empty() {
        return Err(Skip::Failed("DNS: no addresses".into()));
    }
    if !policy.allow_private_ips && addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(Skip::Blocked("resolves to a private address"));
    }

    // Pin the connection to the addresses we just checked (no second DNS lookup).
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(policy.timeout)
        .no_proxy()
        .resolve_to_addrs(&host, &addrs)
        .build()
        .map_err(|e| Skip::Failed(e.to_string()))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| Skip::Failed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Skip::Failed(format!("HTTP {}", resp.status())));
    }
    let is_image = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.trim().to_ascii_lowercase().starts_with("image/"));
    if !is_image {
        return Err(Skip::Blocked("response is not an image"));
    }
    if resp
        .content_length()
        .is_some_and(|n| n > policy.max_bytes as u64)
    {
        return Err(Skip::Blocked("image too large"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| Skip::Failed(e.to_string()))?
    {
        if body.len() + chunk.len() > policy.max_bytes {
            return Err(Skip::Blocked("image too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Publicly routable unicast address.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b)) // CGNAT 100.64/10
                || (a == 192 && b == 0 && c == 0) // 192.0.0/24
                || (a == 198 && (b == 18 || b == 19)) // benchmarking 198.18/15
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (first & 0xffc0) == 0xfe80) // link local fe80::/10
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_img_sources_and_decodes_entities() {
        let html = r#"<p><IMG class="a" SRC="https:&#x2f;&#x2f;cdn.example.com&#x2f;a.png?x=1&amp;y=2">
            <img data-src="nope" src='b.png'><img src=c.png alt=x><img src="b.png"><img alt="no src"></p>"#;
        assert_eq!(
            image_sources(html),
            vec!["https://cdn.example.com/a.png?x=1&y=2", "b.png", "c.png"]
        );
    }

    #[test]
    fn host_allowlist_with_wildcards() {
        let p = AssetPolicy {
            allowed_hosts: vec!["cdn.example.com".into(), "*.assets.io".into()],
            ..Default::default()
        };
        assert!(p.host_allowed("cdn.example.com"));
        assert!(p.host_allowed("CDN.EXAMPLE.COM"));
        assert!(p.host_allowed("img.assets.io"));
        assert!(!p.host_allowed("assets.io"));
        assert!(!p.host_allowed("evil-cdn.example.com"));
        assert!(!p.host_allowed("cdn.example.com.evil.io"));
    }

    #[test]
    fn private_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "224.0.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn data_uris() {
        assert_eq!(
            decode_data_uri("image/png;base64,aGk=", 100).ok(),
            Some(b"hi".to_vec())
        );
        assert_eq!(
            decode_data_uri("image/svg+xml,%3Csvg%3E", 100).ok(),
            Some(b"<svg>".to_vec())
        );
        assert_eq!(
            decode_data_uri("image/svg+xml,é%é%4", 100).ok(),
            Some("é%é%4".as_bytes().to_vec())
        );
        assert!(matches!(
            decode_data_uri("text/html,<b>", 100),
            Err(Skip::Ignored)
        ));
        assert!(matches!(
            decode_data_uri("image/png;base64,aGVsbG8gd29ybGQ=", 4),
            Err(Skip::Blocked(_))
        ));
    }
}
