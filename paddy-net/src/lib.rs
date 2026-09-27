#![forbid(unsafe_code)]
//! The only network code in paddy: small, size-limited, HTTPS-only downloads.
//!
//! Rules enforced here (each has a test):
//! * HTTPS only, no credentials in the URL, sane length, no control characters.
//! * The connection itself refuses loopback, private, link-local (cloud
//!   metadata), CGNAT, multicast and other non-public addresses, including on
//!   redirects, because the check sits in the resolver that ureq must use.
//! * Proxy environment variables are ignored: always a direct connection.
//! * Hard body size limit and overall timeout; at most 3 redirects; no cookies,
//!   no request body, a fixed User-Agent. Nothing about the user is ever sent.
//! * Optional SHA-256 pin: a mismatch discards the bytes.
//!
//! Downloaded bytes are only ever treated as data by the callers.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use ureq::Agent;

pub use paddy_core::sha256_hex;

pub const MAX_URL_LEN: usize = 2048;

#[derive(Debug, Clone)]
pub struct Policy {
    pub https_only: bool,
    /// Allow loopback/private addresses (for labs hosting their own packs). Off by default.
    pub allow_private_hosts: bool,
    pub timeout: Duration,
    pub max_redirects: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Self { https_only: true, allow_private_hosts: false, timeout: Duration::from_secs(20), max_redirects: 3 }
    }
}

#[derive(Debug)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NetError {
    BadUrl(String),
    NotHttps,
    /// Points at (or redirected to) an address that is not on the public internet.
    PrivateAddress(String),
    TooBig {
        limit: u64,
    },
    Status(u16),
    Hash {
        expected: String,
        actual: String,
    },
    Other(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::BadUrl(why) => write!(f, "bad URL: {why}"),
            NetError::NotHttps => write!(f, "only https:// URLs are allowed"),
            NetError::PrivateAddress(a) => write!(f, "refusing to connect to non-public address {a}"),
            NetError::TooBig { limit } => write!(f, "download is larger than the {limit} byte limit"),
            NetError::Status(c) => write!(f, "server answered with status {c}"),
            NetError::Hash { .. } => write!(f, "checksum mismatch: the file is not what the catalog expects"),
            NetError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for NetError {}

/// Is this address on the public internet? Everything else is refused by default.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            // IPv4-mapped / compatible addresses take the IPv4 rules.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_v4(v4);
            }
            let s = v6.segments();
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] & 0xffc0) == 0xfec0 // deprecated site-local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64: can reach v4 private space
                || v6 == Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0))
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local() // 169.254/16 incl. cloud metadata 169.254.169.254
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // benchmarking 198.18/15
        || o[0] >= 240) // reserved
}

/// Resolver that only hands out addresses the policy allows. Because ureq
/// connects to exactly what the resolver returns (also for redirects), there is
/// no gap between "checked" and "connected" for DNS rebinding to slip through.
#[derive(Debug)]
struct SafeResolver {
    inner: DefaultResolver,
    allow_private: bool,
}

impl Resolver for SafeResolver {
    fn resolve(&self, uri: &Uri, config: &Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let all = self.inner.resolve(uri, config, timeout)?;
        if self.allow_private {
            return Ok(all);
        }
        let mut allowed = self.empty();
        let mut first_blocked = None;
        for a in all.iter() {
            if is_public_ip(a.ip()) {
                allowed.push(*a);
            } else if first_blocked.is_none() {
                first_blocked = Some(a.ip());
            }
        }
        if allowed.is_empty() {
            // Surface as an IO error carrying a marker we translate back to PrivateAddress.
            let ip = first_blocked.map_or_else(|| "?".to_string(), |i| i.to_string());
            return Err(ureq::Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{PRIVATE_MARK}{ip}"),
            )));
        }
        Ok(allowed)
    }
}

const PRIVATE_MARK: &str = "paddy-net: non-public address ";

/// Validate a URL string before it goes anywhere near the network.
pub fn check_url(url: &str, policy: &Policy) -> Result<(), NetError> {
    let bad = |why: &str| Err(NetError::BadUrl(why.into()));
    if url.is_empty() || url.len() > MAX_URL_LEN {
        return bad("empty or longer than 2048 characters");
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace() || paddy_core::is_deceptive(c)) {
        return bad("contains whitespace or control characters");
    }
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if policy.https_only {
        return Err(NetError::NotHttps);
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else {
        return bad("expected an https:// URL");
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return bad("missing host");
    }
    if authority.contains('@') {
        return bad("credentials in URLs are not allowed");
    }
    Ok(())
}

fn agent(policy: &Policy) -> Agent {
    let config = Agent::config_builder()
        // Ignore HTTP(S)_PROXY / ALL_PROXY from the environment: through a proxy the
        // address check would see the proxy, not the real server (and a local Burp
        // proxy on 127.0.0.1 would make every download fail). Always connect directly.
        .proxy(None)
        .https_only(policy.https_only)
        .max_redirects(policy.max_redirects)
        .timeout_global(Some(policy.timeout))
        .timeout_connect(Some(Duration::from_secs(8).min(policy.timeout)))
        .user_agent(concat!("paddy/", env!("CARGO_PKG_VERSION"), " (pack download)"))
        .max_response_header_size(16 * 1024)
        .build();
    Agent::with_parts(
        config,
        DefaultConnector::new(),
        SafeResolver { inner: DefaultResolver::default(), allow_private: policy.allow_private_hosts },
    )
}

fn translate(e: ureq::Error) -> NetError {
    if let ureq::Error::Io(io) = &e {
        if let Some(ip) = io.to_string().strip_prefix(PRIVATE_MARK) {
            return NetError::PrivateAddress(ip.to_string());
        }
    }
    match e {
        ureq::Error::StatusCode(c) => NetError::Status(c),
        ureq::Error::BodyExceedsLimit(limit) => NetError::TooBig { limit },
        ureq::Error::Timeout(_) => NetError::Other("timed out".into()),
        ureq::Error::HostNotFound => NetError::Other("host not found".into()),
        other => NetError::Other(other.to_string()),
    }
}

/// Download `url` into memory, refusing anything over `max_bytes`.
pub fn fetch(url: &str, max_bytes: u64, policy: &Policy) -> Result<Fetched, NetError> {
    check_url(url, policy)?;
    let mut resp = agent(policy).get(url).call().map_err(translate)?;
    // Trust the declared length only to fail fast; the read below enforces the limit.
    if let Some(len) =
        resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok())
    {
        if len > max_bytes {
            return Err(NetError::TooBig { limit: max_bytes });
        }
    }
    let bytes = resp.body_mut().with_config().limit(max_bytes).read_to_vec().map_err(translate)?;
    let sha256 = sha256_hex(&bytes);
    Ok(Fetched { bytes, sha256 })
}

/// Like [`fetch`], but the bytes must hash to `expected_sha256`.
pub fn fetch_verified(url: &str, expected_sha256: &str, max_bytes: u64, policy: &Policy) -> Result<Vec<u8>, NetError> {
    let got = fetch(url, max_bytes, policy)?;
    if !got.sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(NetError::Hash { expected: expected_sha256.to_string(), actual: got.sha256 });
    }
    Ok(got.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn public_and_private_addresses() {
        for public in [
            "8.8.8.8",
            "1.1.1.1",
            "140.82.112.3",
            "185.199.108.133",
            "2606:4700:4700::1111",
            "2a00:1450:4001:81b::200e",
        ] {
            assert!(is_public_ip(ip(public)), "{public} should be public");
        }
        for private in [
            "127.0.0.1",
            "127.255.255.254",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "224.0.0.1",
            "240.0.0.1",
            "192.0.2.1",
            "198.51.100.7",
            "203.0.113.9",
            "198.18.0.1",
            "192.0.0.5",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public_ip(ip(private)), "{private} must be refused");
        }
        assert!(
            is_public_ip(ip("172.15.255.255")) && is_public_ip(ip("172.32.0.1")),
            "just outside 172.16/12 is public"
        );
        assert!(is_public_ip(ip("100.63.255.255")) && is_public_ip(ip("100.128.0.1")), "just outside CGNAT is public");
    }

    #[test]
    fn url_checks() {
        let p = Policy::default();
        assert!(check_url("https://raw.githubusercontent.com/a/b/c.txt", &p).is_ok());
        assert!(check_url("HTTPS://Example.com/x?y=1#z", &p).is_ok());
        for (url, want) in [
            ("http://example.com/x", NetError::NotHttps),
            ("ftp://example.com/x", NetError::NotHttps),
            ("file:///etc/passwd", NetError::NotHttps),
            ("//example.com/x", NetError::NotHttps),
        ] {
            assert_eq!(check_url(url, &p), Err(want), "{url}");
        }
        for url in [
            "",
            "https://",
            "https:///path",
            "https://user:pw@example.com/",
            "https://example.com@evil.com/",
            "https://exa mple.com/",
            "https://example.com/\n",
            "https://example.com/\u{202e}x",
            "https://example.com/\u{0}",
        ] {
            assert!(matches!(check_url(url, &p), Err(NetError::BadUrl(_))), "{url:?}");
        }
        assert!(matches!(
            check_url(&format!("https://e.com/{}", "a".repeat(MAX_URL_LEN)), &p),
            Err(NetError::BadUrl(_))
        ));
        // relaxed policy (tests / labs) accepts plain http, but still no credentials
        let lab = Policy { https_only: false, ..Policy::default() };
        assert!(check_url("http://10.0.0.5/pack.txt", &lab).is_ok());
        assert!(check_url("http://u:p@10.0.0.5/", &lab).is_err());
    }

    #[test]
    fn private_hosts_are_refused_before_any_connection() {
        let lab = Policy { https_only: false, ..Policy::default() }; // http allowed, private still blocked
        for url in [
            "http://127.0.0.1:9/x",
            "http://localhost:9/x",
            "http://[::1]:9/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.1.2.3/x",
            "http://192.168.0.1/x",
            "http://[::ffff:127.0.0.1]/x",
            "http://0.0.0.0:9/x",
        ] {
            match fetch(url, 1000, &lab) {
                Err(NetError::PrivateAddress(_)) => {}
                other => panic!("{url}: expected PrivateAddress, got {other:?}"),
            }
        }
    }

    /// One-shot HTTP server for behavior tests. `respond` gets the raw request and returns the raw response.
    fn serve(handler: impl Fn(&str) -> Vec<u8> + Send + Sync + 'static) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                h.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let resp = handler(&req);
                let _ = s.write_all(&resp);
                let _ = s.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), hits)
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut v =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        v.extend_from_slice(body);
        v
    }

    fn lab() -> Policy {
        Policy { https_only: false, allow_private_hosts: true, timeout: Duration::from_secs(3), max_redirects: 3 }
    }

    #[test]
    fn downloads_bytes_and_hashes_them() {
        let (base, _) = serve(|_| ok(b"hello pack"));
        let got = fetch(&format!("{base}/a"), 1000, &lab()).unwrap();
        assert_eq!(got.bytes, b"hello pack");
        assert_eq!(got.sha256, sha256_hex(b"hello pack"));
    }

    #[test]
    fn sends_nothing_identifying() {
        let (base, _) = serve(|req| {
            // the server echoes the request it saw back as the body
            ok(req.as_bytes())
        });
        let got = fetch(&format!("{base}/pack.txt"), 10_000, &lab()).unwrap();
        let req = String::from_utf8(got.bytes).unwrap().to_lowercase();
        assert!(req.starts_with("get /pack.txt http/1.1"), "{req}");
        assert!(req.contains("user-agent: paddy/"), "{req}");
        for forbidden in ["cookie", "authorization", "referer", "x-", "content-length"] {
            assert!(!req.contains(forbidden), "request leaked {forbidden}: {req}");
        }
    }

    #[test]
    fn enforces_the_size_limit_with_and_without_content_length() {
        let (base, _) = serve(|_| ok(&vec![b'x'; 5000]));
        assert!(
            matches!(fetch(&format!("{base}/a"), 1000, &lab()), Err(NetError::TooBig { .. })),
            "declared length too big"
        );
        // a server that lies about (omits) the length still cannot exceed the limit
        let (base, _) = serve(|_| {
            let mut v = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
            v.extend(vec![b'y'; 100_000]);
            v
        });
        assert!(
            matches!(fetch(&format!("{base}/a"), 1000, &lab()), Err(NetError::TooBig { .. })),
            "streamed body too big"
        );
        assert!(fetch(&format!("{base}/a"), 200_000, &lab()).is_ok());
    }

    #[test]
    fn http_errors_and_timeouts_are_errors_not_data() {
        let (base, _) =
            serve(|_| b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope".to_vec());
        assert_eq!(fetch(&format!("{base}/a"), 1000, &lab()).unwrap_err(), NetError::Status(404));
        let (base, _) = serve(|_| {
            std::thread::sleep(Duration::from_secs(6));
            ok(b"late")
        });
        let quick = Policy { timeout: Duration::from_millis(600), ..lab() };
        let t = std::time::Instant::now();
        assert!(fetch(&format!("{base}/a"), 1000, &quick).is_err());
        assert!(t.elapsed() < Duration::from_secs(4), "gave up in time: {:?}", t.elapsed());
    }

    #[test]
    fn redirects_are_limited() {
        let (base, hits) = serve({
            move |req| {
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let n: u32 = path.trim_start_matches('/').parse().unwrap_or(0);
                format!("HTTP/1.1 302 Found\r\nLocation: /{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", n + 1)
                    .into_bytes()
            }
        });
        let r = fetch(&format!("{base}/0"), 1000, &lab());
        assert!(r.is_err(), "endless redirects must stop");
        assert!(hits.load(Ordering::SeqCst) <= 6, "followed {} times", hits.load(Ordering::SeqCst));
    }

    #[test]
    fn a_redirect_into_private_space_is_blocked() {
        // public-policy client, server on loopback can't even be reached, so test the resolver rule directly
        let r = SafeResolver { inner: DefaultResolver::default(), allow_private: false };
        let uri: Uri = "https://127.0.0.1:1/x".parse().unwrap();
        let cfg = Agent::config_builder().build();
        let res = r.resolve(
            &uri,
            &cfg,
            NextTimeout {
                after: ureq::unversioned::transport::time::Duration::NotHappening,
                reason: ureq::Timeout::Global,
            },
        );
        assert!(matches!(res, Err(ureq::Error::Io(ref e)) if e.to_string().starts_with(PRIVATE_MARK)), "{res:?}");
    }

    #[test]
    fn proxy_environment_variables_are_ignored() {
        // Serialize env mutation across all tests in this binary.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let (proxy, proxy_hits) = serve(|_| ok(b"FROM THE PROXY"));
        let (real, _) = serve(|_| ok(b"the real file"));
        let proxy_vars = ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "HTTPS_PROXY", "https_proxy"];
        for var in proxy_vars {
            std::env::set_var(var, &proxy);
        }
        let got = fetch(&format!("{real}/a"), 1000, &lab());
        for var in proxy_vars {
            std::env::remove_var(var);
        }
        assert_eq!(got.unwrap().bytes, b"the real file");
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 0, "the proxy was never contacted");
    }

    #[test]
    fn hash_pin_rejects_a_swapped_file() {
        let (base, _) = serve(|_| ok(b"the real file"));
        let good = sha256_hex(b"the real file");
        assert_eq!(fetch_verified(&format!("{base}/a"), &good, 1000, &lab()).unwrap(), b"the real file");
        let err = fetch_verified(&format!("{base}/a"), &sha256_hex(b"something else"), 1000, &lab()).unwrap_err();
        assert!(matches!(err, NetError::Hash { .. }), "{err:?}");
        // the pin is case-insensitive hex
        assert!(fetch_verified(&format!("{base}/a"), &good.to_uppercase(), 1000, &lab()).is_ok());
    }
}
