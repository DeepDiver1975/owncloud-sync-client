// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Bidirectional file sync against an ownCloud Classic (oc10) server.
//! Down-sync: a file created on the server appears locally. Up-sync: a file
//! created locally appears on the server. Both directions in a single test.
//! File names mix non-ASCII (Chinese) and HTTP-encoding chars (space, ?, <, >).

use std::time::Duration;

use acceptance_test::poll::poll_until;
use acceptance_test::testutil::{oc10_env_after_initial_sync, skip_if_no_oc10_acceptance};

#[tokio::test]
async fn test_oc10_sync_bidirectional() {
    if skip_if_no_oc10_acceptance() {
        return;
    }

    let env = oc10_env_after_initial_sync().await;
    let sync_path = env.personal_sync_dir();

    // --- DOWN-SYNC: create file on server, verify it lands locally ---
    let remote_name = "测试 file?name.txt";
    env.ocis_client
        .put(remote_name, b"down-sync-content")
        .await
        .expect("seed remote file");

    poll_until(
        || {
            let p = sync_path.join(remote_name);
            async move {
                std::fs::read(&p)
                    .map(|c| c == b"down-sync-content")
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("remote file did not sync down within 30s");

    // --- UP-SYNC: create file locally, verify it lands on the server ---
    let local_name = "上传 file<test>.txt";
    let local_file = sync_path.join(local_name);
    std::fs::write(&local_file, b"up-sync-content").expect("write local file");

    poll_until(
        || async { env.ocis_client.exists(local_name).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local file did not sync up to server within 30s");

    let remote_content = env
        .ocis_client
        .get(local_name)
        .await
        .expect("fetch synced file from server");
    assert_eq!(remote_content.as_ref(), b"up-sync-content");
}
