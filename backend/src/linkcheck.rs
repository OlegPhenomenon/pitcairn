//! External-link availability check (§5 item 11, architecture §2.1).
//!
//! When results stay in a university repository the team hands in an
//! address; the coordinator must see when it stops working. Outcomes:
//!
//! | status           | meaning                                                   |
//! |------------------|-----------------------------------------------------------|
//! | `available`      | 2xx after following (re-validated) redirects              |
//! | `missing`        | HTTP 404 / 410 — the file was deleted or moved            |
//! | `unreachable`    | DNS failure, timeout, refused connection, TLS error, 5xx, |
//! |                  | too many redirects, other 4xx, or refused by the SSRF guard|
//! | `login_required` | HTTP 401 / 403 / 407 or a redirect to a login page — NOT   |
//! |                  | data loss (closed access may have been agreed)             |
//! | `unchecked`      | never checked yet                                          |
//!
//! Modes (`PITCAIRN_LINK_CHECK_MODE`): `live` (default) fetches every URL
//! except reserved demo hosts (`*.invalid`, `*.example`, `*.test`,
//! `example.org/com/net`), which keep the deterministic mock so seeded demo
//! examples stay stable; `mock` uses the mock for every URL.
//!
//! SSRF guard (live): only `http(s)`; every hop's host is resolved here and
//! refused when any address is loopback/private/link-local/unique-local/
//! multicast/unspecified (unless `PITCAIRN_LINK_CHECK_ALLOW_PRIVATE=true`,
//! a test-only setting); the connection is pinned to the vetted addresses so
//! a second DNS answer cannot redirect it; redirects are followed manually
//! (at most [`MAX_REDIRECTS`]) and each target is validated again.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::header::{LOCATION, RANGE};
use reqwest::{StatusCode, Url};

pub const MODE_MOCK: &str = "mock";
pub const MODE_LIVE: &str = "live";

/// Per-request (connect + response headers) timeout.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper bound for one whole check including redirects.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REDIRECTS: usize = 5;

const USER_AGENT: &str = "PitcairnLinkCheck/1.0 (research data availability check)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    Unchecked,
    Available,
    Missing,
    Unreachable,
    LoginRequired,
}

impl LinkStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkStatus::Unchecked => "unchecked",
            LinkStatus::Available => "available",
            LinkStatus::Missing => "missing",
            LinkStatus::Unreachable => "unreachable",
            LinkStatus::LoginRequired => "login_required",
        }
    }

    pub fn parse(value: &str) -> LinkStatus {
        match value {
            "available" => LinkStatus::Available,
            "missing" => LinkStatus::Missing,
            "unreachable" => LinkStatus::Unreachable,
            "login_required" => LinkStatus::LoginRequired,
            _ => LinkStatus::Unchecked,
        }
    }

    /// Data may be lost: the coordinator is alerted. `login_required` is
    /// deliberately not lost — closed access can be the agreed arrangement.
    pub fn is_lost(self) -> bool {
        matches!(self, LinkStatus::Missing | LinkStatus::Unreachable)
    }
}

/// One check outcome, persisted on `external_links`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCheck {
    pub status: LinkStatus,
    /// Final HTTP status code when a response was received.
    pub http_status: Option<u16>,
    /// Short human-readable reason shown to the coordinator.
    pub reason: String,
}

impl LinkCheck {
    fn new(status: LinkStatus, http_status: Option<u16>, reason: impl Into<String>) -> Self {
        LinkCheck {
            status,
            http_status,
            reason: reason.into(),
        }
    }

    fn unreachable(reason: impl Into<String>) -> Self {
        LinkCheck::new(LinkStatus::Unreachable, None, reason)
    }
}

/// Check one URL. `mode` is `PITCAIRN_LINK_CHECK_MODE`; anything other than
/// `mock` runs the live check.
pub async fn check(url: &str, mode: &str, allow_private: bool) -> LinkCheck {
    let Ok(parsed) = Url::parse(url.trim()) else {
        return LinkCheck::unreachable("Not a valid URL");
    };
    if !is_http(&parsed) || parsed.host_str().is_none_or(str::is_empty) {
        return LinkCheck::unreachable("Only http(s) addresses can be checked");
    }
    if mode == MODE_MOCK || parsed.host_str().is_some_and(is_demo_host) {
        return mock_check(&parsed);
    }
    match tokio::time::timeout(TOTAL_TIMEOUT, live_check(parsed, allow_private)).await {
        Ok(outcome) => outcome,
        Err(_) => LinkCheck::unreachable("Check timed out"),
    }
}

/// Shape check used by request validation: only `http(s)` URLs with a host.
pub fn is_http_url(url: &str) -> bool {
    Url::parse(url.trim()).is_ok_and(|u| {
        is_http(&u)
            && u.host_str()
                .is_some_and(|h| !h.is_empty() && h.len() <= 253)
    })
}

fn is_http(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

// ---------------------------------------------------------------------------
// Deterministic mock (demo hosts, or every URL in `mock` mode)
// ---------------------------------------------------------------------------

/// Reserved names (RFC 2606 / RFC 6761) used by the demo data. In `live`
/// mode these keep the mock so prepared demo examples never touch the network.
pub fn is_demo_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let reserved_tld = ["invalid", "example", "test"]
        .iter()
        .any(|tld| host == *tld || host.ends_with(&format!(".{tld}")));
    let example_domain = ["example.org", "example.com", "example.net"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")));
    reserved_tld || example_domain
}

/// The mock never fetches: hosts ending `.invalid` do not resolve, paths
/// containing `/missing` were deleted, paths containing `/restricted` need a
/// login; everything else is available.
fn mock_check(url: &Url) -> LinkCheck {
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let path = url.path();
    if host == "invalid" || host.ends_with(".invalid") {
        LinkCheck::unreachable("Host name could not be resolved (demo check)")
    } else if path.contains("/missing") {
        LinkCheck::new(LinkStatus::Missing, None, "Not found (demo check)")
    } else if path.contains("/restricted") {
        LinkCheck::new(
            LinkStatus::LoginRequired,
            None,
            "Login required (demo check)",
        )
    } else {
        LinkCheck::new(LinkStatus::Available, None, "Reachable (demo check)")
    }
}

// ---------------------------------------------------------------------------
// Live check
// ---------------------------------------------------------------------------

async fn live_check(mut url: Url, allow_private: bool) -> LinkCheck {
    for _ in 0..=MAX_REDIRECTS {
        let pinned = match vet_target(&url, allow_private).await {
            Ok(pinned) => pinned,
            Err(outcome) => return outcome,
        };
        let client = match client_for(pinned.as_ref()) {
            Ok(client) => client,
            Err(_) => return LinkCheck::unreachable("Could not start the check"),
        };
        let response = match fetch(&client, &url).await {
            Ok(response) => response,
            Err(err) => return LinkCheck::unreachable(transport_reason(&err)),
        };
        let code = response.status();
        if !code.is_redirection() {
            return classify(code);
        }
        let Some(next) = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|loc| url.join(loc).ok())
        else {
            return LinkCheck::new(
                LinkStatus::Unreachable,
                Some(code.as_u16()),
                format!("Broken redirect (HTTP {})", code.as_u16()),
            );
        };
        if !is_http(&next) {
            return LinkCheck::new(
                LinkStatus::Unreachable,
                Some(code.as_u16()),
                "Redirects to a non-http(s) address",
            );
        }
        if looks_like_login(&next) {
            return LinkCheck::new(
                LinkStatus::LoginRequired,
                Some(code.as_u16()),
                "Redirects to a login page",
            );
        }
        url = next;
    }
    LinkCheck::unreachable("Too many redirects")
}

/// HEAD first; servers that refuse HEAD (405/501) get a one-byte ranged GET
/// whose body is never read.
async fn fetch(client: &reqwest::Client, url: &Url) -> reqwest::Result<reqwest::Response> {
    let head = client.head(url.clone()).send().await?;
    if matches!(
        head.status(),
        StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
    ) {
        return client
            .get(url.clone())
            .header(RANGE, "bytes=0-0")
            .send()
            .await;
    }
    Ok(head)
}

/// Map a final (non-redirect) HTTP status to an outcome.
pub fn classify(code: StatusCode) -> LinkCheck {
    let n = code.as_u16();
    let (status, reason) = match n {
        200..=299 => (LinkStatus::Available, format!("Reachable (HTTP {n})")),
        // Ranged GET of an empty file: the resource exists.
        416 => (
            LinkStatus::Available,
            format!("Reachable, empty file (HTTP {n})"),
        ),
        401 | 407 => (
            LinkStatus::LoginRequired,
            format!("Login required (HTTP {n})"),
        ),
        403 => (
            LinkStatus::LoginRequired,
            format!("Access restricted (HTTP {n})"),
        ),
        404 => (LinkStatus::Missing, format!("Not found (HTTP {n})")),
        410 => (LinkStatus::Missing, format!("Removed (HTTP {n})")),
        500..=599 => (LinkStatus::Unreachable, format!("Server error (HTTP {n})")),
        _ => (
            LinkStatus::Unreachable,
            format!("Unexpected response (HTTP {n})"),
        ),
    };
    LinkCheck::new(status, Some(n), reason)
}

/// A redirect target that is evidently a sign-in page (institutional SSO,
/// CAS, Shibboleth, OAuth…): the data sits behind a login, not deleted.
/// Matches whole host labels / path segments only (optionally with a web-page
/// extension such as `login.php`) — never substrings, so a redirect to e.g.
/// `/cataloging.csv` is followed and a 404 there is still reported missing.
pub fn looks_like_login(url: &Url) -> bool {
    const HOST_LABELS: [&str; 7] = ["login", "signin", "sso", "auth", "idp", "cas", "accounts"];
    const SEGMENTS: [&str; 18] = [
        "login",
        "logon",
        "signin",
        "sign-in",
        "sign_in",
        "sso",
        "auth",
        "authenticate",
        "authorize",
        "cas",
        "idp",
        "saml",
        "saml2",
        "shibboleth",
        "shibboleth.sso",
        "oauth",
        "oauth2",
        "openid-connect",
    ];
    const PAGE_EXTENSIONS: [&str; 9] = [
        "php", "jsp", "asp", "aspx", "html", "htm", "do", "action", "cgi",
    ];
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if host
        .split('.')
        .next()
        .is_some_and(|label| HOST_LABELS.contains(&label))
    {
        return true;
    }
    url.path_segments().is_some_and(|mut segments| {
        segments.any(|segment| {
            let segment = segment.to_ascii_lowercase();
            let page = match segment.rsplit_once('.') {
                Some((stem, ext)) if PAGE_EXTENSIONS.contains(&ext) => stem,
                _ => segment.as_str(),
            };
            SEGMENTS.contains(&segment.as_str()) || SEGMENTS.contains(&page)
        })
    })
}

/// Resolve and vet the host of `url`. Returns the domain + vetted addresses
/// to pin the connection to (None for IP-literal hosts), or the outcome to
/// report when the target is refused or does not resolve.
async fn vet_target(
    url: &Url,
    allow_private: bool,
) -> Result<Option<(String, Vec<SocketAddr>)>, LinkCheck> {
    let host = url.host_str().unwrap_or("");
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return if !allow_private && is_blocked_ip(ip) {
            Err(refused())
        } else {
            Ok(None)
        };
    }
    if bare.is_empty() {
        return Err(LinkCheck::unreachable(
            "Only http(s) addresses can be checked",
        ));
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs: Vec<SocketAddr> =
        match tokio::time::timeout(REQUEST_TIMEOUT, tokio::net::lookup_host((bare, port))).await {
            Ok(Ok(addrs)) => addrs.collect(),
            Ok(Err(_)) => Vec::new(),
            Err(_) => return Err(LinkCheck::unreachable("Name lookup timed out")),
        };
    if addrs.is_empty() {
        return Err(LinkCheck::unreachable("Host name could not be resolved"));
    }
    if !allow_private && addrs.iter().any(|a| is_blocked_ip(a.ip())) {
        return Err(refused());
    }
    Ok(Some((bare.to_string(), addrs)))
}

fn refused() -> LinkCheck {
    LinkCheck::unreachable("Refused: address is in a private or local network")
}

fn client_for(pinned: Option<&(String, Vec<SocketAddr>)>) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        // A proxy would connect on our behalf and bypass the address vetting.
        .no_proxy();
    if let Some((domain, addrs)) = pinned {
        builder = builder.resolve_to_addrs(domain, addrs);
    }
    builder.build()
}

/// Short reason for a transport-level failure (no HTTP response). Only the
/// error's causes are inspected — the top-level message contains the URL.
fn transport_reason(err: &reqwest::Error) -> &'static str {
    let mut source = std::error::Error::source(err);
    while let Some(e) = source {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionRefused => return "Connection refused",
                std::io::ErrorKind::TimedOut => return "Connection timed out",
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => {
                    return "Connection reset";
                }
                _ => {}
            }
        }
        let text = e.to_string().to_ascii_lowercase();
        if text.contains("certificate") || text.contains("tls") || text.contains("handshake") {
            return "Secure connection (TLS) failed";
        }
        source = e.source();
    }
    if err.is_timeout() {
        "Connection timed out"
    } else if err.is_connect() {
        "Could not connect"
    } else {
        "Request failed"
    }
}

/// Addresses the live check never contacts (SSRF guard).
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || a == 0 // "this network"
        || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT 100.64/10
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4);
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
    fn status_codes_map_to_the_four_outcomes() {
        assert_eq!(classify(StatusCode::OK).status, LinkStatus::Available);
        assert_eq!(
            classify(StatusCode::PARTIAL_CONTENT).status,
            LinkStatus::Available
        );
        assert_eq!(classify(StatusCode::NOT_FOUND).status, LinkStatus::Missing);
        assert_eq!(classify(StatusCode::GONE).status, LinkStatus::Missing);
        assert_eq!(
            classify(StatusCode::UNAUTHORIZED).status,
            LinkStatus::LoginRequired
        );
        assert_eq!(
            classify(StatusCode::FORBIDDEN).status,
            LinkStatus::LoginRequired
        );
        assert_eq!(
            classify(StatusCode::SERVICE_UNAVAILABLE).status,
            LinkStatus::Unreachable
        );
        assert_eq!(
            classify(StatusCode::NOT_FOUND).reason,
            "Not found (HTTP 404)"
        );
        assert!(!LinkStatus::LoginRequired.is_lost());
        assert!(LinkStatus::Missing.is_lost() && LinkStatus::Unreachable.is_lost());
    }

    #[test]
    fn private_and_local_addresses_are_blocked() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "224.0.0.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_blocked_ip(ip.parse().unwrap()), "{ip} must be blocked");
        }
        for ip in ["93.184.216.34", "2001:4860:4860::8888", "::ffff:8.8.8.8"] {
            assert!(!is_blocked_ip(ip.parse().unwrap()), "{ip} is public");
        }
    }

    #[test]
    fn demo_hosts_are_reserved_names_only() {
        for host in [
            "data.example.invalid",
            "repo.pitcairn.invalid",
            "archive.example",
            "files.test",
            "example.org",
            "data.example.com",
            "www.example.net",
        ] {
            assert!(is_demo_host(host), "{host}");
        }
        for host in [
            "zenodo.org",
            "myexample.org",
            "example.org.evil.com",
            "testing.org",
        ] {
            assert!(!is_demo_host(host), "{host}");
        }
    }

    #[test]
    fn login_redirect_targets_are_recognised() {
        for url in [
            "https://idp.uni.example/idp/profile/SAML2/Redirect/SSO",
            "https://idp.example.ac.uk/profile/start?target=x",
            "https://repo.uni.edu/login?next=/data",
            "https://repo.uni.edu/login.php?next=/data",
            "https://repo.uni.edu/Shibboleth.sso/Login",
            "https://login.uni.edu/cas",
            "https://repo.uni.edu/users/sign_in",
        ] {
            assert!(looks_like_login(&Url::parse(url).unwrap()), "{url}");
        }
        // Login words inside other names are not sign-in pages.
        for url in [
            "https://repo.uni.edu/records/12",
            "https://repo.uni.edu/data/blog.csv",
            "https://repo.uni.edu/data/cataloging.csv",
            "https://repo.uni.edu/blogin/notes",
            "https://repo.uni.edu/data/login.csv",
            "https://repo.uni.edu/authors/smith",
            "https://repo.uni.edu/cascade/run1.nc",
            "https://idpx.uni.edu/data",
            "https://catalog.login-free.org/data",
        ] {
            assert!(!looks_like_login(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[tokio::test]
    async fn mock_mode_never_reads_the_network_and_rejects_other_schemes() {
        let missing = check("https://zenodo.org/missing/1", MODE_MOCK, false).await;
        assert_eq!(missing.status, LinkStatus::Missing);
        let ftp = check("ftp://zenodo.org/file", MODE_LIVE, false).await;
        assert_eq!(ftp.status, LinkStatus::Unreachable);
        assert!(is_http_url("https://repo.uni.edu/x"));
        assert!(!is_http_url("ftp://repo.uni.edu/x"));
        assert!(!is_http_url("https://"));
    }
}
