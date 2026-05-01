use crate::view_model::ViewKind;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum Action {
    NavigateTo(ViewKind),
    ToggleWindow,
    AddAccountUrlChanged(String),
    AddAccountSubmit,
    PauseFolder(Uuid),
    ResumeFolder(Uuid),
    ForceSyncFolder(Uuid),
    RemoveAccount(Uuid),
    OpenFolder(String),
    Quit,
}

/// Commands the backend must handle that AppCore cannot do itself.
#[derive(Debug, Clone)]
pub enum BackendCommand {
    OpenFolder(String),
    Quit,
}
