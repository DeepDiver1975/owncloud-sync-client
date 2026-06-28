// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

// crates/ocis-client/src/webfinger.rs
//
// RFC 7033 WebFinger resolution for multi-tenant ownCloud / oCIS deployments.
//
// Some deployments do not host the server at the URL the user enters. Instead
// the entered host is a thin WebFinger endpoint (`drive.example.org`) that maps
// an account to the *actual* server instance (`abc.drive.example.org`). On such
// a host both `/.well-known/openid-configuration` and `status.php` return HTML
// rather than JSON, so OIDC discovery and the status probe both fail with an
// opaque "error decoding response body", which the GUI surfaces as
// "not a reachable ownCloud instance" (issue #80).
//
// To support these deployments we query WebFinger *first* and, when it points at
// a concrete server-instance, run discovery/probe against that resolved URL.
// Single-instance servers that do not implement WebFinger (404 / non-JRD) fall
// back to the entered URL unchanged, so the existing happy path is preserved.

use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::error::{OcisError, Result};

/// Link relation oCIS advertises for the concrete server instance backing an
/// account. See <https://owncloud.dev/services/webfinger/>.
const REL_SERVER_INSTANCE: &str = "http://webfinger.owncloud/rel/server-instance";

/// A JSON Resource Descriptor (RFC 7033 §4.4).
#[derive(Debug, Deserialize)]
struct Jrd {
    #[serde(default)]
    links: Vec<JrdLink>,
}

#[derive(Debug, Deserialize)]
struct JrdLink {
    #[serde(default)]
    rel: String,
    #[serde(default)]
    href: Option<String>,
}

/// Pick the first usable `server-instance` href from a parsed JRD, validated
/// against the host the user actually entered (`base`).
///
/// A WebFinger host fully controls the advertised `href`, so we do NOT follow it
/// blindly — that would let a (possibly MITM'd or only loosely-trusted) base host
/// redirect the OAuth flow and bearer tokens to an arbitrary domain, or downgrade
/// the carefully-enforced `https` to cleartext `http`. We therefore only accept a
/// resolved instance that is:
///   * `https` (never downgrade), and
///   * within the entered host's registrable domain — i.e. the instance host is
///     equal to, or a subdomain of, the entered host's domain. This covers the
///     real oCIS use case (`drive.example.org` -> `abc.drive.example.org`) while
///     rejecting `drive.example.org` -> `evil.attacker.tld`.
fn select_server_instance(base: &Url, jrd: &Jrd) -> Option<Url> {
    let base_host = base.host_str()?;
    jrd.links
        .iter()
        .filter(|l| l.rel == REL_SERVER_INSTANCE)
        .filter_map(|l| l.href.as_deref())
        .filter_map(|href| Url::parse(href).ok())
        .find(|u| u.scheme() == "https" && instance_host_is_trusted(base_host, u))
}

/// True when `instance`'s host is within the same registrable domain as
/// `base_host`: an exact match, or a subdomain of `base_host`'s registrable
/// domain (approximated as its last two labels, which is correct for the
/// `*.example.org` deployments this feature targets).
fn instance_host_is_trusted(base_host: &str, instance: &Url) -> bool {
    let Some(inst_host) = instance.host_str() else {
        return false;
    };
    if inst_host.eq_ignore_ascii_case(base_host) {
        return true;
    }
    let base_domain = registrable_domain(base_host);
    let inst_lower = inst_host.to_ascii_lowercase();
    // Subdomain of, or equal to, the entered host's registrable domain.
    inst_lower == base_domain || inst_lower.ends_with(&format!(".{base_domain}"))
}

/// Approximate the registrable domain ("eTLD+1") as the last two dot-separated
/// labels, lowercased. Without a full Public Suffix List this is a deliberately
/// conservative heuristic: it keeps the same-domain check tight for ordinary
/// `host.tld` deployments. (Hosts under multi-label public suffixes like
/// `co.uk` would be treated as their last two labels; the cost is only that a
/// legitimate sibling instance there falls back to the entered URL rather than
/// being followed — it never widens trust.)
fn registrable_domain(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        host
    } else {
        labels[labels.len() - 2..].join(".")
    }
}

/// Build the WebFinger query URL for `base`: `{scheme}://{authority}/.well-known/webfinger`.
fn webfinger_endpoint(base: &Url) -> Result<Url> {
    // Always query the host's root .well-known, regardless of any path the user
    // may have typed, per RFC 7033 §4.
    let mut ep = base.clone();
    ep.set_path("/.well-known/webfinger");
    ep.set_query(None);
    ep.set_fragment(None);
    Ok(ep)
}

/// Resolve the entered `base_url` to a concrete ownCloud/oCIS server instance via
/// RFC 7033 WebFinger.
///
/// Returns:
/// - `Ok(Some(url))` when WebFinger advertises a `server-instance` link — query
///   discovery/probe against `url`.
/// - `Ok(None)` when the host does not implement WebFinger usefully (404, a
///   non-JRD/HTML body, or a JRD without a usable `server-instance` link). The
///   caller should fall back to `base_url` unchanged. A non-JSON body is treated
///   as "no WebFinger here" rather than a hard error, so deployments that serve
///   HTML at `/.well-known/webfinger` do not break account setup.
/// - `Err(_)` only for a transport-level failure (DNS, TLS, connection refused) —
///   i.e. the host is unreachable, which is worth surfacing.
///
/// Set `insecure = true` (or `OCIS_INSECURE=1`) to accept invalid TLS certs.
pub async fn resolve_server_instance(base_url: &Url, insecure: bool) -> Result<Option<Url>> {
    let env_insecure = std::env::var("OCIS_INSECURE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let client = Client::builder()
        .danger_accept_invalid_certs(insecure || env_insecure)
        .build()
        .map_err(OcisError::Http)?;

    resolve_with_client(&client, base_url).await
}

/// Inner resolution against an explicit client (kept separate so tests can point
/// it at a mock server).
async fn resolve_with_client(client: &Client, base_url: &Url) -> Result<Option<Url>> {
    let endpoint = webfinger_endpoint(base_url)?;

    // oCIS WebFinger accepts the instance domain URI as the resource (an `acct:`
    // URI also works, but we have no username at account-add time).
    let resp = match client
        .get(endpoint)
        .query(&[("resource", base_url.as_str())])
        .header(reqwest::header::ACCEPT, "application/jrd+json")
        .send()
        .await
    {
        Ok(r) => r,
        // Transport-level failure (DNS/TLS/refused): the host is unreachable.
        Err(e) => return Err(OcisError::Http(e)),
    };

    // A 404/410 (or any non-success) means "no WebFinger here" → fall back.
    if !resp.status().is_success() {
        tracing::debug!(
            status = %resp.status(),
            "WebFinger endpoint returned non-success; falling back to entered URL"
        );
        return Ok(None);
    }

    // Read the body as text so an HTML response (common on multi-tenant base
    // hosts) degrades to a clean fallback instead of an opaque decode error.
    let body = resp.text().await.map_err(OcisError::Http)?;

    match parse_server_instance(base_url, &body) {
        Some(url) => {
            tracing::info!(%url, "WebFinger resolved server instance");
            Ok(Some(url))
        }
        None => {
            tracing::debug!("WebFinger response had no usable server-instance link; falling back");
            Ok(None)
        }
    }
}

/// Parse a JRD body and extract the server-instance URL, if any.
///
/// Returns `None` for a non-JSON/HTML body, a JRD that lacks a usable
/// `server-instance` link, or a link that fails the trust check against `base` —
/// all mean "fall back to the entered URL".
fn parse_server_instance(base: &Url, body: &str) -> Option<Url> {
    let jrd: Jrd = serde_json::from_str(body).ok()?;
    select_server_instance(base, &jrd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        // The host the user entered for all parsing tests below.
        Url::parse("https://drive.example.org/").unwrap()
    }

    #[test]
    fn parses_server_instance_link() {
        let body = r#"{
            "subject": "acct:einstein@drive.example.org",
            "links": [
                { "rel": "http://openid.net/specs/connect/1.0/issuer",
                  "href": "https://sso.example.org/cas/oidc/" },
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://abc.drive.example.org",
                  "titles": { "en": "oCIS Instance" } }
            ]
        }"#;
        let url = parse_server_instance(&base(), body).expect("should resolve a server instance");
        assert_eq!(url.as_str(), "https://abc.drive.example.org/");
    }

    #[test]
    fn picks_first_in_domain_server_instance_when_multiple() {
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://first.example.org" },
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://second.example.org" }
            ]
        }"#;
        let url = parse_server_instance(&base(), body).unwrap();
        assert_eq!(url.as_str(), "https://first.example.org/");
    }

    #[test]
    fn html_response_falls_back_to_none() {
        // A multi-tenant base host that serves an HTML landing page rather than a
        // JRD must NOT raise a decode error — it degrades to None (fall back).
        let body = "<!DOCTYPE html><html><head><title>Welcome</title></head></html>";
        assert!(parse_server_instance(&base(), body).is_none());
    }

    #[test]
    fn jrd_without_server_instance_falls_back_to_none() {
        let body = r#"{
            "subject": "acct:einstein@drive.example.org",
            "links": [
                { "rel": "http://openid.net/specs/connect/1.0/issuer",
                  "href": "https://sso.example.org/" }
            ]
        }"#;
        assert!(parse_server_instance(&base(), body).is_none());
    }

    #[test]
    fn ignores_server_instance_with_non_http_scheme() {
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "ftp://weird.example.org" }
            ]
        }"#;
        assert!(parse_server_instance(&base(), body).is_none());
    }

    #[test]
    fn rejects_cross_domain_server_instance() {
        // A WebFinger host must NOT be able to redirect the OAuth/token flow to an
        // unrelated domain — that link is ignored and we fall back to the entered
        // URL (returns None here).
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://evil.attacker.tld" }
            ]
        }"#;
        assert!(parse_server_instance(&base(), body).is_none());
    }

    #[test]
    fn rejects_http_scheme_downgrade() {
        // The entered URL is forced to https; a JRD must not downgrade the flow to
        // cleartext http even within the same domain.
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "http://abc.drive.example.org" }
            ]
        }"#;
        assert!(parse_server_instance(&base(), body).is_none());
    }

    #[test]
    fn skips_cross_domain_link_and_picks_in_domain_one() {
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://evil.attacker.tld" },
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://abc.drive.example.org" }
            ]
        }"#;
        let url = parse_server_instance(&base(), body).unwrap();
        assert_eq!(url.as_str(), "https://abc.drive.example.org/");
    }

    #[test]
    fn accepts_exact_host_match() {
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://drive.example.org" }
            ]
        }"#;
        let url = parse_server_instance(&base(), body).unwrap();
        assert_eq!(url.as_str(), "https://drive.example.org/");
    }

    #[test]
    fn registrable_domain_uses_last_two_labels() {
        assert_eq!(registrable_domain("abc.drive.example.org"), "example.org");
        assert_eq!(registrable_domain("example.org"), "example.org");
        assert_eq!(registrable_domain("localhost"), "localhost");
        assert_eq!(registrable_domain("Drive.Example.ORG"), "example.org");
    }

    #[test]
    fn server_instance_link_without_href_is_skipped() {
        let body = r#"{
            "links": [
                { "rel": "http://webfinger.owncloud/rel/server-instance" },
                { "rel": "http://webfinger.owncloud/rel/server-instance",
                  "href": "https://fallback.example.org" }
            ]
        }"#;
        let url = parse_server_instance(&base(), body).unwrap();
        assert_eq!(url.as_str(), "https://fallback.example.org/");
    }

    #[test]
    fn empty_body_falls_back_to_none() {
        assert!(parse_server_instance(&base(), "").is_none());
    }

    #[test]
    fn webfinger_endpoint_uses_host_root_and_drops_path_query() {
        let base = Url::parse("https://drive.example.org/some/path?foo=bar#frag").unwrap();
        let ep = webfinger_endpoint(&base).unwrap();
        assert_eq!(
            ep.as_str(),
            "https://drive.example.org/.well-known/webfinger"
        );
    }

    #[test]
    fn webfinger_endpoint_preserves_port() {
        let base = Url::parse("https://drive.example.org:8443/").unwrap();
        let ep = webfinger_endpoint(&base).unwrap();
        assert_eq!(
            ep.as_str(),
            "https://drive.example.org:8443/.well-known/webfinger"
        );
    }
}
