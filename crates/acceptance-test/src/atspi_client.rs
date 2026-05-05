//! AT-SPI2 client wrapper for GUI automation in acceptance tests.
//!
//! Provides [`AtSpiClient`] which connects to the AT-SPI2 D-Bus accessibility bus and
//! exposes helpers to find widgets by role/name, click them, and set text.

use anyhow::{anyhow, Context, Result};
use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::zbus::Address;
use atspi::{AccessibilityConnection, Role};
use atspi_common::ObjectRefOwned;
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use tokio::time::Instant;

/// A handle to the AT-SPI2 accessibility bus used for GUI automation.
pub struct AtSpiClient {
    conn: AccessibilityConnection,
}

impl AtSpiClient {
    /// Open a connection to the AT-SPI2 D-Bus accessibility bus.
    ///
    /// Uses `AT_SPI_BUS_ADDRESS` if set; otherwise falls back to session bus discovery.
    ///
    /// # Errors
    /// Returns an error if the AT-SPI2 bus is not available or the connection cannot be
    /// established.
    pub async fn connect() -> Result<Self> {
        let conn = if let Ok(addr_str) = std::env::var("AT_SPI_BUS_ADDRESS") {
            if !addr_str.is_empty() {
                let addr: Address = addr_str.parse().context("invalid AT_SPI_BUS_ADDRESS")?;
                AccessibilityConnection::from_address(addr)
                    .await
                    .context("failed to connect to AT-SPI2 bus at AT_SPI_BUS_ADDRESS")?
            } else {
                AccessibilityConnection::new()
                    .await
                    .context("failed to connect to AT-SPI2 accessibility bus")?
            }
        } else {
            AccessibilityConnection::new()
                .await
                .context("failed to connect to AT-SPI2 accessibility bus")?
        };
        Ok(Self { conn })
    }

    /// Search the entire accessible tree for a widget matching the given `role` and
    /// accessible `name`.
    ///
    /// Performs an iterative BFS starting from the registry root (which lists all running
    /// applications as children).  Returns the first matching [`ObjectRefOwned`] found.
    ///
    /// # Errors
    /// Returns an error if no matching widget is found, or if D-Bus communication fails.
    pub async fn find_widget(&self, role: Role, name: &str) -> Result<ObjectRefOwned> {
        let zconn = self.conn.connection();

        // The registry root's children are the application root objects.
        let registry_root = self
            .conn
            .root_accessible_on_registry()
            .await
            .context("failed to get AT-SPI registry root")?;

        let app_refs = registry_root
            .get_children()
            .await
            .context("failed to get children of AT-SPI registry root")?;

        // BFS queue of ObjectRefOwned to visit.
        let mut queue: VecDeque<ObjectRefOwned> = VecDeque::from(app_refs);
        // Track visited nodes to avoid infinite loops in cyclic graphs.
        let mut visited: HashSet<(String, String)> = HashSet::new();

        while let Some(obj_ref) = queue.pop_front() {
            if obj_ref.is_null() {
                continue;
            }

            // Check if we've already visited this node.
            let key = (
                obj_ref.name_as_str().unwrap_or("").to_string(),
                obj_ref.path_as_str().to_string(),
            );
            if !visited.insert(key) {
                continue; // already visited, skip
            }

            // Build a proxy for this node.
            let proxy: AccessibleProxy<'_> = match obj_ref.as_accessible_proxy(zconn).await {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Check role first (cheaper than name in many implementations).
            let obj_role = match proxy.get_role().await {
                Ok(r) => r,
                Err(_) => continue,
            };

            if obj_role == role {
                let obj_name = proxy.name().await.unwrap_or_default();
                if obj_name == name {
                    return Ok(obj_ref);
                }
            }

            // Enqueue children for BFS.
            if let Ok(children) = proxy.get_children().await {
                for child in children {
                    if !child.is_null() {
                        queue.push_back(child);
                    }
                }
            }
        }

        Err(anyhow!(
            "widget with role {:?} and name {:?} not found in accessibility tree",
            role,
            name
        ))
    }

    /// Poll [`find_widget`] until a widget is found or `timeout` elapses.
    ///
    /// Polls every 250 ms.
    ///
    /// # Errors
    /// Returns an error if the timeout elapses before a matching widget is found.
    pub async fn wait_for_widget(
        &self,
        role: Role,
        name: &str,
        timeout: Duration,
    ) -> Result<ObjectRefOwned> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.find_widget(role, name).await {
                Ok(widget) => return Ok(widget),
                Err(_) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(anyhow!(
                            "timed out waiting for widget with role {:?} and name {:?}",
                            role,
                            name
                        ));
                    }
                    let remaining = deadline - now;
                    tokio::time::sleep(remaining.min(Duration::from_millis(250))).await;
                }
            }
        }
    }

    /// Invoke action 0 ("click") on the given widget.
    ///
    /// # Errors
    /// Returns an error if the widget does not implement the `Action` interface, or if the
    /// D-Bus call fails.
    pub async fn click(&self, widget: &ObjectRefOwned) -> Result<()> {
        let zconn = self.conn.connection();
        let proxy = widget
            .as_accessible_proxy(zconn)
            .await
            .context("failed to build AccessibleProxy for click target")?;

        use atspi::proxy::proxy_ext::ProxyExt;
        let proxies = proxy
            .proxies()
            .await
            .context("failed to retrieve interface proxies for click target")?;

        let action_proxy = proxies
            .action()
            .await
            .context("widget does not implement the Action interface")?;

        let success = action_proxy
            .do_action(0)
            .await
            .context("do_action(0) call failed")?;

        if success {
            Ok(())
        } else {
            Err(anyhow!("do_action(0) returned false"))
        }
    }

    /// Set the text content of an editable widget.
    ///
    /// Uses the `EditableText` interface's `SetTextContents` method.
    ///
    /// # Errors
    /// Returns an error if the widget does not implement `EditableText`, or if the D-Bus call
    /// fails.
    pub async fn set_text(&self, widget: &ObjectRefOwned, value: &str) -> Result<()> {
        let zconn = self.conn.connection();
        let proxy = widget
            .as_accessible_proxy(zconn)
            .await
            .context("failed to build AccessibleProxy for set_text target")?;

        use atspi::proxy::proxy_ext::ProxyExt;
        let proxies = proxy
            .proxies()
            .await
            .context("failed to retrieve interface proxies for set_text target")?;

        let editable = proxies
            .editable_text()
            .await
            .context("widget does not implement the EditableText interface")?;

        let success = editable
            .set_text_contents(value)
            .await
            .context("set_text_contents call failed")?;

        if success {
            Ok(())
        } else {
            Err(anyhow!("set_text_contents returned false"))
        }
    }
}
