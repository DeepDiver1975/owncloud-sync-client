pub fn handle_share(path: &str) -> String {
    format!("SHARE:OK:{path}\n")
}

pub fn handle_copy_private_link(path: &str) -> String {
    format!("COPY_PRIVATE_LINK:OK:{path}\n")
}
