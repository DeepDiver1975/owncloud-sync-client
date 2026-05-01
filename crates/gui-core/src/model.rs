use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderStatus {
    Idle,
    Syncing,
    Error,
    Paused,
}

impl fmt::Display for FolderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FolderStatus::Idle => write!(f, "Idle"),
            FolderStatus::Syncing => write!(f, "Syncing"),
            FolderStatus::Error => write!(f, "Error"),
            FolderStatus::Paused => write!(f, "Paused"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FolderView {
    pub id: Uuid,
    pub display_name: String,
    pub local_path: String,
    pub status: FolderStatus,
    pub progress: Option<(u64, u64)>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AccountView {
    pub id: Uuid,
    pub url: String,
    pub display_name: String,
    pub folders: Vec<FolderView>,
}
