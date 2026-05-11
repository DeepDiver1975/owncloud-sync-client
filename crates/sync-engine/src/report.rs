use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpEvent {
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration_ms: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncReport {
    pub folder_id: Uuid,
    pub remote_entries: usize,
    pub local_entries: usize,
    pub downloads: usize,
    pub uploads: usize,
    pub conflicts: usize,
    pub deletes_local: usize,
    pub deletes_remote: usize,
    pub ignored: usize,
    pub errors: Vec<String>,
    pub http_events: Vec<HttpEvent>,
    pub duration_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_report_serde_roundtrip() {
        let report = SyncReport {
            folder_id: Uuid::nil(),
            remote_entries: 3,
            local_entries: 1,
            downloads: 3,
            uploads: 1,
            conflicts: 0,
            deletes_local: 0,
            deletes_remote: 0,
            ignored: 0,
            errors: vec!["oops".to_string()],
            http_events: vec![HttpEvent {
                method: "GET".to_string(),
                url: "/dav/spaces/s1/hello.txt".to_string(),
                status: 200,
                duration_ms: 42,
                bytes: 5,
            }],
            duration_ms: 100,
        };

        let json = serde_json::to_string(&report).unwrap();
        let back: SyncReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.folder_id, Uuid::nil());
        assert_eq!(back.remote_entries, 3);
        assert_eq!(back.http_events.len(), 1);
        assert_eq!(back.http_events[0].method, "GET");
        assert_eq!(back.errors, vec!["oops".to_string()]);
    }
}
