pub fn status_to_emblem(status: &str) -> &'static str {
    match status {
        "OK" => "emblem-default",
        "SYNC" => "emblem-synchronizing",
        "WARNING" => "emblem-important",
        "ERROR" => "emblem-problem",
        "EXCLUDED" => "emblem-readonly",
        _ => "",
    }
}
