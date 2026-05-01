pub mod action;
pub mod core;
pub(crate) mod daemon_conn;
pub mod model;
pub(crate) mod spawn;
pub mod view_model;

pub use action::{Action, BackendCommand};
pub use model::{AccountView, FolderStatus, FolderView};
pub use view_model::{ViewKind, ViewModel};
