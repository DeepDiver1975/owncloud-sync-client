# Path Encoding Fix Design

**Date:** 2026-05-12  
**Branch:** fix/path-encoding  
**Status:** Approved

## Problem

Files and folders on oCIS whose names contain spaces or other characters that require HTTP percent-encoding (e.g. `hello world.txt`) appear on the local desktop with the raw encoded form (`hello%20world.txt`). This applies to any character that a WebDAV server percent-encodes in `<D:href>` values — spaces (`%20`), `#` (`%23`), `<`/`>`, non-ASCII UTF-8 sequences, emoji, etc.

The bug exists in both sync directions:

- **Down-sync (remote → local):** `parse_propfind` in `sync-engine/src/discovery/remote.rs` reads `<D:href>` values and strips the space-root prefix to get a relative path, but never percent-decodes the result. `RemoteEntry.path` ends up as `hello%20world.txt` instead of `hello world.txt`.
- **Up-sync (local → remote):** `engine.rs` constructs `remote_url` by calling `space_root.join(rel_path.as_str())`. The `url` crate's `join` does not percent-encode raw spaces in the input string, producing a malformed or rejected URL when `rel_path` contains characters outside the URL safe set.

## Invariant

`RemoteEntry.path` is always a **decoded filesystem path** — no percent-encoding. URLs sent to the server always have paths **properly percent-encoded**. This is enforced at two explicit boundaries.

## Fix

### Dependency

Add `percent-encoding = "2"` explicitly to `crates/sync-engine/Cargo.toml`. The crate is already in the lockfile as a transitive dependency of `url`; making it explicit avoids relying on that implicitly.

### Decode boundary — `parse_propfind` in `discovery/remote.rs`

Add a helper function:

```rust
fn decode_href_path(s: &str) -> String {
    // TODO: add NFC/NFD Unicode normalization here per platform (macOS vs Windows/Linux)
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}
```

Apply it to the `rel` variable before building `Utf8PathBuf`, covering both file and directory branches:

```rust
let rel = decode_href_path(
    href.strip_prefix(root_path)
        .unwrap_or(&href)
        .trim_start_matches('/'),
);
```

The `// TODO` comment marks the designated place for future NFC/NFD normalization (relevant for macOS NFD vs Windows/Linux NFC path encoding).

### Encode boundary — URL construction in `engine.rs`

Add a helper function (in `engine.rs` or a shared `util` module):

```rust
fn path_to_url(base: &Url, rel: &str) -> Result<Url, SyncError> {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    // Encode each path segment individually, preserving '/' as separator.
    let encoded: String = rel
        .split('/')
        .map(|seg| utf8_percent_encode(seg, percent_encoding::PATH_SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/");
    base.join(&encoded).map_err(|e| SyncError::Parse(e.to_string()))
}
```

Replace both calls to `space_root.join(rel_path.as_str())` in `engine.rs` (dir loop and file loop) with `path_to_url(&self.cfg.space_root, rel_path.as_str())`.

`ensure_parent_collections` in `upload.rs` operates on the already-encoded `remote_url.path()` and continues to work unchanged.

## Tests

### Unit tests — decode (`crates/sync-engine/tests/remote_discovery.rs`)

**`discovers_file_with_encoded_name`**  
Mock PROPFIND response with href `/dav/spaces/space1/caf%C3%A9%20%E6%96%87%E4%BB%B6%20%3C1%3E.txt` (URL-encoding of `café 文件 <1>.txt`).  
Assert `entries[0].path == "café 文件 <1>.txt"`.

**`discovers_dir_with_encoded_name`**  
Mock PROPFIND response with collection href `/dav/spaces/space1/my%20folder%20%F0%9F%93%81/` (URL-encoding of `my folder 📁/`).  
Assert entry has `path == "my folder 📁"` and `is_dir == true`.

### Unit tests — encode (new `crates/sync-engine/tests/path_encoding_tests.rs` or inline)

**`path_to_url_encodes_special_chars`**  
Input: `"café 文件 <1>.txt"`.  
Assert resulting URL path ends with `caf%C3%A9%20%E6%96%87%E4%BB%B6%20%3C1%3E.txt`.

**`path_to_url_round_trips`**  
For several inputs (space, `#`, `?`, `[`, emoji, Chinese), verify that `percent_decode(path_to_url(base, input).path().strip_prefix(base.path())) == input`.

### Acceptance test — `test_sync_path_encoding` (`crates/acceptance-test/tests/sync.rs`)

Single bidirectional test covering both directions:

1. Pre-seed `"héllo wörld 文件 #1.txt"` on oCIS via `ocis_client.put("héllo wörld 文件 #1.txt", b"remote content")`.
2. Add account; wait for `SyncFinished` with no errors.
3. Assert `sync_dir/héllo wörld 文件 #1.txt` exists with content `b"remote content"`.
4. Assert no file whose name contains `%` exists in `sync_dir` (guard against un-decoded names).
5. Write `sync_dir/upload 📄 <test>.txt` with content `b"local content"`.
6. Poll until `ocis_client.exists("upload 📄 <test>.txt")` returns true.
7. Assert `ocis_client.get("upload 📄 <test>.txt")` returns `b"local content"`.

## Out of scope

- NFC/NFD Unicode normalization (macOS NFD vs Windows/Linux NFC) — the `decode_href_path` helper is the designated future insertion point.
- Percent-encoding in `OcisClient::webdav_url` in the acceptance-test helper — `Url::join` handles this correctly when the path is built via `format!("/dav/spaces/{}/...", ...)` because the `url` crate parses and re-serializes the path. Verify during implementation.
