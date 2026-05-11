# Bearer Token Propagation to Download/Upload + Remove OCIS_BASIC_AUTH

**Date:** 2026-05-11
**Status:** Approved

## Problem

`propagate_download` and `propagate_upload` build an unauthenticated HTTP client — they call `ocis_client::build_http_client()` with no `Authorization` header. Every GET, PUT, POST (TUS create), and PATCH (TUS upload) against a real oCIS instance returns `401 Unauthorized`.

The acceptance tests did not catch this because the test daemon was launched with `OCIS_BASIC_AUTH=admin:admin`, which caused `build_http_client()` to inject Basic Auth on every request — masking the missing bearer token entirely.

Additionally, `build_http_client()` contains Basic Auth credential injection via the `OCIS_BASIC_AUTH` environment variable. This is production code that should not contain test-only auth bypass logic.

## Goals

1. Pass the bearer token obtained during discovery into every download and upload HTTP call.
2. Remove `OCIS_BASIC_AUTH` support from `build_http_client()` — no Basic Auth in production code.
3. Remove `OCIS_BASIC_AUTH` from the acceptance test daemon spawn — tests must rely on the real OIDC token path.
4. `build_http_client()` retains the `OCIS_INSECURE` TLS bypass (legitimate for self-signed oCIS dev/test instances).

## Non-Goals

- Refreshing the token mid-sync (tokens are valid for minutes; sync cycles take seconds).
- Changing how `OcisClient` authenticates (it uses `.basic_auth()` directly via reqwest — that is test-harness code, not production sync engine code, and is unaffected).
- Changing `GraphClient` auth (uses `TokenManager` separately).

---

## Architecture

### `DownloadRequest` and `UploadRequest` gain `bearer_token: String`

```rust
// crates/sync-engine/src/propagate/download.rs
pub struct DownloadRequest {
    pub remote_url: Url,
    pub local_dest: Utf8PathBuf,
    pub expected_etag: Option<String>,
    pub bearer_token: String,   // ← new
}

// crates/sync-engine/src/propagate/upload.rs
pub struct UploadRequest {
    pub local_path: Utf8PathBuf,
    pub remote_url: Url,
    pub size: u64,
    pub checksum: Option<String>,
    pub tus_threshold: u64,
    pub bearer_token: String,   // ← new
}
```

### HTTP calls add `.bearer_auth(&req.bearer_token)`

`propagate_download`:
```rust
let resp = client
    .get(req.remote_url.as_str())
    .bearer_auth(&req.bearer_token)   // ← new
    .send()
    .await ...
```

`upload_put` (inside `upload.rs`):
```rust
let resp = client
    .put(req.remote_url.as_str())
    .bearer_auth(&req.bearer_token)   // ← new
    ...
```

`upload_tus` — POST (create) and PATCH (upload) both get `.bearer_auth(&req.bearer_token)`.

### Engine clones token into each request struct

`run_sync` already obtains `bearer_token: String` before discovery. That same value is cloned into each `DownloadRequest` / `UploadRequest` before spawning the task:

```rust
let req = DownloadRequest {
    remote_url,
    local_dest: local_path.clone(),
    expected_etag: None,
    bearer_token: bearer_token.clone(),
};
```

### `build_http_client()` — remove Basic Auth logic

Before:
```rust
pub fn build_http_client() -> reqwest::Client {
    let insecure = std::env::var("OCIS_INSECURE")...;
    let mut builder = reqwest::Client::builder().danger_accept_invalid_certs(insecure);

    if let Ok(basic) = std::env::var("OCIS_BASIC_AUTH") {
        // ... base64 encode, inject default Authorization header
    }

    builder.build().expect("build reqwest client")
}
```

After:
```rust
pub fn build_http_client() -> reqwest::Client {
    let insecure = std::env::var("OCIS_INSECURE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    reqwest::Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .expect("build reqwest client")
}
```

The `base64` dependency in `crates/ocis-client/Cargo.toml` must be removed if it is only used for this Basic Auth encoding.

### Acceptance test fixture — remove `OCIS_BASIC_AUTH`

`crates/acceptance-test/src/fixture.rs`, `spawn_daemon()`:

Remove:
```rust
.env("OCIS_BASIC_AUTH", "admin:admin")
```

Tests now exercise the real OIDC token path through the daemon, catching any future auth regression.

---

## Touched Files

| File | Change |
|---|---|
| `crates/ocis-client/src/lib.rs` | Remove `OCIS_BASIC_AUTH` env-var logic and `base64` usage from `build_http_client()` |
| `crates/ocis-client/Cargo.toml` | Remove `base64` dependency if no longer used elsewhere |
| `crates/sync-engine/src/propagate/download.rs` | Add `bearer_token: String` to `DownloadRequest`; add `.bearer_auth()` to GET call |
| `crates/sync-engine/src/propagate/upload.rs` | Add `bearer_token: String` to `UploadRequest`; add `.bearer_auth()` to PUT, POST, PATCH calls |
| `crates/sync-engine/src/engine.rs` | Clone `bearer_token` into each `DownloadRequest` and `UploadRequest` |
| `crates/acceptance-test/src/fixture.rs` | Remove `.env("OCIS_BASIC_AUTH", "admin:admin")` from `spawn_daemon()` |

---

## Testing Strategy

- Unit tests for `propagate_download` and `propagate_upload` use `wiremock`; update mock expectations to assert the `Authorization: Bearer <token>` header is present on every request.
- The existing engine integration tests (`crates/sync-engine/tests/engine_tests.rs`) use `wiremock` — update them to assert bearer auth on all HTTP calls.
- Acceptance tests (`OCIS_ACCEPTANCE=1 cargo test`) verify end-to-end: the daemon acquires OIDC tokens, passes them to the sync engine, and downloads/uploads succeed against the real oCIS instance without any Basic Auth bypass.
- `cargo test --workspace` (without `OCIS_ACCEPTANCE`) verifies no regressions in unit/integration tests.
