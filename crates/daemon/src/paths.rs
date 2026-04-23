use std::path::PathBuf;

pub fn platform_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    { dirs::config_dir().expect("APPDATA not set").join("ownCloud") }

    #[cfg(target_os = "macos")]
    { dirs::config_dir().expect("home dir unavailable").join("ownCloud") }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { dirs::config_dir().expect("config dir unavailable").join("owncloud") }
}

pub fn platform_lock_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir().expect("LOCALAPPDATA not set")
            .join("ownCloud").join("ocsyncd.lock")
    }

    #[cfg(target_os = "macos")]
    {
        dirs::config_dir().expect("home dir unavailable")
            .join("ownCloud").join("ocsyncd.lock")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("owncloud").join("ocsyncd.lock")
    }
}

pub fn platform_gui_socket_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
        PathBuf::from(format!(r"\\.\pipe\ownCloud-GUI-{}", username))
    }

    #[cfg(target_os = "macos")]
    {
        dirs::config_dir().expect("home dir unavailable")
            .join("ownCloud").join("daemon-gui.sock")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("owncloud").join("daemon-gui.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_is_non_empty() {
        assert!(!platform_config_dir().as_os_str().is_empty());
    }

    #[test]
    fn lock_path_is_non_empty() {
        let p = platform_lock_path();
        assert!(!p.as_os_str().is_empty());
        assert_eq!(p.file_name().unwrap(), "ocsyncd.lock");
    }

    #[test]
    fn gui_socket_path_is_non_empty() {
        assert!(!platform_gui_socket_path().as_os_str().is_empty());
    }
}
