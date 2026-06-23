// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Bidirectional edit against an ownCloud Classic (oc10) server: a local
//! overwrite propagates up; a server overwrite propagates down. Both directions
//! in a single test. Names mix non-ASCII and HTTP-encoding chars.

use std::time::Duration;

use acceptance_test::poll::poll_until;
use acceptance_test::testutil::{oc10_env_after_initial_sync, skip_if_no_oc10_acceptance};

#[tokio::test]
async fn test_oc10_edit_file_bidirectional() {
    if skip_if_no_oc10_acceptance() {
        return;
    }

    let env = oc10_env_after_initial_sync().await;
    let sync_dir = env.personal_sync_dir();
    let up_name = "编辑 up?file.txt"; // edited locally
    let down_name = "编辑 down<file>.txt"; // edited on server

    // Seed both on the server, wait for them to sync down with seed content.
    env.ocis_client
        .put(up_name, b"original up")
        .await
        .expect("seed up");
    env.ocis_client
        .put(down_name, b"original down")
        .await
        .expect("seed down");
    for (name, content) in [
        (up_name, b"original up" as &[u8]),
        (down_name, b"original down"),
    ] {
        let p = sync_dir.join(name);
        poll_until(
            || {
                let p = p.clone();
                async move { std::fs::read(&p).map(|c| c == content).unwrap_or(false) }
            },
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} did not sync down with seed content"));
    }

    // --- UP: overwrite locally, expect server content updated ---
    std::fs::write(sync_dir.join(up_name), b"edited up content").expect("overwrite local");
    poll_until(
        || async {
            env.ocis_client
                .get(up_name)
                .await
                .map(|b| b.as_ref() == b"edited up content")
                .unwrap_or(false)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local edit did not propagate to server");

    // --- DOWN: overwrite on server, expect local content updated ---
    env.ocis_client
        .put(down_name, b"edited down content")
        .await
        .expect("server overwrite");
    poll_until(
        || {
            let p = sync_dir.join(down_name);
            async move {
                std::fs::read(&p)
                    .map(|c| c == b"edited down content")
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server edit did not propagate to client");
}
