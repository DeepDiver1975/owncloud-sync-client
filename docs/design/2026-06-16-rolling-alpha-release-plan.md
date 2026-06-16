# Rolling Alpha Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On each merge to `main`, publish/refresh a single rolling `alpha` GitHub pre-release carrying `.deb`/`.pkg`/`.msi` packages, plus an immutable `alpha-<shortsha>` history tag — with the build matrix extracted into one reusable workflow that `package.yml`, `release.yml`, and the new `alpha.yml` all call.

**Architecture:** A reusable `build-packages.yml` (`workflow_call`, single `version_label` input) holds the three per-platform build jobs. `package.yml` (PR/push validation) and `alpha.yml` (push-to-main publish) each compute a short-sha label in a tiny `prepare` job and call the reusable workflow; `release.yml` (tag push) calls it with the tag name. `alpha.yml` adds a `publish-alpha` job that force-moves the `alpha` git tag, pushes an immutable `alpha-<shortsha>` tag, and edits the rolling `alpha` release in place.

**Tech Stack:** GitHub Actions (reusable workflows / `workflow_call`), `gh` CLI, `cargo-deb`, `pkgbuild`/`productbuild`, WiX 4, `actionlint`.

**Reference spec:** `docs/design/2026-06-16-rolling-alpha-release-design.md`

---

## File Structure

```
.github/workflows/
  build-packages.yml   CREATE  reusable (workflow_call): 3 build jobs, uploads linux-deb/macos-pkg/windows-msi
  package.yml          REWRITE prepare(short sha) + call build-packages.yml  (PR + push validation)
  release.yml          REWRITE call build-packages.yml + publish tagged release
  alpha.yml            CREATE  prepare + call build-packages.yml + publish-alpha (rolling release + tags)
```

**Lint convention used by every task:** prefer `actionlint`; if it is not installed and cannot be fetched, fall back to a YAML parse. The exact command block is:

```bash
if command -v actionlint >/dev/null 2>&1; then
  actionlint .github/workflows/*.yml
else
  echo "actionlint not found — trying to fetch it"
  if curl -fsSL https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash -o /tmp/dl-actionlint.bash 2>/dev/null; then
    bash /tmp/dl-actionlint.bash >/dev/null && ./actionlint .github/workflows/*.yml && rm -f ./actionlint
  else
    echo "no network for actionlint — falling back to YAML parse"
    for f in .github/workflows/*.yml; do python3 -c "import sys,yaml;yaml.safe_load(open('$f'))" && echo "OK $f"; done
  fi
fi
```

This is referenced below as **\[LINT\]**.

---

## Task 1: Create the reusable build workflow

**Files:**
- Create: `.github/workflows/build-packages.yml`

- [ ] **Step 1: Write the workflow file**

Create `.github/workflows/build-packages.yml` with exactly this content:

```yaml
name: Build Packages

# Reusable build workflow. Builds the three platform packages and uploads them
# as artifacts named linux-deb / macos-pkg / windows-msi. Callers (package.yml,
# release.yml, alpha.yml) pass a version_label that is embedded in the asset
# filenames. Internal version fields (Cargo.toml, Info.plist, pkgbuild --version,
# WiX) are intentionally NOT rewritten — see the design doc's non-goals.
on:
  workflow_call:
    inputs:
      version_label:
        description: "Label embedded in artifact filenames (e.g. 0.1.0-alpha.<sha> or v0.2.0)"
        required: true
        type: string

env:
  CARGO_TERM_COLOR: always

permissions:
  contents: read

jobs:
  package-linux:
    name: Package Linux (.deb)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Install system dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libdbus-1-dev pkg-config libsecret-1-dev \
            libgtk-3-dev libayatana-appindicator3-dev libxdo-dev

      - name: Build release binaries
        run: cargo build --release --workspace

      - name: Install cargo-deb
        run: cargo install cargo-deb --locked

      - name: Build .deb
        run: cargo deb --manifest-path crates/daemon/Cargo.toml --no-build

      - name: Rename artifact with version label
        run: |
          DEB=$(ls target/debian/*.deb | head -1)
          cp "$DEB" "owncloud-sync_${{ inputs.version_label }}_amd64.deb"

      - uses: actions/upload-artifact@v7
        with:
          name: linux-deb
          path: "owncloud-sync_*.deb"
          retention-days: 30

  package-macos:
    name: Package macOS (.pkg)
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v6

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Build release binaries
        run: cargo build --release --workspace

      - name: Build Swift extensions
        run: |
          xcodebuild archive \
            -project shell-integration/macos/FinderSync/FinderSync.xcodeproj \
            -scheme FinderSync -archivePath /tmp/FinderSync.xcarchive \
            CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO || true
          xcodebuild archive \
            -project shell-integration/macos/FileProvider/FileProvider.xcodeproj \
            -scheme FileProvider -archivePath /tmp/FileProvider.xcarchive \
            CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO || true

      - name: Assemble app bundle
        run: |
          rm -rf staging/ocsync.app
          mkdir -p staging/ocsync.app/Contents/MacOS
          mkdir -p staging/ocsync.app/Contents/PlugIns
          cp target/release/ocsync staging/ocsync.app/Contents/MacOS/
          cp target/release/ocsyncd staging/ocsync.app/Contents/MacOS/
          [ -d /tmp/FinderSync.xcarchive/Products ] && \
            cp -r /tmp/FinderSync.xcarchive/Products/Applications/FinderSync.appex \
                  staging/ocsync.app/Contents/PlugIns/ || true
          [ -d /tmp/FileProvider.xcarchive/Products ] && \
            cp -r /tmp/FileProvider.xcarchive/Products/Applications/FileProvider.appex \
                  staging/ocsync.app/Contents/PlugIns/ || true
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

      - name: Build component.pkg
        run: |
          pkgbuild \
            --root staging/ocsync.app \
            --install-location /Applications/ownCloud.app \
            --scripts packaging/macos/scripts \
            --identifier com.owncloud.sync \
            --version 0.1.0 \
            component.pkg

      - name: Build distribution .pkg
        run: |
          productbuild \
            --distribution packaging/macos/Distribution.xml \
            --package-path . \
            "owncloud-${{ inputs.version_label }}.pkg"

      - uses: actions/upload-artifact@v7
        with:
          name: macos-pkg
          path: "owncloud-*.pkg"
          retention-days: 30

  package-windows:
    name: Package Windows (.msi)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v6

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Build release binaries (main workspace)
        run: cargo build --release -p daemon -p gui

      - name: Build release DLLs (shell-integration)
        run: cargo build --release --manifest-path shell-integration/windows/Cargo.toml --workspace

      - name: Install WiX 4
        # Pin to 4.x: the .wxs uses the v4 schema, and WiX v6/v7 added a
        # mandatory OSMF EULA gate (error WIX7015) that breaks unattended CI.
        run: dotnet tool install --global wix --version 4.0.6

      - name: Refresh PATH for WiX
        run: echo "$env:USERPROFILE\.dotnet\tools" | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append

      - name: Build MSI
        run: |
          # TODO: sign with signtool.exe /f cert.pfx /p $env:CERT_PASSWORD owncloud-setup-*.msi
          # Main workspace binaries (ocsync, ocsyncd) and the shell-integration
          # DLLs live in two separate cargo workspaces with separate target dirs.
          wix build packaging/windows/owncloud.wxs `
            -d "BinDir=target/release" `
            -d "ShellBinDir=shell-integration/windows/target/release" `
            -o "owncloud-setup-${{ inputs.version_label }}.msi"

      - uses: actions/upload-artifact@v7
        with:
          name: windows-msi
          path: "*.msi"
          retention-days: 30
```

- [ ] **Step 2: Lint the new workflow**

Run **\[LINT\]** (the block defined in the File Structure section).
Expected: no errors; `actionlint` exits 0 (or YAML-parse prints `OK .github/workflows/build-packages.yml`).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/build-packages.yml
git commit -s -m "ci: add reusable build-packages workflow (#67)"
```

---

## Task 2: Refactor package.yml to call the reusable workflow

**Files:**
- Modify (full rewrite): `.github/workflows/package.yml`

- [ ] **Step 1: Replace the file content**

Overwrite `.github/workflows/package.yml` with exactly this content. The three inline build jobs are gone; a `prepare` job computes a short-sha label and `build` calls the reusable workflow.

```yaml
name: Package

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  prepare:
    name: Compute version label
    runs-on: ubuntu-latest
    outputs:
      version_label: ${{ steps.label.outputs.version_label }}
    steps:
      - id: label
        run: echo "version_label=${GITHUB_SHA::7}" >> "$GITHUB_OUTPUT"

  build:
    needs: prepare
    uses: ./.github/workflows/build-packages.yml
    with:
      version_label: ${{ needs.prepare.outputs.version_label }}
```

- [ ] **Step 2: Lint**

Run **\[LINT\]**.
Expected: exits 0 / `OK` for all workflow files.

- [ ] **Step 3: Sanity-check the wiring by eye**

Confirm: `build` has `needs: prepare`, uses `./.github/workflows/build-packages.yml`, and passes `version_label`. There are no remaining `package-linux`/`package-macos`/`package-windows` jobs in this file.

Run: `grep -c "package-linux\|package-macos\|package-windows" .github/workflows/package.yml`
Expected: `0`

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/package.yml
git commit -s -m "ci: package.yml calls reusable build-packages workflow (#67)"
```

---

## Task 3: Refactor release.yml to call the reusable workflow

**Files:**
- Modify (full rewrite): `.github/workflows/release.yml`

- [ ] **Step 1: Replace the file content**

Overwrite `.github/workflows/release.yml` with exactly this content. The three inline build jobs are replaced by a single `build` job calling the reusable workflow with the tag name as the label; the `publish` job is preserved.

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: read

jobs:
  build:
    uses: ./.github/workflows/build-packages.yml
    with:
      version_label: ${{ github.ref_name }}

  publish:
    name: Publish GitHub Release
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v6

      - uses: actions/download-artifact@v8
        with:
          merge-multiple: true
          path: dist/

      - name: Create GitHub Release
        run: |
          gh release create "${{ github.ref_name }}" \
            dist/*.deb dist/*.pkg dist/*.msi \
            --generate-notes \
            --title "ownCloud Sync ${{ github.ref_name }}"
        env:
          GH_TOKEN: ${{ github.token }}
```

- [ ] **Step 2: Lint**

Run **\[LINT\]**.
Expected: exits 0 / `OK` for all workflow files.

- [ ] **Step 3: Confirm no inline build jobs remain**

Run: `grep -c "runs-on: macos-latest\|runs-on: windows-latest" .github/workflows/release.yml`
Expected: `0` (the build now happens in the reusable workflow).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -s -m "ci: release.yml calls reusable build-packages workflow (#67)"
```

---

## Task 4: Create alpha.yml (rolling alpha publisher)

**Files:**
- Create: `.github/workflows/alpha.yml`

- [ ] **Step 1: Write the workflow file**

Create `.github/workflows/alpha.yml` with exactly this content:

```yaml
name: Alpha Release

# On each merge to main: build the three packages, push an immutable
# alpha-<shortsha> history tag, force-move the rolling `alpha` tag, and refresh
# the single `alpha` pre-release in place (delete stale assets, upload new ones).
on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

# Serialize publishes so two quick merges can't race on the force-moved tag or
# the clobbered release assets — the later run queues behind the earlier one.
concurrency:
  group: alpha-release
  cancel-in-progress: false

jobs:
  prepare:
    name: Compute version label
    runs-on: ubuntu-latest
    outputs:
      short_sha: ${{ steps.label.outputs.short_sha }}
      version_label: ${{ steps.label.outputs.version_label }}
    steps:
      - id: label
        run: |
          SHORT="${GITHUB_SHA::7}"
          echo "short_sha=$SHORT" >> "$GITHUB_OUTPUT"
          echo "version_label=0.1.0-alpha.$SHORT" >> "$GITHUB_OUTPUT"

  build:
    needs: prepare
    uses: ./.github/workflows/build-packages.yml
    with:
      version_label: ${{ needs.prepare.outputs.version_label }}

  publish-alpha:
    name: Publish rolling alpha release
    needs: [prepare, build]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    env:
      GH_TOKEN: ${{ github.token }}
      SHORT_SHA: ${{ needs.prepare.outputs.short_sha }}
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0

      - uses: actions/download-artifact@v8
        with:
          merge-multiple: true
          path: dist/

      - name: Push immutable history tag
        run: |
          TAG="alpha-${SHORT_SHA}"
          if git ls-remote --tags origin "refs/tags/${TAG}" | grep -q .; then
            echo "History tag ${TAG} already exists — skipping."
          else
            git tag "${TAG}" "${GITHUB_SHA}"
            git push origin "${TAG}"
          fi

      - name: Force-move rolling alpha tag
        run: |
          git tag -f alpha "${GITHUB_SHA}"
          git push -f origin alpha

      - name: Publish / refresh the alpha pre-release
        run: |
          BUILT_AT="$(date -u +'%Y-%m-%d %H:%M UTC')"
          NOTES="Auto-updated bleeding-edge build from \`main\`.
          Commit: \`${SHORT_SHA}\` · Built: ${BUILT_AT}

          > These packages are unsigned alpha builds for testing only."
          if gh release view alpha >/dev/null 2>&1; then
            echo "Updating existing alpha release — removing stale assets."
            for asset in $(gh release view alpha --json assets --jq '.assets[].name'); do
              gh release delete-asset alpha "$asset" --yes
            done
            gh release upload alpha dist/*.deb dist/*.pkg dist/*.msi --clobber
            gh release edit alpha \
              --prerelease \
              --title "ownCloud Sync (alpha)" \
              --notes "${NOTES}"
          else
            echo "Creating alpha release for the first time."
            gh release create alpha \
              dist/*.deb dist/*.pkg dist/*.msi \
              --prerelease \
              --title "ownCloud Sync (alpha)" \
              --notes "${NOTES}"
          fi
```

- [ ] **Step 2: Lint**

Run **\[LINT\]**.
Expected: exits 0 / `OK` for all workflow files.

- [ ] **Step 3: Verify the publish job's key invariants by eye**

Confirm all of:
- `publish-alpha` has `permissions: contents: write` and `needs: [prepare, build]`.
- History tag creation is guarded by `git ls-remote` (idempotent on re-run).
- The update path deletes existing assets *before* uploading (prevents multi-SHA asset accumulation).
- `concurrency.group: alpha-release` with `cancel-in-progress: false`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/alpha.yml
git commit -s -m "ci: add rolling alpha release on merge to main (#67)"
```

---

## Task 5: Final lint, push, and open PR

**Files:** none (verification + PR).

- [ ] **Step 1: Lint all four workflows together**

Run **\[LINT\]**.
Expected: exits 0, or `OK` printed for each of `alpha.yml`, `build-packages.yml`, `package.yml`, `release.yml`.

- [ ] **Step 2: Confirm the four files are present and the diff is workflow-only**

Run: `git diff --stat origin/main -- .github/workflows/`
Expected: shows `alpha.yml` (new), `build-packages.yml` (new), `package.yml` (rewritten), `release.yml` (rewritten) — and nothing outside `.github/workflows/` except the already-committed design/plan docs.

- [ ] **Step 3: Push the branch**

> Note: PGP/SSH signing requires the sandbox disabled (per global rules). The executor runs git commands with signing enabled outside the sandbox and verifies `git cat-file commit HEAD | grep gpgsig` before pushing.

```bash
git push -u origin feat/alpha-release-67
```

- [ ] **Step 4: Open the PR**

```bash
gh pr create --base main --head feat/alpha-release-67 \
  --title "ci: rolling alpha release on merge to main (#67)" \
  --body "$(cat <<'BODY'
Closes #67.

Extracts the package build matrix into a reusable `build-packages.yml`
(`workflow_call`, single `version_label` input). `package.yml`, `release.yml`,
and a new `alpha.yml` all call it — eliminating the duplicated build jobs.

`alpha.yml` runs on each merge to `main`: it pushes an immutable
`alpha-<shortsha>` history tag, force-moves the rolling `alpha` tag, and
refreshes a single `alpha` pre-release in place with fresh `.deb`/`.pkg`/`.msi`
assets.

Design: `docs/design/2026-06-16-rolling-alpha-release-design.md`

### Manual verification after merge
- [ ] Exactly one `alpha` pre-release exists on the Releases page with three assets.
- [ ] An `alpha-<shortsha>` tag appears under Tags.
- [ ] A second merge refreshes the same release (no duplicate pre-release) and adds a new history tag.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
BODY
)"
```

Expected: PR URL printed. The PR's own `package.yml` run (via `pull_request`) exercises the reusable workflow before any alpha publish can fire.
