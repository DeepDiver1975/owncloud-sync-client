// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

use crate::protocol::format_response;
use crate::status_resolver::StatusResolver;

pub fn handle_retrieve_file_status(path: &str, resolver: &StatusResolver) -> String {
    let tag = resolver.resolve_file(path);
    format_response("STATUS", &[tag, path])
}

pub fn handle_retrieve_folder_status(path: &str, resolver: &StatusResolver) -> String {
    let tag = resolver.resolve_folder(path);
    format_response("STATUS", &[tag, path])
}
