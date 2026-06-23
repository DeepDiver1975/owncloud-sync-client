// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, Command};
use url::Url;

use crate::daemon_ipc::DaemonIpcClient;
use crate::ocis_client::OcisClient;
use crate::playwright::{complete_oidc_login, LoginSelectors};
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent, SpaceSelection};

/// Which server backend a [`TestEnvironment`] targets. Selects the docker
/// compose stack, base URL, health endpoint, and web-login selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// oCIS (`owncloud/ocis:latest`) on `https://127.0.0.1:9200`.
    Ocis,
    /// ownCloud Classic / oc10 (`owncloud/server:latest` behind a TLS proxy) on
    /// `https://127.0.0.1:9201`.
    Oc10,
}

impl Backend {
    fn base_url(self) -> &'static str {
        match self {
            Backend::Ocis => "https://127.0.0.1:9200",
            // oc10 is fronted by a TLS proxy whose self-signed cert only carries
            // a `localhost` SAN, so the host must be `localhost` (not
            // `127.0.0.1`) for the handshake to succeed. This also matches the
            // host oc10's OAuth2 app requires in the redirect_uri.
            Backend::Oc10 => "https://localhost:9201",
        }
    }

    fn compose_path(self) -> PathBuf {
        let file = match self {
            Backend::Ocis => "compose.yml",
            Backend::Oc10 => "compose.oc10.yml",
        };
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/docker/{file}"))
    }

    /// Health endpoint polled (over HTTPS, certs ignored) until the server is
    /// ready: oCIS exposes `/health`; oc10 exposes `/status.php`.
    fn health_path(self) -> &'static str {
        match self {
            Backend::Ocis => "/health",
            Backend::Oc10 => "/status.php",
        }
    }

    fn login_selectors(self) -> LoginSelectors {
        match self {
            Backend::Ocis => LoginSelectors::OCIS,
            Backend::Oc10 => LoginSelectors::OC10,
        }
    }
}

/// Per-account results from a successful account-setup flow.
///
/// Returned by [`TestEnvironment::add_account_as`] so multi-account tests can
/// address each account independently without relying on the single-account
/// scalar fields that [`TestEnvironment::add_account`] populates.
///
/// The `personal_*` field names are retained for API compatibility, but they
/// hold whichever space was selected during setup — for accounts created via
/// [`TestEnvironment::add_account_on_space`] that is a project space, not the
/// personal one.
#[derive(Debug, Clone)]
pub struct AccountHandle {
    pub account_id: uuid::Uuid,
    pub personal_folder_id: uuid::Uuid,
    /// oCIS names the personal space after the user (e.g. "Alice").
    pub personal_space_name: String,
    /// Local root of this account's personal space:
    /// `sync_dir/<username>/<personal_space_name>`.
    pub personal_sync_dir: PathBuf,
}

/// Which space `add_account_inner` selects for `SetAccountFolders`.
enum SpaceChoice {
    /// Select the user's personal space (`drive_type == "personal"`).
    Personal,
    /// Select a project space by its (unique) name as it appears in
    /// `SpacesListed`.
    Named(String),
}

pub struct TestEnvironment {
    pub backend: Backend,
    pub ocis_url: Url,
    pub sync_dir: TempDir,
    pub config_dir: TempDir,
    pub daemon_ipc: DaemonIpcClient,
    pub ocis_client: OcisClient,
    pub daemon_stdout: Lines<BufReader<tokio::process::ChildStdout>>,
    /// Name of the personal space as reported by oCIS during `add_account`.
    /// oCIS names the personal space after the user (e.g. "Admin"), not
    /// literally "Personal", and that name is also the local sync sub-folder.
    pub personal_space_name: String,
    /// `folder_id` of the personal-space sync folder, captured from the
    /// `AccountFolderAdded` event during `add_account()`. `None` until then.
    pub personal_folder_id: Option<uuid::Uuid>,
    /// `account_id` captured from the `AccountAddCompleted` event during
    /// `add_account()`. `None` until then.
    pub account_id: Option<uuid::Uuid>,
    daemon: Child,
    gui: Child,
}

impl TestEnvironment {
    /// Starts an oCIS-backed environment. Equivalent to
    /// `start_with(Backend::Ocis)`; kept for the many existing oCIS tests.
    pub async fn start() -> Result<Self> {
        Self::start_with(Backend::Ocis).await
    }

    /// Starts an ownCloud Classic (oc10) backed environment.
    pub async fn start_oc10() -> Result<Self> {
        Self::start_with(Backend::Oc10).await
    }

    /// Brings up the docker stack for `backend`, waits for it to become healthy,
    /// then spawns the daemon and GUI against a fresh temp config. The
    /// account-setup flow is added later via [`Self::add_account`] et al.
    pub async fn start_with(backend: Backend) -> Result<Self> {
        if std::env::var("OCIS_ACCEPTANCE").is_err() {
            panic!("Set OCIS_ACCEPTANCE=1 to run acceptance tests");
        }

        let base_url = backend.base_url();

        StdCommand::new("docker")
            .args([
                "compose",
                "-f",
                &backend.compose_path().to_string_lossy(),
                "up",
                "-d",
                "--no-recreate",
            ])
            .status()
            .with_context(|| format!("failed to start {backend:?} via docker compose"))?;

        wait_server_ready(base_url, backend.health_path())
            .await
            .with_context(|| format!("{backend:?} did not become healthy"))?;

        let config_dir = TempDir::new()?;
        let sync_dir = TempDir::new()?;

        // Write minimal config (no accounts — account added later via AddAccount IPC)
        let owncloud_dir = config_dir.path().join("owncloud");
        std::fs::create_dir_all(&owncloud_dir)?;
        std::fs::write(
            owncloud_dir.join("owncloud.toml"),
            "[general]\npoll_interval_secs = 5\ninsecure = true\n",
        )?;

        // CARGO_MANIFEST_DIR is crates/acceptance-test; workspace root is two levels up
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .context("could not resolve workspace root")?;
        let bin_dir = workspace_root.join("target/debug");

        let (daemon, daemon_stdout) =
            spawn_daemon(&bin_dir, config_dir.path()).context("failed to spawn ocsyncd")?;

        // Wait for GUI socket to appear
        let socket_path = socket_path_for(config_dir.path());
        wait_for_path(&socket_path, Duration::from_secs(30))
            .await
            .context("daemon GUI socket did not appear")?;

        let daemon_ipc = DaemonIpcClient::connect(&socket_path)
            .await
            .context("failed to connect to daemon GUI socket")?;

        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":99".into());
        let dbus_session_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_else(|_| {
            let uid = nix::unistd::getuid().as_raw();
            format!("unix:path=/run/user/{uid}/bus")
        });
        let gui = Command::new(bin_dir.join("ocsync"))
            .env("XDG_CONFIG_HOME", config_dir.path())
            .env("XDG_RUNTIME_DIR", config_dir.path())
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_session_addr)
            .env("DISPLAY", &display)
            .env_remove("WAYLAND_DISPLAY")
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .context("failed to spawn ocsync")?;

        let server_url = Url::parse(base_url)?;
        let ocis_client = match backend {
            Backend::Ocis => OcisClient::from_credentials(server_url.clone(), "admin", "admin")
                .await
                .context("failed to create OcisClient")?,
            // oc10 has no Graph API; root the assertion client at the legacy
            // per-user WebDAV path for the bootstrap admin (user id `admin`).
            Backend::Oc10 => {
                OcisClient::from_credentials_oc10(server_url.clone(), "admin", "admin")
                    .context("failed to create oc10 OcisClient")?
            }
        };

        Ok(Self {
            backend,
            ocis_url: server_url,
            sync_dir,
            config_dir,
            daemon_ipc,
            ocis_client,
            daemon_stdout,
            // Populated by add_account() once the personal space is discovered.
            personal_space_name: String::new(),
            personal_folder_id: None,
            account_id: None,
            daemon,
            gui,
        })
    }

    /// Credential- and root-path-parameterized account-setup flow shared by
    /// [`Self::add_account`] (admin, rooted at `sync_dir`) and
    /// [`Self::add_account_as`] (arbitrary user, rooted at `sync_dir/<username>`).
    /// Drives the same daemon IPC path the GUI uses.
    async fn add_account_inner(
        &mut self,
        username: &str,
        password: &str,
        root_path: PathBuf,
        space: SpaceChoice,
    ) -> Result<(AccountHandle, String)> {
        // 1. Send AddAccount to the daemon.
        self.daemon_ipc
            .send(DaemonCommand::AddAccount {
                url: self.bare_url(),
            })
            .await
            .context("failed to send AddAccount")?;

        // 2. Wait for daemon to start the OIDC flow.
        self.daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
                Duration::from_secs(15),
            )
            .await
            .ok_or_else(|| anyhow!("AccountAddStarted not received"))?;

        // 3. Read the OIDC authorization URL from daemon stdout.
        let auth_url = self.wait_for_oidc_url().await?;

        let callback_port = auth_url
            .query_pairs()
            .find_map(|(k, v)| {
                if k == "redirect_uri" {
                    url::Url::parse(&v).ok().and_then(|u| u.port())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("could not extract callback port from redirect_uri"))?;

        // 4. Complete OIDC login in headless browser as the requested user.
        let callback_title = complete_oidc_login(
            &auth_url,
            callback_port,
            username,
            password,
            self.backend.login_selectors(),
        )
        .await
        .context("Playwright OIDC login failed")?;

        // 5. Wait for daemon to confirm OIDC completed and account saved.
        let completed_event = self
            .daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountAddCompleted { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountAddCompleted not received"))?;

        let account_id = match completed_event {
            DaemonEvent::AccountAddCompleted { account_id, .. } => account_id,
            _ => unreachable!(),
        };

        // 6. List spaces.
        self.daemon_ipc
            .send(DaemonCommand::ListSpaces { account_id })
            .await
            .context("failed to send ListSpaces")?;

        let spaces_event = self
            .daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::SpacesListed { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("SpacesListed not received"))?;

        let spaces = match spaces_event {
            DaemonEvent::SpacesListed { spaces, .. } => spaces,
            _ => unreachable!(),
        };
        let selected = match &space {
            SpaceChoice::Personal => spaces
                .iter()
                .find(|s| s.drive_type == "personal")
                .ok_or_else(|| anyhow!("no personal space in SpacesListed"))?,
            SpaceChoice::Named(name) => spaces
                .iter()
                .find(|s| &s.name == name)
                .ok_or_else(|| anyhow!("space {name:?} not in SpacesListed: {spaces:?}"))?,
        };
        let selected_space_name = selected.name.clone();

        // 7. Set account folders — selected space under the requested root.
        let root = root_path.to_string_lossy().into_owned();
        self.daemon_ipc
            .send(DaemonCommand::SetAccountFolders {
                account_id,
                root_path: root,
                spaces: vec![SpaceSelection {
                    space_id: selected.id.clone(),
                    display_name: selected.name.clone(),
                }],
            })
            .await
            .context("failed to send SetAccountFolders")?;

        // 8. Wait for folder added, capturing the personal folder_id.
        let folder_added = self
            .daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountFolderAdded { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountFolderAdded not received"))?;
        let personal_folder_id = match folder_added {
            DaemonEvent::AccountFolderAdded { folder_id, .. } => folder_id,
            _ => unreachable!(),
        };

        let handle = AccountHandle {
            account_id,
            personal_folder_id,
            personal_sync_dir: root_path.join(&selected_space_name),
            personal_space_name: selected_space_name,
        };
        Ok((handle, callback_title))
    }

    /// Runs the full account-setup flow for the bootstrap `admin` user, rooted
    /// directly at `sync_dir` (so `personal_sync_dir()` stays
    /// `sync_dir/<space>`), and stores the results into the single-account
    /// scalar fields used by existing tests. Returns the OIDC callback-page
    /// title. Backward-compatible with all existing callers.
    pub async fn add_account(&mut self) -> Result<String> {
        let root = self.sync_dir.path().to_path_buf();
        let (handle, title) = self
            .add_account_inner("admin", "admin", root, SpaceChoice::Personal)
            .await?;
        self.account_id = Some(handle.account_id);
        self.personal_folder_id = Some(handle.personal_folder_id);
        self.personal_space_name = handle.personal_space_name;
        Ok(title)
    }

    /// Runs the full account-setup flow for an arbitrary user, rooting the
    /// account at `sync_dir/<username>/` so multiple accounts never share a
    /// local path. Does **not** touch the single-account scalar fields. Returns
    /// the [`AccountHandle`] and the OIDC callback-page title.
    pub async fn add_account_as(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<(AccountHandle, String)> {
        let root = self.sync_dir.path().join(username);
        self.add_account_inner(username, password, root, SpaceChoice::Personal)
            .await
    }

    /// Runs the full account-setup flow for an arbitrary user, selecting a named
    /// **project space** (instead of personal) for sync. Roots the account at
    /// `sync_dir/<username>/` so accounts never share a local path; the returned
    /// [`AccountHandle`]'s `personal_sync_dir` points at the project space's
    /// local root (`sync_dir/<username>/<space_name>`). The space must already
    /// be shared with the user (e.g. via `SpaceProvisioner::assign_role`) or it
    /// will not appear in `SpacesListed` and this errors.
    pub async fn add_account_on_space(
        &mut self,
        username: &str,
        password: &str,
        space_name: &str,
    ) -> Result<(AccountHandle, String)> {
        let root = self.sync_dir.path().join(username);
        self.add_account_inner(
            username,
            password,
            root,
            SpaceChoice::Named(space_name.to_owned()),
        )
        .await
    }

    /// Returns the bare `host:port` string expected by `DaemonCommand::AddAccount`.
    pub fn bare_url(&self) -> String {
        let default_port = match self.backend {
            Backend::Ocis => 9200,
            Backend::Oc10 => 9201,
        };
        format!(
            "{}:{}",
            self.ocis_url.host_str().unwrap_or("127.0.0.1"),
            self.ocis_url.port().unwrap_or(default_port)
        )
    }

    /// Returns the local path where the personal space syncs.
    /// After SetAccountFolders, the personal space lands at
    /// sync_dir/<personal_space_name>. oCIS names the personal space after the
    /// user (e.g. "Admin"), captured during add_account().
    pub fn personal_sync_dir(&self) -> std::path::PathBuf {
        self.sync_dir.path().join(&self.personal_space_name)
    }

    /// Returns the personal-space sync folder's `folder_id`, captured during
    /// `add_account()`. Panics if called before a successful `add_account()`.
    pub fn personal_folder_id(&self) -> uuid::Uuid {
        self.personal_folder_id
            .expect("personal_folder_id not set — call add_account() first")
    }

    /// Returns the `account_id` captured during `add_account()`.
    /// Panics if called before a successful `add_account()`.
    pub fn account_id(&self) -> uuid::Uuid {
        self.account_id
            .expect("account_id not set — call add_account() first")
    }

    /// Opens a fresh GUI-IPC connection to the running daemon. The daemon sends
    /// an `AccountSnapshot` to every new subscriber, so this is the way to read
    /// the daemon's *persisted* account list after a mutation (e.g. to confirm a
    /// `RemoveAccount` actually took effect, not just that an event was broadcast).
    pub async fn connect_fresh_ipc(&self) -> Result<DaemonIpcClient> {
        let socket_path = socket_path_for(self.config_dir.path());
        DaemonIpcClient::connect(&socket_path)
            .await
            .context("failed to open fresh GUI-IPC connection")
    }

    /// Reads daemon stdout until a `OIDC_AUTH_URL=<url>` line is found, then returns the URL.
    /// Must be called after `AddAccount` is sent to the daemon.
    pub async fn wait_for_oidc_url(&mut self) -> Result<Url> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "timed out waiting for OIDC_AUTH_URL from daemon stdout"
                ));
            }
            match tokio::time::timeout(remaining, self.daemon_stdout.next_line()).await {
                Ok(Ok(Some(line))) => {
                    if let Some(url_str) = line.strip_prefix("OIDC_AUTH_URL=") {
                        return Ok(Url::parse(url_str.trim())?);
                    }
                    // Non-matching line — continue reading
                }
                Ok(Ok(None)) => {
                    return Err(anyhow!(
                        "daemon stdout closed before OIDC_AUTH_URL was emitted"
                    ));
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    return Err(anyhow!(
                        "timed out waiting for OIDC_AUTH_URL from daemon stdout"
                    ))
                }
            }
        }
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let _ = self.daemon.start_kill();
        let _ = self.gui.start_kill();
        // oCIS is kept running across tests so that keychain tokens remain valid
        // between test binaries. The CI workflow's "Stop oCIS" step tears it down.
    }
}

fn spawn_daemon(
    bin_dir: &Path,
    config_dir: &Path,
) -> Result<(Child, Lines<BufReader<tokio::process::ChildStdout>>)> {
    let mut cmd = Command::new(bin_dir.join("ocsyncd"));
    cmd.env("XDG_CONFIG_HOME", config_dir)
        .env("XDG_RUNTIME_DIR", config_dir)
        .env("OCIS_INSECURE", "1")
        .env("OCSYNCD_NO_BROWSER", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child = cmd.spawn().context("failed to spawn ocsyncd")?;
    let raw_stdout = child.stdout.take().expect("stdout was piped");
    let lines = BufReader::new(raw_stdout).lines();
    Ok((child, lines))
}

async fn wait_server_ready(base_url: &str, health_path: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;
    let health_url = format!("{base_url}{health_path}");
    crate::poll::poll_until(
        || {
            let client = client.clone();
            let url = health_url.clone();
            async move {
                client
                    .get(&url)
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .await
}

async fn wait_for_path(path: &Path, timeout: Duration) -> Result<()> {
    crate::poll::poll_until(
        || {
            let exists = path.exists();
            async move { exists }
        },
        timeout,
        Duration::from_millis(500),
    )
    .await
}

fn socket_path_for(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("owncloud").join("daemon-gui.sock")
}
