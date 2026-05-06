#![allow(dead_code)]

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{error, info};

mod config;
mod folder_manager;
mod gui_ipc;
mod lock;
mod oidc_callback;
mod paths;
mod scheduler;
mod vfs_factory;
mod watcher;

use config::AppConfig;
use folder_manager::FolderManager;
use gui_ipc::handler::{handle_command, ShouldQuit};
use gui_ipc::{protocol::DaemonCommand, protocol::DaemonEvent, GuiIpcServer};
use lock::{LockError, LockFile};
use scheduler::SyncScheduler;
use socket_api::server::SocketApiServer;
use socket_api::transport::unix::UnixTransport;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Acquire exclusive lock to prevent multiple daemon instances.
    let lock_path = paths::platform_lock_path();
    let _lock = match LockFile::acquire(&lock_path) {
        Ok(l) => l,
        Err(LockError::AlreadyRunning) => {
            eprintln!("ocsyncd is already running (lock: {})", lock_path.display());
            std::process::exit(1);
        }
        Err(LockError::Io(e)) => {
            eprintln!("Failed to acquire lock at {}: {e}", lock_path.display());
            std::process::exit(1);
        }
    };
    info!("Lock acquired: {}", lock_path.display());

    let config_path = paths::platform_config_dir().join("owncloud.toml");
    let initial_config = AppConfig::load_or_default(&config_path)?;
    info!("Config loaded from {}", config_path.display());

    let poll_secs = initial_config.general.poll_interval_secs;

    let all_folders: Vec<_> = initial_config
        .account
        .iter()
        .flat_map(|a| a.folder.clone())
        .collect();
    let mut folder_manager =
        FolderManager::init_sync(&all_folders, &initial_config.account).await?;
    let config = Arc::new(tokio::sync::Mutex::new(initial_config));
    info!("FolderManager: {} folders", folder_manager.folders.len());

    let sync_states = folder_manager.sync_states();
    let folder_roots = folder_manager.folder_roots();
    let shared_vfs = folder_manager.shared_vfs();
    let socket_api = Arc::new(SocketApiServer::new(sync_states, folder_roots, shared_vfs));

    let (gui_ipc, _initial_rx) = GuiIpcServer::new();

    let folder_ids: Vec<_> = folder_manager.folders.keys().cloned().collect();
    let mut scheduler = SyncScheduler::new(folder_ids.clone());

    // Spawn SocketApiServer.
    let socket_api_clone = Arc::clone(&socket_api);
    tokio::spawn(async move {
        let transport = match UnixTransport::bind(&UnixTransport::default_path()).await {
            Ok(t) => t,
            Err(e) => {
                error!("socket-api bind error: {e}");
                return;
            }
        };
        if let Err(e) = socket_api_clone.run(Box::new(transport)).await {
            error!("SocketApiServer error: {e}");
        }
    });

    // Spawn GuiIpcServer.
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(64);
    let gui_ipc_clone = Arc::clone(&gui_ipc);
    let gui_socket_path = paths::platform_gui_socket_path();
    tokio::spawn(async move {
        if let Err(e) = gui_ipc_clone.run(&gui_socket_path, cmd_tx).await {
            error!("GuiIpcServer error: {e}");
        }
    });

    gui_ipc.broadcast(DaemonEvent::Ready);

    // Spawn remote poll loop — sends TriggerSync periodically.
    let folder_ids_poll = folder_ids.clone();
    let (poll_tx, mut poll_rx) = mpsc::channel::<DaemonCommand>(64);
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(poll_secs));
        ticker.tick().await; // skip first immediate tick
        loop {
            ticker.tick().await;
            info!("poll tick: {} folder(s)", folder_ids_poll.len());
            for id in &folder_ids_poll {
                let _ = poll_tx
                    .send(DaemonCommand::TriggerSync { folder_id: *id })
                    .await;
            }
        }
    });

    // Main loop.
    let mut scheduler_tick = interval(Duration::from_millis(100));
    info!("ocsyncd ready");

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match handle_command(
                    cmd,
                    &mut scheduler,
                    &mut folder_manager,
                    &gui_ipc,
                    Arc::clone(&config),
                    config_path.clone(),
                ).await {
                    Ok(ShouldQuit::Yes) => {
                        info!("Quit command received; shutting down");
                        break;
                    }
                    Ok(ShouldQuit::No) => {}
                    Err(e) => error!("handle_command error: {e}"),
                }
            }

            Some(cmd) = poll_rx.recv() => {
                if let DaemonCommand::TriggerSync { folder_id } = cmd {
                    scheduler.request_sync(folder_id);
                }
            }

            _ = scheduler_tick.tick() => {
                let ready = scheduler.ready_to_run();
                if !ready.is_empty() {
                    info!("scheduler: {} folder(s) ready to sync", ready.len());
                }
                for folder_id in ready {
                    scheduler.start_sync(folder_id);
                    gui_ipc.broadcast(DaemonEvent::SyncStarted { folder_id });

                    let engine = folder_manager.get_engine(folder_id).cloned();
                    let ipc = Arc::clone(&gui_ipc);
                    tokio::spawn(async move {
                        if let Some(engine) = engine {
                            info!("run_sync starting for folder {folder_id}");
                            let errors = match engine.run_sync().await {
                                Ok(_) => vec![],
                                Err(e) => {
                                    info!("run_sync error for folder {folder_id}: {e}");
                                    vec![e.to_string()]
                                }
                            };
                            info!("run_sync done for folder {folder_id}: {} error(s)", errors.len());
                            ipc.broadcast(DaemonEvent::SyncFinished { folder_id, errors });
                        } else {
                            info!("run_sync: no engine for folder {folder_id}");
                        }
                    });
                    scheduler.finish_sync(folder_id);
                }
            }
        }
    }

    info!("ocsyncd exiting");
    Ok(())
}
