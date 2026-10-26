//! Images for `<img src="...">`.
//!
//! - `data:image/...` URIs are decoded (always on).
//! - `https://` URLs are fetched only when the host is in `ASSET_ALLOWED_HOSTS`. Fetches never
//!   follow redirects, refuse private/loopback addresses (checked after DNS, and the connection
//!   is pinned to the checked address), accept only `image/*` responses, and are capped in size
//!   and time.
//! - Anything else (`file://`, relative paths, blocked hosts) is skipped; the PDF still renders.
//!   Unless `ASSET_LOCAL_DIR` is set: then files inside that folder load too (IPC mode).

use crate::font_face::{FontFaceDecl, LoadedFontFace, font_face_decls, is_font};
use crate::image_util::{
    Skip, decode_data_uri_as, image_sources, is_supported_image, percent_decode,
};
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
    /// Folder that `<img src>` may read local files from (`ASSET_LOCAL_DIR`): relative paths and
    /// `file:` URLs inside it. Unset by default, and only meant for a program that renders its
    /// own templates (the IPC mode); a web server rendering other people's templates keeps it off.
    pub local_dir: Option<std::path::PathBuf>,
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
            local_dir: None,
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
            local_dir: env::var_os("ASSET_LOCAL_DIR")
                .filter(|d| !d.is_empty())
                .map(Into::into),
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
    resolve_sources(policy, image_sources(html)).await
}

/// Resolve these `src` values (at most `max_images` of them) as `resolve_images` does.
pub async fn resolve_sources(
    policy: &AssetPolicy,
    sources: Vec<String>,
) -> Vec<(String, Arc<Vec<u8>>)> {
    let sources: Vec<String> = sources.into_iter().take(policy.max_images).collect();
    let fetches = sources.into_iter().map(|src| async move {
        let result = load(policy, &src, Kind::Image).await.and_then(|bytes| {
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

/// Load the fonts of every usable `@font-face` rule in `html`, by the same rules as images (a `data:` URI always; a
/// URL only from an allowed host; a file only inside the allowed folder). A font that cannot be loaded is skipped.
pub async fn resolve_font_faces(policy: &AssetPolicy, html: &str) -> Vec<LoadedFontFace> {
    resolve_font_decls(policy, font_face_decls(html)).await
}

/// As [`resolve_font_faces`], for rules already read.
pub async fn resolve_font_decls(
    policy: &AssetPolicy,
    decls: Vec<FontFaceDecl>,
) -> Vec<LoadedFontFace> {
    let fetches = decls.into_iter().map(|decl| async move {
        let result = load(policy, &decl.src, Kind::Font).await.and_then(|bytes| {
            if is_font(&bytes) {
                Ok(bytes)
            } else {
                Err(Skip::Blocked("not a TrueType, OpenType or WOFF2 font"))
            }
        });
        match &result {
            Err(Skip::Blocked(why)) => {
                tracing::warn!(src = %truncate(&decl.src), why, "font blocked");
            }
            Err(Skip::Failed(why)) => {
                tracing::warn!(src = %truncate(&decl.src), why = %why, "font fetch failed");
            }
            Ok(_) | Err(Skip::Ignored) => {}
        }
        result.ok().map(|data| LoadedFontFace {
            family: decl.family,
            data,
            weight: decl.weight,
            italic: decl.italic,
        })
    });
    join_all(fetches).await.into_iter().flatten().collect()
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

/// What a source is loaded for: images and fonts share the loading rules but not the size limit or the checks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Image,
    Font,
}

/// The largest font file loaded (CJK fonts are several MB).
const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;

async fn load(policy: &AssetPolicy, src: &str, kind: Kind) -> Result<Arc<Vec<u8>>, Skip> {
    let max = if kind == Kind::Font {
        policy.max_bytes.max(MAX_FONT_BYTES)
    } else {
        policy.max_bytes
    };
    if let Some(rest) = src.strip_prefix("data:") {
        return decode_data_uri_as(rest, max, kind == Kind::Image).map(Arc::new);
    }
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return fetch(policy, src, kind, max).await;
    }
    if let Some(dir) = &policy.local_dir {
        return read_local(dir, src, max).await;
    }
    Err(Skip::Ignored)
}

/// A file inside `dir` (relative path or `file:` URL). Anything that resolves outside it,
/// through `..` or a symlink, is refused.
async fn read_local(dir: &std::path::Path, src: &str, max: usize) -> Result<Arc<Vec<u8>>, Skip> {
    let path = match src.get(..5).filter(|p| p.eq_ignore_ascii_case("file:")) {
        Some(_) => {
            let rest = src[5..].trim_start_matches("//");
            let rest = rest.strip_prefix("localhost").unwrap_or(rest);
            let decoded = String::from_utf8_lossy(&percent_decode(rest)).into_owned();
            // `file:///C:/x.png` leaves `/C:/x.png` on Windows.
            let decoded = match decoded.as_bytes() {
                [b'/', d, b':', ..] if d.is_ascii_alphabetic() => decoded[1..].to_string(),
                _ => decoded,
            };
            std::path::PathBuf::from(decoded)
        }
        None if src.contains("://") => return Err(Skip::Ignored),
        None => {
            std::path::PathBuf::from(String::from_utf8_lossy(&percent_decode(src)).into_owned())
        }
    };
    let base = tokio::fs::canonicalize(dir)
        .await
        .map_err(|e| Skip::Failed(format!("image folder: {e}")))?;
    let full = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    let full = tokio::fs::canonicalize(&full)
        .await
        .map_err(|_| Skip::Failed("file not found".into()))?;
    if !full.starts_with(&base) {
        return Err(Skip::Blocked("outside the image folder"));
    }
    let len = tokio::fs::metadata(&full)
        .await
        .map_err(|e| Skip::Failed(e.to_string()))?
        .len();
    if len > max as u64 {
        return Err(Skip::Blocked("file too large"));
    }
    tokio::fs::read(&full)
        .await
        .map(Arc::new)
        .map_err(|e| Skip::Failed(e.to_string()))
}

async fn fetch(
    policy: &AssetPolicy,
    src: &str,
    kind: Kind,
    max: usize,
) -> Result<Arc<Vec<u8>>, Skip> {
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

    // Fonts are not cached (they are large and rarely repeat across requests).
    if kind == Kind::Font {
        return fetch_pinned(policy, url, host, port, kind, max)
            .await
            .map(Arc::new);
    }
    // The scheme and host checks above run on every call; the cache only skips the network.
    let Some(cache) = IMAGE_CACHE.as_ref() else {
        return fetch_pinned(policy, url, host, port, kind, max)
            .await
            .map(Arc::new);
    };
    cache
        .try_get_with(src.to_string(), async {
            let bytes = fetch_pinned(policy, url, host, port, kind, max).await?;
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
    kind: Kind,
    max: usize,
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
    // Images are checked by their content type; a font is checked by its first bytes by the caller.
    if kind == Kind::Image {
        let is_image = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.trim().to_ascii_lowercase().starts_with("image/"));
        if !is_image {
            return Err(Skip::Blocked("response is not an image"));
        }
    }
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(Skip::Blocked("file too large"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| Skip::Failed(e.to_string()))?
    {
        if body.len() + chunk.len() > max {
            return Err(Skip::Blocked("file too large"));
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
    use crate::image_util::decode_data_uri;

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
