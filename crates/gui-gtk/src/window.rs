use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};
use gui_core::{ViewKind, ViewModel};
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, HeaderBar};

pub fn build_window(app: &libadwaita::Application) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("ownCloud Sync")
        .default_width(480)
        .default_height(360)
        .build();

    let header = HeaderBar::new();
    let content = GtkBox::new(Orientation::Vertical, 0);
    content.append(&header);

    let body = Label::new(Some("Connecting to sync daemon\u{2026}"));
    body.set_vexpand(true);
    content.append(&body);

    window.set_content(Some(&content));
    window
}

pub fn render_view_model(window: &ApplicationWindow, vm: &ViewModel) {
    let summary = match &vm.active_view {
        ViewKind::SyncStatus if vm.accounts.is_empty() => "No accounts configured.".to_string(),
        ViewKind::SyncStatus => {
            format!("{} account(s) syncing", vm.accounts.len())
        }
        ViewKind::AddAccount { .. } => "Add Account".to_string(),
        ViewKind::AddAccountWaiting { .. } => "Waiting for browser sign-in\u{2026}".to_string(),
        ViewKind::AccountSettings(_) => "Account Settings".to_string(),
        ViewKind::GeneralSettings => "General Settings".to_string(),
    };

    if let Some(content) = window.content() {
        if let Some(gtk_box) = content.downcast_ref::<GtkBox>() {
            let mut child = gtk_box.first_child();
            let mut idx = 0;
            while let Some(c) = child {
                if idx == 1 {
                    gtk_box.remove(&c);
                    break;
                }
                child = c.next_sibling();
                idx += 1;
            }
            let label = Label::new(Some(&summary));
            label.set_vexpand(true);
            gtk_box.append(&label);
        }
    }
}
