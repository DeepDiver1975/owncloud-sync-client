use serde::{Deserialize, Serialize};

/// Commands sent from Rust to the Swift FileProvider extension over XPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum XpcCommand {
    CreatePlaceholder {
        path: String,
        etag: String,
        size: u64,
        mtime: i64,
    },
    UpdatePlaceholder {
        path: String,
        etag: String,
        size: u64,
        mtime: i64,
    },
    Hydrate {
        path: String,
    },
    Dehydrate {
        path: String,
    },
    IsVirtual {
        path: String,
    },
    Status {
        path: String,
    },
    SetPinned {
        path: String,
        pinned: bool,
    },
}

/// Reply from Swift FileProvider extension back to Rust over XPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XpcReply {
    pub ok: bool,
    pub error: Option<String>,
    #[serde(rename = "bool")]
    pub bool_value: Option<bool>,
    pub status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug>(value: &T) {
        let json = serde_json::to_string(value).expect("serialize");
        let decoded: T = serde_json::from_str(&json).expect("deserialize");
        // Verify by re-serializing decoded — both must produce the same JSON.
        let json2 = serde_json::to_string(&decoded).expect("re-serialize");
        assert_eq!(json, json2, "roundtrip mismatch");
    }

    #[test]
    fn test_create_placeholder_roundtrip() {
        roundtrip(&XpcCommand::CreatePlaceholder {
            path: "docs/readme.md".to_string(),
            etag: "abc123".to_string(),
            size: 1024,
            mtime: 1700000000,
        });
    }

    #[test]
    fn test_update_placeholder_roundtrip() {
        roundtrip(&XpcCommand::UpdatePlaceholder {
            path: "docs/readme.md".to_string(),
            etag: "def456".to_string(),
            size: 2048,
            mtime: 1700001000,
        });
    }

    #[test]
    fn test_hydrate_roundtrip() {
        roundtrip(&XpcCommand::Hydrate {
            path: "photos/img.png".to_string(),
        });
    }

    #[test]
    fn test_dehydrate_roundtrip() {
        roundtrip(&XpcCommand::Dehydrate {
            path: "photos/img.png".to_string(),
        });
    }

    #[test]
    fn test_is_virtual_roundtrip() {
        roundtrip(&XpcCommand::IsVirtual {
            path: "photos/img.png".to_string(),
        });
    }

    #[test]
    fn test_status_roundtrip() {
        roundtrip(&XpcCommand::Status {
            path: "docs/readme.md".to_string(),
        });
    }

    #[test]
    fn test_set_pinned_roundtrip() {
        roundtrip(&XpcCommand::SetPinned {
            path: "docs/readme.md".to_string(),
            pinned: true,
        });
        roundtrip(&XpcCommand::SetPinned {
            path: "docs/readme.md".to_string(),
            pinned: false,
        });
    }

    #[test]
    fn test_reply_ok_roundtrip() {
        roundtrip(&XpcReply {
            ok: true,
            error: None,
            bool_value: None,
            status: None,
        });
    }

    #[test]
    fn test_reply_error_roundtrip() {
        roundtrip(&XpcReply {
            ok: false,
            error: Some("file not found".to_string()),
            bool_value: None,
            status: None,
        });
    }

    #[test]
    fn test_reply_bool_roundtrip() {
        roundtrip(&XpcReply {
            ok: true,
            error: None,
            bool_value: Some(true),
            status: None,
        });
    }

    #[test]
    fn test_reply_status_roundtrip() {
        roundtrip(&XpcReply {
            ok: true,
            error: None,
            bool_value: None,
            status: Some("Hydrated".to_string()),
        });
    }

    #[test]
    fn test_create_placeholder_json_tag() {
        let cmd = XpcCommand::CreatePlaceholder {
            path: "a/b".to_string(),
            etag: "e".to_string(),
            size: 0,
            mtime: 0,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(
            json.contains(r#""cmd":"create_placeholder""#),
            "unexpected JSON: {json}"
        );
    }

    #[test]
    fn test_is_virtual_json_tag() {
        let cmd = XpcCommand::IsVirtual {
            path: "x".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(
            json.contains(r#""cmd":"is_virtual""#),
            "unexpected JSON: {json}"
        );
    }

    #[test]
    fn test_set_pinned_json_tag() {
        let cmd = XpcCommand::SetPinned {
            path: "x".to_string(),
            pinned: true,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(
            json.contains(r#""cmd":"set_pinned""#),
            "unexpected JSON: {json}"
        );
    }
}
