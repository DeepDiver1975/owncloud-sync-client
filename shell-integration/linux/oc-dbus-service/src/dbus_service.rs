use zbus::interface;

use crate::emblem::status_to_emblem;
use crate::socket_client::SocketClient;

pub struct OwnCloudFileManager {
    pub socket_path: String,
}

#[interface(name = "org.owncloud.FileManager1")]
impl OwnCloudFileManager {
    async fn get_file_status(&self, path: String) -> zbus::fdo::Result<(String, String)> {
        let mut client = match SocketClient::connect_path(&self.socket_path).await {
            Ok(c) => c,
            Err(_) => return Ok(("NONE".to_string(), "".to_string())),
        };
        match client.get_file_status(&path).await {
            Ok(tag) => {
                let emblem = status_to_emblem(&tag).to_string();
                Ok((tag, emblem))
            }
            Err(_) => Ok(("NONE".to_string(), "".to_string())),
        }
    }

    async fn get_menu_items(&self, path: String) -> zbus::fdo::Result<Vec<(String, String, bool)>> {
        let mut client = match SocketClient::connect_path(&self.socket_path).await {
            Ok(c) => c,
            Err(_) => return Ok(vec![]),
        };
        match client.get_menu_items(&path).await {
            Ok(items) => Ok(items),
            Err(_) => Ok(vec![]),
        }
    }

    async fn execute_command(&self, command: String, paths: Vec<String>) -> zbus::fdo::Result<()> {
        let mut client = match SocketClient::connect_path(&self.socket_path).await {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let _ = client.execute_command(&command, &paths).await;
        Ok(())
    }

    #[zbus(signal)]
    pub async fn status_changed(
        ctx: &zbus::object_server::SignalEmitter<'_>,
        path: String,
        status: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn path_registered(
        ctx: &zbus::object_server::SignalEmitter<'_>,
        path: String,
    ) -> zbus::Result<()>;
}
