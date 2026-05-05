// crates/ocis-client/src/auth/oidc.rs
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{OcisError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer_url: Url,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: Url,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
}

/// Opaque PKCE code verifier — must be kept secret until code exchange.
#[derive(Debug, Clone)]
pub struct PkceVerifier(pub(crate) String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) when `access_token` expires.
    pub expires_at: i64,
}

impl TokenSet {
    /// Returns `true` if the access token has expired (or expires within 30 s).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now >= self.expires_at - 30
    }
}

#[derive(Debug, Deserialize)]
struct DiscoveryDocument {
    #[allow(dead_code)]
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

/// Stateless helper for performing OIDC PKCE flows against an oCIS server.
#[derive(Debug, Clone)]
pub struct OidcAuth {
    config: OidcConfig,
    http: Client,
}

impl OidcAuth {
    /// Fetch the OIDC discovery document and build an [`OidcAuth`] instance.
    ///
    /// Set `insecure = true` to accept self-signed / invalid TLS certificates (testing only).
    pub async fn discover(
        issuer: &str,
        client_id: impl Into<String>,
        client_secret: Option<String>,
        redirect_uri: impl Into<String>,
        insecure: bool,
    ) -> Result<Self> {
        let issuer_url: Url = issuer
            .parse()
            .map_err(|e: url::ParseError| OcisError::Auth(e.to_string()))?;

        let discovery_url: Url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        )
        .parse()
        .map_err(|e: url::ParseError| OcisError::Auth(e.to_string()))?;

        let env_insecure = std::env::var("OCIS_INSECURE")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
        let http = Client::builder()
            .danger_accept_invalid_certs(insecure || env_insecure)
            .build()
            .map_err(OcisError::Http)?;

        let doc: DiscoveryDocument = http
            .get(discovery_url)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?
            .json()
            .await
            .map_err(OcisError::Http)?;

        let redirect_uri_url: Url = redirect_uri
            .into()
            .parse()
            .map_err(|e: url::ParseError| OcisError::Auth(e.to_string()))?;

        let config = OidcConfig {
            issuer_url,
            client_id: client_id.into(),
            client_secret,
            redirect_uri: redirect_uri_url,
            authorization_endpoint: doc.authorization_endpoint,
            token_endpoint: doc.token_endpoint,
        };

        Ok(Self { config, http })
    }

    /// Build an authorization URL for the PKCE flow.
    ///
    /// Returns `(authorization_url, verifier)`.
    pub fn start_pkce_flow(&self) -> Result<(Url, PkceVerifier)> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use sha2::{Digest, Sha256};

        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).map_err(|e| OcisError::Auth(format!("RNG failure: {e}")))?;

        let verifier = URL_SAFE_NO_PAD.encode(raw);

        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(&hash[..]);

        let state = {
            let mut raw_state = [0u8; 16];
            getrandom::fill(&mut raw_state)
                .map_err(|e| OcisError::Auth(format!("RNG failure: {e}")))?;
            URL_SAFE_NO_PAD.encode(raw_state)
        };

        let mut auth_url = self.config.authorization_endpoint.clone();
        {
            let mut q = auth_url.query_pairs_mut();
            q.append_pair("response_type", "code");
            q.append_pair("client_id", &self.config.client_id);
            q.append_pair("redirect_uri", self.config.redirect_uri.as_str());
            q.append_pair("scope", "openid profile email offline_access");
            q.append_pair("code_challenge", &challenge);
            q.append_pair("code_challenge_method", "S256");
            q.append_pair("state", &state);
        }

        Ok((auth_url, PkceVerifier(verifier)))
    }

    /// Exchange an authorization `code` for a [`TokenSet`] using PKCE.
    pub async fn exchange_code(&self, code: &str, verifier: PkceVerifier) -> Result<TokenSet> {
        let mut params = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("client_id", self.config.client_id.as_str()),
            ("code_verifier", verifier.0.as_str()),
        ];
        if let Some(ref secret) = self.config.client_secret {
            params.push(("client_secret", secret.as_str()));
        }

        let resp: TokenResponse = self
            .http
            .post(self.config.token_endpoint.clone())
            .form(&params)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?
            .json()
            .await
            .map_err(OcisError::Http)?;

        Ok(token_response_to_set(resp))
    }

    /// Use a `refresh_token` to obtain a fresh [`TokenSet`].
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenSet> {
        let mut params = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", self.config.client_id.as_str()),
        ];
        if let Some(ref secret) = self.config.client_secret {
            params.push(("client_secret", secret.as_str()));
        }

        let resp: TokenResponse = self
            .http
            .post(self.config.token_endpoint.clone())
            .form(&params)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?
            .json()
            .await
            .map_err(OcisError::Http)?;

        Ok(token_response_to_set(resp))
    }
}

fn token_response_to_set(resp: TokenResponse) -> TokenSet {
    let expires_at = {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now + resp.expires_in.unwrap_or(3600) as i64
    };
    TokenSet {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at,
    }
}
