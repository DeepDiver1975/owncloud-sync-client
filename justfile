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
acceptance:
	docker compose -f tests/docker/compose.yml up -d
	OCIS_ACCEPTANCE=1 cargo test -p acceptance-test -- --nocapture
	docker compose -f tests/docker/compose.yml down
