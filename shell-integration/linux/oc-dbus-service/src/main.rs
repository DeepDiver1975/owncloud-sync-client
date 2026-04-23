use oc_dbus_service::dbus_service::OwnCloudFileManager;
use oc_dbus_service::socket_client::{Broadcast, SocketClient};

fn socket_path() -> String {
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    format!("{xdg_runtime}/owncloud/socket")
}

fn parse_cli_subcommand() -> Option<(&'static str, Vec<String>)> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "execute-command" {
        let rest = args[2..].to_vec();
        Some(("execute-command", rest))
    } else {
        None
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Some(("execute-command", args)) = parse_cli_subcommand() {
        if args.is_empty() {
            eprintln!("Usage: oc-dbus-service execute-command COMMAND [PATH...]");
            std::process::exit(1);
        }
        let command = &args[0];
        let paths = args[1..].to_vec();
        let path_str = socket_path();
        match SocketClient::connect_path(&path_str).await {
            Ok(mut client) => {
                let _ = client.execute_command(command, &paths).await;
            }
            Err(e) => {
                eprintln!("failed to connect to daemon: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let socket = socket_path();

    let conn = zbus::connection::Builder::session()?
        .name("org.owncloud.FileManager1")?
        .serve_at(
            "/org/owncloud/FileManager1",
            OwnCloudFileManager {
                socket_path: socket.clone(),
            },
        )?
        .build()
        .await?;

    let conn_clone = conn.clone();
    let socket_clone = socket.clone();
    tokio::spawn(async move {
        loop {
            match SocketClient::connect_path(&socket_clone).await {
                Ok(mut client) => loop {
                    match client.read_broadcast().await {
                        Ok(Broadcast::Status { tag, path }) => {
                            let iface_ref = conn_clone
                                .object_server()
                                .interface::<_, OwnCloudFileManager>("/org/owncloud/FileManager1")
                                .await;
                            if let Ok(iface) = iface_ref {
                                let _ = OwnCloudFileManager::status_changed(
                                    iface.signal_context(),
                                    path,
                                    tag,
                                )
                                .await;
                            }
                        }
                        Ok(Broadcast::RegisterPath(path)) => {
                            let iface_ref = conn_clone
                                .object_server()
                                .interface::<_, OwnCloudFileManager>("/org/owncloud/FileManager1")
                                .await;
                            if let Ok(iface) = iface_ref {
                                let _ = OwnCloudFileManager::path_registered(
                                    iface.signal_context(),
                                    path,
                                )
                                .await;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("broadcast read error: {e}, reconnecting in 5s");
                            break;
                        }
                    }
                },
                Err(e) => {
                    tracing::debug!("cannot connect to daemon socket: {e}, retrying in 5s");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    std::future::pending::<()>().await;
    Ok(())
}
