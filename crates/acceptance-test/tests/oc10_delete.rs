// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Bidirectional deletion against an ownCloud Classic (oc10) server: a local
//! delete propagates up; a server delete propagates down. A sibling file is
//! asserted to survive both deletions. Names mix non-ASCII and HTTP-encoding
//! chars.

use std::time::Duration;

use acceptance_test::poll::poll_until;
use acceptance_test::testutil::{oc10_env_after_initial_sync, skip_if_no_oc10_acceptance};

#[tokio::test]
async fn test_oc10_delete_file_bidirectional() {
    if skip_if_no_oc10_acceptance() {
        return;
    }

    let env = oc10_env_after_initial_sync().await;
    let sync_dir = env.personal_sync_dir();

    let up_file = "删除 up?file.txt"; // deleted locally -> expect gone on server
    let survivor = "幸存 keep<file>.txt"; // must remain after both deletes
    let down_file = "删除 down file.txt"; // deleted on server -> expect gone locally

    // Seed all three on the server, wait for them to sync down.
    for name in [up_file, survivor, down_file] {
        env.ocis_client
            .put(name, b"seed")
            .await
            .unwrap_or_else(|_| panic!("seed {name}"));
    }
    for name in [up_file, survivor, down_file] {
        let p = sync_dir.join(name);
        poll_until(
            || {
                let p = p.clone();
                async move { p.exists() }
            },
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} did not sync down"));
    }

    // --- UP: delete locally, expect gone on server ---
    std::fs::remove_file(sync_dir.join(up_file)).expect("remove local up_file");
    poll_until(
        || async { !env.ocis_client.exists(up_file).await.unwrap_or(true) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local deletion did not propagate to server");

    // --- DOWN: delete on server, expect gone locally ---
    env.ocis_client
        .delete(down_file)
        .await
        .expect("server delete");
    poll_until(
        || {
            let p = sync_dir.join(down_file);
            async move { !p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server deletion did not propagate to client");

    // --- SURVIVOR: untouched on both ends ---
    assert!(
        env.ocis_client.exists(survivor).await.unwrap_or(false),
        "survivor must still exist on server"
    );
    assert!(
        sync_dir.join(survivor).exists(),
        "survivor must still exist locally"
    );
}
