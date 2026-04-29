use std::sync::Arc;

use camino::Utf8Path;

use crate::broadcast::BroadcastSender;
use vfs_core::Vfs;

pub async fn handle_make_available_locally(
    paths: Vec<String>,
    vfs: Arc<dyn Vfs>,
    broadcast: &BroadcastSender,
) -> String {
    let mut had_error = false;

    for path in &paths {
        let utf8 = Utf8Path::new(path.as_str());
        match vfs.hydrate(utf8).await {
            Ok(()) => broadcast.status_changed("OK", path).await,
            Err(e) => {
                tracing::warn!("hydrate failed for {path}: {e}");
                broadcast.status_changed("ERROR", path).await;
                had_error = true;
            }
        }
    }

    if had_error {
        "MAKE_AVAILABLE_LOCALLY:ERROR\n".to_string()
    } else {
        "MAKE_AVAILABLE_LOCALLY:OK\n".to_string()
    }
}

pub async fn handle_make_online_only(
    paths: Vec<String>,
    vfs: Arc<dyn Vfs>,
    broadcast: &BroadcastSender,
) -> String {
    let mut had_error = false;

    for path in &paths {
        let utf8 = Utf8Path::new(path.as_str());
        match vfs.dehydrate(utf8).await {
            Ok(()) => broadcast.status_changed("OK", path).await,
            Err(e) => {
                tracing::warn!("dehydrate failed for {path}: {e}");
                broadcast.status_changed("ERROR", path).await;
                had_error = true;
            }
        }
    }

    if had_error {
        "MAKE_ONLINE_ONLY:ERROR\n".to_string()
    } else {
        "MAKE_ONLINE_ONLY:OK\n".to_string()
    }
}
