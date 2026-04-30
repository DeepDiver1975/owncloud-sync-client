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

const OCIS_URL: &str = "https://127.0.0.1:9200";
const COMPOSE_FILE: &str = "tests/docker/compose.yml";

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
            "[general]\npoll_interval_secs = 5\n",
        )?;

        // Spawn daemon with piped stdout to capture OIDC_AUTH_URL lines
        let mut daemon_cmd = Command::new("cargo");
        daemon_cmd
            .args(["run", "--bin", "ocsyncd", "--"])
            .env("XDG_CONFIG_HOME", config_dir.path())
            .env("XDG_RUNTIME_DIR", config_dir.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut daemon = daemon_cmd.spawn().context("failed to spawn ocsyncd")?;
        let daemon_raw_stdout = daemon.stdout.take().expect("stdout was piped");
        let daemon_stdout = BufReader::new(daemon_raw_stdout).lines();

        // Wait for GUI socket to appear
        let socket_path = socket_path_for(config_dir.path());
        wait_for_path(&socket_path, Duration::from_secs(30))
            .await
            .context("daemon GUI socket did not appear")?;

        let daemon_ipc = DaemonIpcClient::connect(&socket_path)
            .await
            .context("failed to connect to daemon GUI socket")?;

        // Spawn GUI (with test-accessibility feature compiled in)
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":99".into());
        let gui = Command::new("cargo")
            .args([
                "run",
                "--bin",
                "ocsync",
                "--features",
                "gui/test-accessibility",
                "--",
            ])
            .env("XDG_CONFIG_HOME", config_dir.path())
            .env("XDG_RUNTIME_DIR", config_dir.path())
            .env("DISPLAY", &display)
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

    /// Reads daemon stdout until a `OIDC_AUTH_URL=<url>` line is found, then returns the URL.
    /// Must be called after `AddAccount` is sent to the daemon.
    pub async fn wait_for_oidc_url(&mut self) -> Result<Url> {
        loop {
            match tokio::time::timeout(Duration::from_secs(30), self.daemon_stdout.next_line())
                .await
            {
                Ok(Ok(Some(line))) => {
                    if let Some(url_str) = line.strip_prefix("OIDC_AUTH_URL=") {
                        return Ok(Url::parse(url_str.trim())?);
                    }
                }
                _ => {
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
        // Tear down synchronously — Drop is not async
        let _ = StdCommand::new("docker")
            .args(["compose", "-f", COMPOSE_FILE, "down"])
            .status();
    }
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
