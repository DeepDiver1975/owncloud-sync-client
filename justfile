# ownCloud Sync Client — top-level justfile
#
# Prerequisites:
#   cargo, rustup                 (Rust toolchain)
#   just                          (https://github.com/casey/just)
#   xcodebuild                    (macOS only — Xcode command-line tools)
#   cross                         (optional — `cargo install cross` for Docker-based cross-compilation)

# Default: list all recipes
default:
    @just --list

# ---------------------------------------------------------------------------
# Core workspace (Linux + macOS host)
# ---------------------------------------------------------------------------

# Build the main workspace in debug mode
build:
    cargo build --workspace

# Build in release mode
build-release:
    cargo build --workspace --release

# Run all main-workspace tests
test:
    cargo test --workspace

# Run tests with output shown for failing tests
test-verbose:
    cargo test --workspace -- --nocapture

# Lint with clippy
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format all Rust source
fmt:
    cargo fmt --all

# Check formatting without writing
fmt-check:
    cargo fmt --all -- --check

# Run fmt + lint + test in sequence (CI-equivalent for the main workspace)
ci: fmt-check lint test

# ---------------------------------------------------------------------------
# Linux D-Bus service (already a main-workspace member)
# ---------------------------------------------------------------------------

# Build only the Linux D-Bus service
build-linux:
    cargo build -p oc-dbus-service

test-linux:
    cargo test -p oc-dbus-service

# ---------------------------------------------------------------------------
# Windows shell-integration (separate Cargo workspace)
# ---------------------------------------------------------------------------

windows-dir := "shell-integration/windows"

# Build the Windows shell-integration workspace (requires Windows host or cross)
build-windows:
    cargo build --manifest-path {{windows-dir}}/Cargo.toml --workspace

# Test the Windows workspace (menu_builder tests run on any host; ipc/overlay need Windows)
test-windows:
    cargo test --manifest-path {{windows-dir}}/Cargo.toml --workspace

# Lint the Windows workspace
lint-windows:
    cargo clippy --manifest-path {{windows-dir}}/Cargo.toml --workspace --all-targets -- -D warnings

# Cross-compile Windows DLLs for x86_64 (requires `cross` + Docker)
build-windows-cross:
    cross build --manifest-path {{windows-dir}}/Cargo.toml \
        --target x86_64-pc-windows-msvc --release

# ---------------------------------------------------------------------------
# macOS shell-integration (Swift / Xcode — macOS host only)
# ---------------------------------------------------------------------------

macos-findersync := "shell-integration/macos/FinderSync"
macos-fileprovider := "shell-integration/macos/FileProvider"

# Build the FinderSync extension (macOS only)
build-macos-findersync:
    xcodebuild -project {{macos-findersync}}/FinderSync.xcodeproj \
               -scheme FinderSync \
               -configuration Debug \
               build

# Run FinderSync XCTest suite (macOS only)
test-macos-findersync:
    xcodebuild test \
               -project {{macos-findersync}}/FinderSync.xcodeproj \
               -scheme FinderSync \
               -destination 'platform=macOS'

# Build the FileProvider extension (macOS only)
build-macos-fileprovider:
    xcodebuild -project {{macos-fileprovider}}/FileProvider.xcodeproj \
               -scheme FileProvider \
               -configuration Debug \
               build

# Run FileProvider XCTest suite (macOS only)
test-macos-fileprovider:
    xcodebuild test \
               -project {{macos-fileprovider}}/FileProvider.xcodeproj \
               -scheme FileProvider \
               -destination 'platform=macOS'

# Build + test both macOS extensions
macos: build-macos-findersync build-macos-fileprovider test-macos-findersync test-macos-fileprovider

# ---------------------------------------------------------------------------
# Housekeeping
# ---------------------------------------------------------------------------

# Remove all build artefacts (main workspace + Windows sub-workspace)
clean:
    cargo clean
    cargo clean --manifest-path {{windows-dir}}/Cargo.toml

# Check for outdated dependencies (requires `cargo install cargo-outdated`)
outdated:
    cargo outdated --workspace
    cargo outdated --manifest-path {{windows-dir}}/Cargo.toml

# ---------------------------------------------------------------------------
# Acceptance tests (requires Docker, Node.js, Playwright)
# ---------------------------------------------------------------------------

# Install Playwright and Chromium (run once)
acceptance-setup:
	npm install playwright
	npx playwright install chromium

# Run acceptance tests (requires Docker and a display server)
# Docker Compose is started/stopped automatically by the test fixture.
acceptance:
	OCIS_ACCEPTANCE=1 cargo test -p acceptance-test -- --nocapture

# ---------------------------------------------------------------------------
# Packaging
# ---------------------------------------------------------------------------

# Build the Linux .deb package (requires cargo-deb: cargo install cargo-deb)
package-linux:
    cargo build --release --workspace
    cargo deb --manifest-path crates/daemon/Cargo.toml --no-build

# Build the macOS .pkg installer (macOS only — requires Xcode CLI tools)
package-macos:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release --workspace
    # Build Swift extensions (continue-on-error since Xcode projects may not exist yet)
    xcodebuild archive \
      -project shell-integration/macos/FinderSync/FinderSync.xcodeproj \
      -scheme FinderSync -archivePath /tmp/FinderSync.xcarchive \
      CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO || true
    xcodebuild archive \
      -project shell-integration/macos/FileProvider/FileProvider.xcodeproj \
      -scheme FileProvider -archivePath /tmp/FileProvider.xcarchive \
      CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO || true
    # Assemble app bundle
    rm -rf staging/ocsync.app
    mkdir -p staging/ocsync.app/Contents/MacOS
    mkdir -p staging/ocsync.app/Contents/PlugIns
    cp target/release/ocsync staging/ocsync.app/Contents/MacOS/
    cp target/release/ocsyncd staging/ocsync.app/Contents/MacOS/
    # Copy extensions if archives succeeded
    [ -d /tmp/FinderSync.xcarchive/Products ] && \
      cp -r /tmp/FinderSync.xcarchive/Products/Applications/FinderSync.appex \
            staging/ocsync.app/Contents/PlugIns/ || true
    [ -d /tmp/FileProvider.xcarchive/Products ] && \
      cp -r /tmp/FileProvider.xcarchive/Products/Applications/FileProvider.appex \
            staging/ocsync.app/Contents/PlugIns/ || true
    # Create minimal Info.plist
    cat > staging/ocsync.app/Contents/Info.plist <<'PLIST'
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0"><dict>
      <key>CFBundleIdentifier</key><string>com.owncloud.sync</string>
      <key>CFBundleName</key><string>ownCloud Sync</string>
      <key>CFBundleVersion</key><string>0.1.0</string>
      <key>CFBundleExecutable</key><string>ocsync</string>
      <key>LSUIElement</key><true/>
    </dict></plist>
    PLIST
    pkgbuild \
      --root staging/ocsync.app \
      --install-location /Applications/ownCloud.app \
      --scripts packaging/macos/scripts \
      --identifier com.owncloud.sync \
      --version 0.1.0 \
      component.pkg
    productbuild \
      --distribution packaging/macos/Distribution.xml \
      --package-path . \
      owncloud.pkg
    echo "Built: owncloud.pkg"

# Build the Windows MSI installer (Windows only — requires dotnet + wix)
package-windows:
    cargo build --release --manifest-path shell-integration/windows/Cargo.toml --workspace
    cargo build --release -p daemon -p gui
    dotnet tool install --global wix || true
    wix build packaging/windows/owncloud.wxs \
      -d "BinDir=target/release" \
      -o owncloud-setup.msi
    echo "Built: owncloud-setup.msi"

# Build all platform packages sequentially
package: package-linux package-macos package-windows
