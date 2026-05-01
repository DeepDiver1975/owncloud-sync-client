use ksni::Tray;

pub struct OwncloudTray {
    pub on_open: Box<dyn Fn() + Send + Sync>,
    pub on_quit: Box<dyn Fn() + Send + Sync>,
}

impl Tray for OwncloudTray {
    fn id(&self) -> String {
        "owncloud-sync".to_string()
    }

    fn title(&self) -> String {
        "ownCloud Sync".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::MenuItem::Standard(ksni::menu::StandardItem {
                label: "Open".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_open)()),
                ..Default::default()
            }),
            ksni::MenuItem::Separator,
            ksni::MenuItem::Standard(ksni::menu::StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_quit)()),
                ..Default::default()
            }),
        ]
    }
}
