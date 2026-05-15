#![allow(dead_code)]

use anyhow::Result;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

mod config;
mod folder_manager;
mod gui_ipc;
mod lock;
mod oidc_callback;
mod paths;
mod scheduler;
mod space_poller;
mod vfs_factory;
mod watcher;

use config::AppConfig;
use folder_manager::FolderManager;
use gui_ipc::handler::{handle_command, HandleContext, ShouldQuit};
use gui_ipc::protocol::{AccountSnapshot, DaemonCommand, DaemonEvent, FolderSnapshot};
use gui_ipc::{GuiIpcServer, SnapshotProvider};
use lock::{LockError, LockFile};
use scheduler::SyncScheduler;
use socket_api::server::SocketApiServer;
use socket_api::transport::unix::UnixTransport;
use space_poller::SpacePoller;
use sync_engine::SyncReport;

async fn build_token_managers(
    config: &AppConfig,
    insecure: bool,
) -> std::collections::HashMap<uuid::Uuid, Arc<ocis_client::auth::TokenManager>> {
    use ocis_client::auth::{KeychainStore, OidcAuth, TokenManager};

    let mut map = std::collections::HashMap::new();
    for account in &config.account {
        let account_id_str = account.id.to_string();
        let token_set = match tokio::task::spawn_blocking({
            let id = account_id_str.clone();
            move || KeychainStore::load(&id)
        })
        .await
        {
            Ok(Ok(Some(t))) => t,
            Ok(Ok(None)) => {
                tracing::warn!("no keychain entry for account {}; skipping", account.id);
                continue;
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    "keychain load error for account {}: {e}; skipping",
                    account.id
                );
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    "keychain task panicked for account {}: {e}; skipping",
                    account.id
                );
                continue;
            }
        };

        let oidc = match OidcAuth::discover(
            &account.url,
            gui_ipc::handler::OCIS_CLIENT_ID,
            Some(gui_ipc::handler::OCIS_CLIENT_SECRET.to_string()),
            "http://localhost:9999/callback",
            insecure,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(
                    "OIDC re-discovery failed for account {}: {e}; skipping",
                    account.id
                );
                continue;
            }
        };

        let tm = Arc::new(TokenManager::new(oidc, token_set, account_id_str));
        map.insert(account.id, tm);
    }
    map
}

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

    let insecure = initial_config.general.insecure;
    let token_managers_map = build_token_managers(&initial_config, insecure).await;
    info!(
        "TokenManagers: {} account(s) have credentials",
        token_managers_map.len()
    );
    let token_managers: Arc<
        std::sync::RwLock<
            std::collections::HashMap<uuid::Uuid, Arc<ocis_client::auth::TokenManager>>,
        >,
    > = Arc::new(std::sync::RwLock::new(token_managers_map));

    let all_folders: Vec<_> = initial_config
        .account
        .iter()
        .flat_map(|a| a.folder.clone())
        .collect();
    let init_managers: std::collections::HashMap<_, _> = {
        let guard = token_managers.read().unwrap();
        guard.iter().map(|(k, v)| (*k, Arc::clone(v))).collect()
    };
    let mut folder_manager =
        FolderManager::init_sync(&all_folders, &initial_config.account, &init_managers).await?;
    let config = Arc::new(Mutex::new(initial_config));
    info!("FolderManager: {} folders", folder_manager.folders.len());

    // Watcher channel: all FolderWatcher events are forwarded here.
    let (watcher_tx, mut watcher_rx) =
        tokio::sync::mpsc::channel::<(uuid::Uuid, notify::Event)>(256);

    // Spawn per-folder watcher forwarding tasks for all initial folders.
    for folder_id in folder_manager.folders.keys().cloned().collect::<Vec<_>>() {
        if let Some(mut watcher) = folder_manager.take_watcher(folder_id) {
            let tx = watcher_tx.clone();
            tokio::spawn(async move {
                while let Some(event) = watcher.next_event().await {
                    let _ = tx.send((folder_id, event)).await;
                }
            });
        }
    }

    let sync_states = folder_manager.sync_states();
    let folder_roots = folder_manager.folder_roots();
    let shared_vfs = folder_manager.shared_vfs();
    let socket_api = Arc::new(SocketApiServer::new(sync_states, folder_roots, shared_vfs));

    let (gui_ipc, _initial_rx) = GuiIpcServer::new();

    let folder_ids: Vec<_> = folder_manager.folders.keys().cloned().collect();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(folder_ids.clone())));

    // Shared live folder-ID list — updated when folders are added at runtime.
    let live_folder_ids: Arc<RwLock<Vec<uuid::Uuid>>> = Arc::new(RwLock::new(folder_ids.clone()));

    // Build snapshot provider: captures config + scheduler for each new GUI subscriber.
    let snap_config = Arc::clone(&config);
    let snap_scheduler = Arc::clone(&scheduler);
    let snapshot_provider: SnapshotProvider = Arc::new(move || {
        let cfg = Arc::clone(&snap_config);
        let sched = Arc::clone(&snap_scheduler);
        Box::pin(async move {
            let cfg = cfg.lock().await;
            let sched = sched.lock().await;
            let accounts = cfg
                .account
                .iter()
                .map(|a| AccountSnapshot {
                    account_id: a.id,
                    url: a.url.clone(),
                    display_name: a.display_name.clone(),
                    folders: a
                        .folder
                        .iter()
                        .map(|f| FolderSnapshot {
                            folder_id: f.id,
                            display_name: f.display_name.clone(),
                            local_path: f.local_path.clone(),
                            status: sched
                                .folder_status(f.id)
                                .unwrap_or_else(|| "idle".to_string()),
                        })
                        .collect(),
                })
                .collect();
            DaemonEvent::AccountSnapshot { accounts }
        })
    });

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
        if let Err(e) = gui_ipc_clone
            .run(&gui_socket_path, cmd_tx, snapshot_provider)
            .await
        {
            error!("GuiIpcServer error: {e}");
        }
    });

    gui_ipc.broadcast(DaemonEvent::Ready);

    // Launch one SpacePoller per account that has credentials.
    let space_cancel = CancellationToken::new();
    {
        let cfg = config.lock().await;
        let space_poll_interval = Duration::from_secs(cfg.general.space_poll_interval_secs);
        let tms = token_managers.read().unwrap();
        for account in &cfg.account {
            if let Some(tm) = tms.get(&account.id) {
                let poller = SpacePoller::new(
                    account.id,
                    Arc::clone(&config),
                    Arc::new(config_path.clone()),
                    Arc::clone(&gui_ipc),
                    Arc::clone(tm),
                    space_poll_interval,
                    space_cancel.clone(),
                );
                tokio::spawn(async move { poller.run().await });
            }
        }
    }

    // Spawn remote poll loop — sends TriggerSync for all currently registered folders.
    let live_ids_poll = Arc::clone(&live_folder_ids);
    let (poll_tx, mut poll_rx) = mpsc::channel::<DaemonCommand>(64);
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(poll_secs));
        ticker.tick().await; // skip first immediate tick
        loop {
            ticker.tick().await;
            let ids = live_ids_poll.read().unwrap().clone();
            info!("poll tick: {} folder(s)", ids.len());
            for id in ids {
                let _ = poll_tx
                    .send(DaemonCommand::TriggerSync { folder_id: id })
                    .await;
            }
        }
    });

    // Main loop — scheduler is now Arc<Mutex<>>, lock only when needed.
    let mut scheduler_tick = interval(Duration::from_millis(100));

    // Per-folder debounce deadlines: trigger sync 500ms after the last FS event.
    let mut debounce_map: std::collections::HashMap<uuid::Uuid, tokio::time::Instant> =
        std::collections::HashMap::new();

    info!("ocsyncd ready");

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match handle_command(
                    cmd,
                    &mut HandleContext {
                        scheduler: Arc::clone(&scheduler),
                        folder_manager: &mut folder_manager,
                        ipc: Arc::clone(&gui_ipc),
                        config: Arc::clone(&config),
                        config_path: config_path.clone(),
                        live_folder_ids: Arc::clone(&live_folder_ids),
                        token_managers: Arc::clone(&token_managers),
                        watcher_tx: watcher_tx.clone(),
                    },
                ).await {
                    Ok(ShouldQuit::Yes) => {
                        info!("Quit command received; shutting down");
                        space_cancel.cancel();
                        break;
                    }
                    Ok(ShouldQuit::No) => {}
                    Err(e) => error!("handle_command error: {e}"),
                }
            }

            Some(cmd) = poll_rx.recv() => {
                if let DaemonCommand::TriggerSync { folder_id } = cmd {
                    scheduler.lock().await.request_sync(folder_id);
                }
            }

            Some((folder_id, _event)) = watcher_rx.recv() => {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
                debounce_map.insert(folder_id, deadline);
            }

            _ = scheduler_tick.tick() => {
                let ready = {
                    let mut sched = scheduler.lock().await;
                    let ids = sched.ready_to_run();
                    for &folder_id in &ids {
                        sched.start_sync(folder_id);
                    }
                    ids
                };
                if !ready.is_empty() {
                    info!("scheduler: {} folder(s) ready to sync", ready.len());
                }
                for folder_id in ready {
                    gui_ipc.broadcast(DaemonEvent::SyncStarted { folder_id });

                    let engine = folder_manager.get_engine(folder_id).cloned();
                    let ipc = Arc::clone(&gui_ipc);
                    let sched = Arc::clone(&scheduler);
                    tokio::spawn(async move {
                        if let Some(engine) = engine {
                            info!("run_sync starting for folder {folder_id}");
                            let start = std::time::Instant::now();
                            let (errors, report) = match engine.run_sync().await {
                                Ok(r) => {
                                    info!("run_sync done for folder {folder_id}: 0 error(s)");
                                    (vec![], Some(r))
                                }
                                Err(e) => {
                                    info!("run_sync error for folder {folder_id}: {e}");
                                    let err_str = e.to_string();
                                    let partial = SyncReport {
                                        folder_id,
                                        remote_entries: 0,
                                        local_entries: 0,
                                        downloads: 0,
                                        uploads: 0,
                                        conflicts: 0,
                                        deletes_local: 0,
                                        deletes_remote: 0,
                                        ignored: 0,
                                        errors: vec![err_str.clone()],
                                        http_events: vec![],
                                        duration_ms: start.elapsed().as_millis() as u64,
                                    };
                                    (vec![err_str], Some(partial))
                                }
                            };
                            sched.lock().await.finish_sync(folder_id);
                            ipc.broadcast(DaemonEvent::SyncFinished {
                                folder_id,
                                errors,
                                report,
                            });
                        } else {
                            info!("run_sync: no engine for folder {folder_id}");
                        }
                    });
                }

                // Drain debounce entries that have reached their deadline.
                let now = tokio::time::Instant::now();
                let due: Vec<uuid::Uuid> = debounce_map
                    .iter()
                    .filter(|(_, &deadline)| now >= deadline)
                    .map(|(&id, _)| id)
                    .collect();
                for id in due {
                    debounce_map.remove(&id);
                    scheduler.lock().await.request_sync(id);
                }
            }
        }
    }

    info!("ocsyncd exiting");
    Ok(())
}
