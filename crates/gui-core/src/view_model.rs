use crate::model::AccountView;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ViewModel {
    pub accounts: Vec<AccountView>,
    pub active_view: ViewKind,
    pub window_visible: bool,
    pub daemon_connected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewKind {
    SyncStatus,
    AccountSettings(Uuid),
    AddAccount {
        url_input: String,
        error: Option<String>,
    },
    AddAccountWaiting {
        account_id: Uuid,
        url_input: String,
    },
    GeneralSettings,
}
