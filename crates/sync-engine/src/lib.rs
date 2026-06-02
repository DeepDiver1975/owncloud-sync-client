pub mod discovery;
pub mod engine;
pub mod error;
pub mod propagate;
pub mod reconcile;
pub mod report;
pub mod state;
pub mod types;

pub use report::{HttpEvent, SyncReport};

use url::Url;

/// Build a remote WebDAV URL from `space_root` and a **decoded** relative path.
///
/// Each path component is appended as a URL path segment so that characters
/// such as `?`, `#`, spaces and non-ASCII are percent-encoded. Using
/// `Url::join` here would be wrong: it parses a `?` in a decoded file name
/// (e.g. `测试 file?name.txt`) as the start of a query string, producing a URL
/// that points at the wrong path and yields a 404 on GET/PUT.
///
/// `rel` uses `/` as its component separator (matching WebDAV hrefs). A
/// trailing slash is preserved so callers can build collection URLs.
pub(crate) fn join_remote_path(space_root: &Url, rel: &str) -> Result<Url, url::ParseError> {
    let mut url = space_root.clone();
    let trailing_slash = rel.ends_with('/');
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        // space_root's path always ends in `/`, leaving an empty trailing
        // segment; drop it before appending so we don't get a double slash.
        segments.pop_if_empty();
        for component in rel.trim_matches('/').split('/') {
            if !component.is_empty() {
                segments.push(component);
            }
        }
        if trailing_slash {
            segments.push("");
        }
    }
    Ok(url)
}

#[cfg(test)]
mod join_remote_path_tests {
    use super::*;

    fn space() -> Url {
        Url::parse("https://ocis.example.com/dav/spaces/abc$def/").unwrap()
    }

    #[test]
    fn encodes_question_mark_and_space() {
        // Regression: `?` must be percent-encoded, not parsed as a query.
        let url = join_remote_path(&space(), "测试 file?name.txt").unwrap();
        assert_eq!(url.query(), None, "filename must not become a query string");
        assert!(
            url.path()
                .ends_with("/%E6%B5%8B%E8%AF%95%20file%3Fname.txt"),
            "unexpected path: {}",
            url.path()
        );
    }

    #[test]
    fn encodes_angle_brackets() {
        let url = join_remote_path(&space(), "上传 file<test>.txt").unwrap();
        assert_eq!(url.query(), None);
        assert!(url
            .path()
            .ends_with("/%E4%B8%8A%E4%BC%A0%20file%3Ctest%3E.txt"));
    }

    #[test]
    fn nested_paths_keep_separators() {
        let url = join_remote_path(&space(), "a/b c?x.txt").unwrap();
        assert_eq!(url.query(), None);
        assert!(
            url.path().ends_with("/a/b%20c%3Fx.txt"),
            "got {}",
            url.path()
        );
    }

    #[test]
    fn trailing_slash_preserved_for_collections() {
        let url = join_remote_path(&space(), "sub dir/").unwrap();
        assert!(url.path().ends_with("/sub%20dir/"), "got {}", url.path());
    }

    #[test]
    fn no_double_slash_after_space_root() {
        let url = join_remote_path(&space(), "file.txt").unwrap();
        assert!(
            url.path().ends_with("/spaces/abc$def/file.txt"),
            "got {}",
            url.path()
        );
    }
}
