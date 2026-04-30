use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;

pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<Event>>,
}

impl FolderWatcher {
    pub fn watch(path: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::channel(64);

        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.blocking_send(event);
        })?;

        watcher.watch(path, RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    pub async fn next_event(&mut self) -> Option<Event> {
        loop {
            match self.rx.recv().await? {
                Ok(event) => return Some(event),
                Err(e) => {
                    tracing::warn!("watcher error: {e}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn detects_file_create() {
        let dir = tempdir().unwrap();
        let mut watcher = FolderWatcher::watch(dir.path()).unwrap();

        let path = dir.path().join("hello.txt");
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(&path, b"hello").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event())
            .await
            .expect("timeout waiting for create event")
            .expect("channel closed");

        let is_create = matches!(event.kind, EventKind::Create(_));
        assert!(is_create, "expected Create event, got {:?}", event.kind);
    }

    #[tokio::test]
    async fn detects_file_modify() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"initial").unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut watcher = FolderWatcher::watch(dir.path()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        std::fs::write(&path, b"modified").unwrap();

        let found = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let event = watcher.next_event().await?;
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    return Some(event);
                }
            }
        })
        .await
        .expect("timeout waiting for modify event");
        assert!(
            found.is_some(),
            "channel closed before seeing Create/Modify event"
        );
    }
}
