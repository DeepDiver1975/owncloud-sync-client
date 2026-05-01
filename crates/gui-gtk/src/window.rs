use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, ProgressBar, Separator};
use gui_core::{AccountView, FolderStatus, ViewKind, ViewModel};
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

    let body = Label::new(Some("Connecting to sync daemon…"));
    body.set_vexpand(true);
    content.append(&body);

    window.set_content(Some(&content));
    window
}

pub fn render_view_model(window: &ApplicationWindow, vm: &ViewModel) {
    let body: gtk4::Widget = match &vm.active_view {
        ViewKind::SyncStatus => make_sync_status(&vm.accounts).into(),
        ViewKind::AddAccount { url_input, error } => {
            make_add_account(url_input, error.as_deref()).into()
        }
        ViewKind::AddAccountWaiting { .. } => {
            Label::new(Some("Waiting for browser sign-in…")).into()
        }
        ViewKind::AccountSettings(id) => {
            if let Some(acc) = vm.accounts.iter().find(|a| &a.id == id) {
                make_account_settings(acc).into()
            } else {
                Label::new(Some("Account not found")).into()
            }
        }
        ViewKind::GeneralSettings => Label::new(Some("General Settings")).into(),
    };

    if let Some(content) = window.content() {
        if let Some(gtk_box) = content.downcast_ref::<GtkBox>() {
            // Remove all children after the HeaderBar (index 0)
            let mut children_to_remove = Vec::new();
            let mut child = gtk_box.first_child();
            let mut idx = 0;
            while let Some(c) = child {
                if idx > 0 {
                    children_to_remove.push(c.clone());
                }
                child = c.next_sibling();
                idx += 1;
            }
            for c in children_to_remove {
                gtk_box.remove(&c);
            }
            body.set_vexpand(true);
            gtk_box.append(&body);
        }
    }
}

fn make_sync_status(accounts: &[AccountView]) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    if accounts.is_empty() {
        let label = Label::new(Some("No accounts configured."));
        let hint = Label::new(Some("Add your first ownCloud account to start syncing."));
        vbox.append(&label);
        vbox.append(&hint);
        return vbox;
    }

    for account in accounts {
        let acc_label = Label::new(Some(&account.url));
        acc_label.set_xalign(0.0);
        acc_label.add_css_class("title-4");
        vbox.append(&acc_label);

        for folder in &account.folders {
            let row = GtkBox::new(Orientation::Horizontal, 8);

            let status_icon = Label::new(Some(match folder.status {
                FolderStatus::Idle => "●",
                FolderStatus::Syncing => "↻",
                FolderStatus::Error => "✕",
                FolderStatus::Paused => "⏸",
            }));
            row.append(&status_icon);

            let name_col = GtkBox::new(Orientation::Vertical, 2);
            let name_label = Label::new(Some(&folder.display_name));
            name_label.set_xalign(0.0);
            name_col.append(&name_label);
            let path_label = Label::new(Some(&folder.local_path));
            path_label.set_xalign(0.0);
            path_label.add_css_class("caption");
            name_col.append(&path_label);
            name_col.set_hexpand(true);
            row.append(&name_col);

            if let Some((done, total)) = folder.progress {
                let pct = done
                    .checked_mul(100)
                    .and_then(|n| n.checked_div(total))
                    .unwrap_or(0);
                let pb = ProgressBar::new();
                pb.set_fraction(pct as f64 / 100.0);
                row.append(&pb);
            }

            vbox.append(&row);
        }

        vbox.append(&Separator::new(Orientation::Horizontal));
    }
    vbox
}

fn make_add_account(url_input: &str, error: Option<&str>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(24);
    vbox.set_margin_start(24);
    vbox.set_margin_end(24);
    vbox.set_halign(gtk4::Align::Center);
    vbox.set_valign(gtk4::Align::Center);

    let title = Label::new(Some("Add ownCloud account"));
    title.add_css_class("title-2");
    vbox.append(&title);

    let subtitle = Label::new(Some(
        "Enter your server address. You will be redirected to the browser to sign in.",
    ));
    subtitle.set_wrap(true);
    vbox.append(&subtitle);

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("https://your.server.com"));
    entry.set_text(url_input);
    vbox.append(&entry);

    let connect_btn = Button::with_label("Connect");
    vbox.append(&connect_btn);

    if let Some(err) = error {
        let err_label = Label::new(Some(err));
        err_label.add_css_class("error");
        vbox.append(&err_label);
    }

    vbox
}

fn make_account_settings(account: &AccountView) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(24);
    vbox.set_margin_start(24);
    vbox.set_margin_end(24);

    let title = Label::new(Some("Account Settings"));
    title.add_css_class("title-2");
    vbox.append(&title);

    let url_label = Label::new(Some(&format!("Server: {}", account.url)));
    url_label.set_xalign(0.0);
    vbox.append(&url_label);

    for folder in &account.folders {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let name = Label::new(Some(&folder.display_name));
        let arrow = Label::new(Some("→"));
        let path = Label::new(Some(&folder.local_path));
        row.append(&name);
        row.append(&arrow);
        row.append(&path);
        vbox.append(&row);
    }

    let remove_btn = Button::with_label("Remove Account");
    vbox.append(&remove_btn);

    vbox
}
