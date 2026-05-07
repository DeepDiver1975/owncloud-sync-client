use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, Command};
use url::Url;

use crate::atspi_client::AtSpiClient;
use crate::daemon_ipc::DaemonIpcClient;
use crate::ocis_client::OcisClient;
use crate::playwright::complete_oidc_login;
use atspi::Role;
use daemon::gui_ipc::protocol::DaemonEvent;

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
    pub atspi: AtSpiClient,
    pub daemon_stdout: Lines<BufReader<tokio::process::ChildStdout>>,
    daemon: Child,
    gui: Child,
    atspi_bus: Child,
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

        // Resolve the AT-SPI2 bus address so both the GUI and the test client can find it.
        let atspi_env_val =
            resolve_atspi_bus_address().context("failed to resolve AT-SPI2 bus address")?;

        // Propagate the bus address into the environment for both this process and the GUI.
        // Safety: single-threaded at this point in startup.
        unsafe { std::env::set_var("AT_SPI_BUS_ADDRESS", &atspi_env_val) };

        // accesskit_unix watches ScreenReaderEnabled *change* events; it ignores the initial
        // value. To trigger activation, we must:
        //   1. ensure the value starts as false
        //   2. spawn the GUI (so its background thread is listening)
        //   3. flip the value to true (the change event fires and the adapter activates)
        let set_screen_reader = |enabled: bool| {
            let val = if enabled {
                "variant:boolean:true"
            } else {
                "variant:boolean:false"
            };
            let _ = StdCommand::new("dbus-send")
                .args([
                    "--session",
                    "--dest=org.a11y.Bus",
                    "/org/a11y/bus",
                    "org.freedesktop.DBus.Properties.Set",
                    "string:org.a11y.Status",
                    "string:ScreenReaderEnabled",
                    val,
                ])
                .status();
        };

        set_screen_reader(false);

        let atspi_bus = Command::new("true")
            .spawn()
            .context("failed to spawn placeholder")?;

        // Spawn GUI (pre-built with test-accessibility feature).
        // Force X11 backend by unsetting WAYLAND_DISPLAY so iced/winit doesn't try Wayland.
        // Pass DBUS_SESSION_BUS_ADDRESS explicitly: accesskit_unix uses XDG_RUNTIME_DIR/bus
        // as fallback, but we override XDG_RUNTIME_DIR to our tmp config dir, which breaks
        // the session bus lookup inside the GUI's accesskit_unix background thread.
        let dbus_session_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_else(|_| {
            let uid = nix::unistd::getuid().as_raw();
            format!("unix:path=/run/user/{uid}/bus")
        });
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":99".into());
        let gui = Command::new(bin_dir.join("ocsync"))
            .env("XDG_CONFIG_HOME", config_dir.path())
            .env("XDG_RUNTIME_DIR", config_dir.path())
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_session_addr)
            .env("DISPLAY", &display)
            .env("AT_SPI_BUS_ADDRESS", &atspi_env_val)
            .env_remove("WAYLAND_DISPLAY")
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .context("failed to spawn ocsync")?;

        // Give the GUI's accesskit_unix background thread time to subscribe, then trigger it.
        tokio::time::sleep(Duration::from_secs(2)).await;
        set_screen_reader(true);
        tokio::time::sleep(Duration::from_millis(500)).await;

        let atspi = AtSpiClient::connect()
            .await
            .context("failed to connect to AT-SPI2 accessibility bus")?;

        let ocis_client = OcisClient::from_credentials(Url::parse(OCIS_URL)?, "admin", "admin")
            .await
            .context("failed to create OcisClient")?;

        Ok(Self {
            ocis_url: Url::parse(OCIS_URL)?,
            sync_dir,
            config_dir,
            daemon_ipc,
            atspi,
            ocis_client,
            daemon_stdout,
            daemon,
            gui,
            atspi_bus,
        })
    }

    /// Runs the full account-setup flow by driving the GUI through AT-SPI2.
    /// Daemon IPC events are used only as completion signals.
    pub async fn add_account(&mut self) -> Result<()> {
        // 1. Click "Add Account" in the nav sidebar.
        let add_btn = self
            .atspi
            .wait_for_widget(Role::Button, "+ Add Account", Duration::from_secs(10))
            .await
            .context("Add Account nav button not found")?;
        self.atspi
            .click(&add_btn)
            .await
            .context("failed to click Add Account")?;

        // 2. Type the server URL into the text field (found by its placeholder text).
        let url_field = self
            .atspi
            .wait_for_widget(
                Role::Entry,
                "https://your.server.com",
                Duration::from_secs(5),
            )
            .await
            .context("server URL text input not found")?;
        self.atspi
            .set_text(&url_field, self.ocis_url.as_str())
            .await
            .context("failed to set server URL")?;

        // 3. Click "Connect →".
        let connect_btn = self
            .atspi
            .wait_for_widget(Role::Button, "Connect →", Duration::from_secs(5))
            .await
            .context("Connect button not found")?;
        self.atspi
            .click(&connect_btn)
            .await
            .context("failed to click Connect")?;

        // 4. Wait for daemon to confirm OIDC flow started.
        self.daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
                Duration::from_secs(15),
            )
            .await
            .ok_or_else(|| anyhow!("AccountAddStarted not received"))?;

        // 5. Read the OIDC authorization URL from daemon stdout.
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

        // 6. Complete OIDC login in headless browser.
        complete_oidc_login(&auth_url, callback_port, "admin", "admin")
            .await
            .context("Playwright OIDC login failed")?;

        // 7. Wait for daemon to confirm OIDC completed and account saved.
        self.daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountAddCompleted { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountAddCompleted not received"))?;

        // 8. Type the local sync folder path.
        let sync_path = self.sync_dir.path().to_string_lossy().into_owned();
        let folder_field = self
            .atspi
            .wait_for_widget(Role::Entry, "~/ownCloud", Duration::from_secs(10))
            .await
            .context("folder path text input not found")?;
        self.atspi
            .set_text(&folder_field, &sync_path)
            .await
            .context("failed to set folder path")?;

        // 9. Click "Start Syncing".
        let sync_btn = self
            .atspi
            .wait_for_widget(Role::Button, "Start Syncing", Duration::from_secs(5))
            .await
            .context("Start Syncing button not found")?;
        self.atspi
            .click(&sync_btn)
            .await
            .context("failed to click Start Syncing")?;

        // 10. Wait for daemon to confirm folder registered.
        self.daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountFolderAdded { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountFolderAdded not received"))?;

        Ok(())
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
        let _ = self.atspi_bus.start_kill();
        let _ = StdCommand::new("dbus-send")
            .args([
                "--session",
                "--dest=org.a11y.Bus",
                "/org/a11y/bus",
                "org.freedesktop.DBus.Properties.Set",
                "string:org.a11y.Status",
                "string:ScreenReaderEnabled",
                "variant:boolean:false",
            ])
            .status();
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
        .env("OCIS_BASIC_AUTH", "admin:admin")
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

/// Return the AT-SPI2 bus address.
///
/// Checks `AT_SPI_BUS_ADDRESS` first. Otherwise queries `org.a11y.Bus.GetAddress` on the
/// session bus, which triggers D-Bus service activation of `at-spi-bus-launcher` if it is
/// not already running.
fn resolve_atspi_bus_address() -> Result<String> {
    if let Ok(addr) = std::env::var("AT_SPI_BUS_ADDRESS") {
        if !addr.is_empty() {
            return Ok(addr);
        }
    }
    let output = StdCommand::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.a11y.Bus",
            "/org/a11y/bus",
            "org.a11y.Bus.GetAddress",
        ])
        .output()
        .context("failed to run dbus-send to query AT-SPI bus address")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| {
            let t = line.trim();
            if t.starts_with("string \"") && t.ends_with('"') {
                Some(t[8..t.len() - 1].to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow!(
                "could not parse AT-SPI bus address from dbus-send output: {:?}",
                stdout
            )
        })
}
