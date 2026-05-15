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
use crate::playwright::complete_oidc_login;
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent, SpaceSelection};

const OCIS_URL: &str = "https://127.0.0.1:9200";

fn compose_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/docker/compose.yml")
}

pub struct TestEnvironment {
    pub ocis_url: Url,
    pub sync_dir: TempDir,
    pub config_dir: TempDir,
    pub daemon_ipc: DaemonIpcClient,
    pub ocis_client: OcisClient,
    pub daemon_stdout: Lines<BufReader<tokio::process::ChildStdout>>,
    daemon: Child,
    gui: Child,
}

impl TestEnvironment {
    pub async fn start() -> Result<Self> {
        if std::env::var("OCIS_ACCEPTANCE").is_err() {
            panic!("Set OCIS_ACCEPTANCE=1 to run acceptance tests");
        }

        StdCommand::new("docker")
            .args([
                "compose",
                "-f",
                &compose_file().to_string_lossy(),
                "up",
                "-d",
                "--no-recreate",
            ])
            .status()
            .context("failed to start oCIS via docker compose")?;

        wait_ocis_ready(OCIS_URL)
            .await
            .context("oCIS did not become healthy")?;

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

        let ocis_client = OcisClient::from_credentials(Url::parse(OCIS_URL)?, "admin", "admin")
            .await
            .context("failed to create OcisClient")?;

        Ok(Self {
            ocis_url: Url::parse(OCIS_URL)?,
            sync_dir,
            config_dir,
            daemon_ipc,
            ocis_client,
            daemon_stdout,
            daemon,
            gui,
        })
    }

    /// Runs the full account-setup flow via daemon IPC.
    /// The GUI is running in the background; IPC commands reach the daemon through
    /// the same socket the GUI uses, exercising the same daemon code path.
    pub async fn add_account(&mut self) -> Result<String> {
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

        // 4. Complete OIDC login in headless browser.
        let callback_title = complete_oidc_login(&auth_url, callback_port, "admin", "admin")
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
        let personal = spaces
            .iter()
            .find(|s| s.drive_type == "personal")
            .ok_or_else(|| anyhow!("no personal space in SpacesListed"))?;

        // 7. Set account folders — personal space as a sub-folder of sync_dir.
        let root = self.sync_dir.path().to_string_lossy().into_owned();
        self.daemon_ipc
            .send(DaemonCommand::SetAccountFolders {
                account_id,
                root_path: root,
                spaces: vec![SpaceSelection {
                    space_id: personal.id.clone(),
                    display_name: personal.name.clone(),
                }],
            })
            .await
            .context("failed to send SetAccountFolders")?;

        // 8. Wait for folder added.
        self.daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountFolderAdded { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountFolderAdded not received"))?;

        Ok(callback_title)
    }

    /// Returns the bare `host:port` string expected by `DaemonCommand::AddAccount`.
    pub fn bare_url(&self) -> String {
        format!(
            "{}:{}",
            self.ocis_url.host_str().unwrap_or("127.0.0.1"),
            self.ocis_url.port().unwrap_or(9200)
        )
    }

    /// Returns the local path where the personal space syncs.
    /// After SetAccountFolders, the personal space lands at sync_dir/<personal_name>.
    /// oCIS personal spaces are named "Personal" by default.
    pub fn personal_sync_dir(&self) -> std::path::PathBuf {
        self.sync_dir.path().join("Personal")
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

async fn wait_ocis_ready(base_url: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;
    let health_url = format!("{base_url}/health");
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
