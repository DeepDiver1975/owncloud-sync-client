fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    build_icon(&out_dir);
    build_info(&out_dir);
    track_locales();
}

fn build_icon(out_dir: &str) {
    let svg_path = "assets/owncloud-icon.svg";
    println!("cargo:rerun-if-changed={svg_path}");

    let svg_data = std::fs::read_to_string(svg_path)
        .unwrap_or_else(|e| panic!("failed to read {svg_path}: {e}"));

    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg_data, &opt)
        .unwrap_or_else(|e| panic!("failed to parse SVG: {e}"));

    let size = 16u32;
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("failed to allocate pixmap");

    let scale = f32::min(
        size as f32 / tree.size().width(),
        size as f32 / tree.size().height(),
    );
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let out_path = std::path::Path::new(out_dir).join("owncloud-icon-16.png");
    let file = std::fs::File::create(&out_path)
        .unwrap_or_else(|e| panic!("failed to create output PNG: {e}"));

    let mut encoder = png::Encoder::new(file, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .unwrap_or_else(|e| panic!("failed to write PNG header: {e}"));
    writer
        .write_image_data(pixmap.data())
        .unwrap_or_else(|e| panic!("failed to write PNG data: {e}"));
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn build_info(out_dir: &str) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let lock_path = std::path::Path::new(&manifest_dir).join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());

    let app_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into());
    let build_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "unknown".into());
    let os_name = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".into());

    let os_version = detect_os_version(&os_name);

    // Parse library versions from Cargo.lock
    let lock_contents = std::fs::read_to_string(&lock_path).unwrap_or_default();
    let lib_iced = parse_lock_version(&lock_contents, "iced").unwrap_or_else(|| "unknown".into());
    let lib_rustls =
        parse_lock_version(&lock_contents, "rustls").unwrap_or_else(|| "unknown".into());
    let lib_sqlite =
        parse_lock_version(&lock_contents, "libsqlite3-sys").unwrap_or_else(|| "unknown".into());

    let contributors = "Thomas Müller and Claude Code";

    let content = format!(
        r#"pub const APP_VERSION: &str = "{app_version}";
pub const BUILD_ARCH: &str = "{build_arch}";
pub const CPU_ARCH: &str = "{build_arch}";
pub const OS_NAME: &str = "{os_name}";
pub const OS_VERSION: &str = "{os_version}";
pub const LIB_ICED: &str = "{lib_iced}";
pub const LIB_RUSTLS: &str = "{lib_rustls}";
pub const LIB_SQLITE: &str = "{lib_sqlite}";
pub const CONTRIBUTORS: &str = "{contributors}";
"#,
        app_version = escape_str(&app_version),
        build_arch = escape_str(&build_arch),
        os_name = escape_str(&os_name),
        os_version = escape_str(&os_version),
        lib_iced = escape_str(&lib_iced),
        lib_rustls = escape_str(&lib_rustls),
        lib_sqlite = escape_str(&lib_sqlite),
        contributors = escape_str(contributors),
    );

    let out_path = std::path::Path::new(out_dir).join("build_info.rs");
    std::fs::write(&out_path, content)
        .unwrap_or_else(|e| panic!("failed to write build_info.rs: {e}"));
}

fn detect_os_version(os_name: &str) -> String {
    // Queries the build host OS version — correct for native builds, not cross-compilation.
    match os_name {
        "macos" => std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".into()),
        "linux" => std::process::Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".into()),
        _ => "unknown".into(),
    }
}

fn parse_lock_version(lock: &str, package: &str) -> Option<String> {
    // Scan stanza by stanza; rustls has two versions — returns the first (lowest).
    let needle = format!("name = \"{package}\"");
    let mut in_stanza = false;
    for line in lock.lines() {
        if line == "[[package]]" {
            in_stanza = false;
        }
        if line == needle {
            in_stanza = true;
            continue;
        }
        if in_stanza {
            if let Some(rest) = line.strip_prefix("version = \"") {
                return Some(rest.trim_end_matches('"').to_string());
            }
        }
    }
    None
}

fn track_locales() {
    for entry in std::fs::read_dir("locales")
        .unwrap_or_else(|e| panic!("failed to read locales dir: {e}"))
        .flatten()
    {
        println!("cargo:rerun-if-changed={}", entry.path().display());
    }
}
