# Rolling Alpha Release — Design

**Issue:** [#67](https://github.com/DeepDiver1975/owncloud-sync-client/issues/67) — *release: provide an alpha release which is updated upon each merge to main*
**Date:** 2026-06-16
**Status:** Approved

## Problem

Users want to test the software without building it themselves. The project must
publish downloadable packages (`.deb`, `.pkg`, `.msi`) to a GitHub Release that is
**constantly updated on each merge to `main`** — a rolling "alpha" channel.

### Current state

- `package.yml` — builds all three packages on every push to `main` and on PRs, but
  only uploads them as ephemeral CI artifacts (30-day retention). Downloading them
  requires a GitHub login; there is no public Release.
- `release.yml` — triggers only on `v*` tag pushes; builds the same packages and
  publishes a proper GitHub Release with generated notes.

The gap: **nothing publishes a downloadable release on merge to `main`.** The build
matrix is also duplicated between `package.yml` and `release.yml`.

## Goals

1. On each merge to `main`, publish/refresh a single rolling **`alpha`** pre-release
   carrying the three platform packages at stable download URLs.
2. Keep an immutable per-build git tag so historical builds remain traceable.
3. Eliminate build-matrix duplication by extracting a reusable build workflow that
   `package.yml`, `release.yml`, and the new `alpha.yml` all call.

## Non-goals (YAGNI)

- Code signing / notarization (existing `# TODO: sign` markers stay).
- Rewriting internal version metadata in `Cargo.toml` / `Info.plist` / pkgbuild / WiX.
- Changelog automation, auto-update / Sparkle feeds.
- Retention / pruning of old history tags.

## Architecture

Four workflow files; build logic lives in one reusable workflow.

```
.github/workflows/
  build-packages.yml   NEW   reusable (workflow_call): builds .deb/.pkg/.msi, uploads 3 artifacts
  package.yml          EDIT  calls build-packages.yml (PR + push validation, no publish)
  release.yml          EDIT  calls build-packages.yml, then publishes the tagged release
  alpha.yml            NEW   on push to main: calls build-packages.yml, then publishes alpha
```

### Reusable build workflow — `build-packages.yml`

- Trigger: `on: workflow_call`.
- Input: **`version_label`** (string, required) — embedded into asset filenames and
  surfaced in release metadata. Does **not** rewrite internal version fields.
- Contains the three jobs currently duplicated across `package.yml` / `release.yml`:
  `package-linux`, `package-macos`, `package-windows`.
- Each job builds its package, names the artifact using `version_label`
  (e.g. `owncloud-sync_0.1.0-alpha.a1b2c3d_amd64.deb`), and uploads a named artifact
  (`linux-deb`, `macos-pkg`, `windows-msi`).
- macOS Swift `xcodebuild` steps keep their `|| true` (unsigned CI builds), preserving
  current behavior.

### Callers

| Workflow      | Trigger                       | `version_label` passed            | Publishes |
|---------------|-------------------------------|-----------------------------------|-----------|
| `package.yml` | `pull_request`, push to main  | short `${{ github.sha }}`         | no (validation only) |
| `alpha.yml`   | push to `main`, `workflow_dispatch` | `0.1.0-alpha.<shortsha>`    | rolling `alpha` release |
| `release.yml` | push tag `v*`                 | `${{ github.ref_name }}` (tag)    | tagged release |

## Alpha publish behavior (`alpha.yml`)

- Triggers: `push: branches: [main]` + `workflow_dispatch`.
- `permissions: contents: write` (push tags, edit releases).
- `concurrency: { group: alpha-release, cancel-in-progress: false }` — two quick
  merges queue rather than race on the force-pushed tag / clobbered assets.

**Jobs:**

1. **`build`** — `uses: ./.github/workflows/build-packages.yml` with
   `version_label: 0.1.0-alpha.<shortsha>`.
2. **`publish-alpha`** (`needs: build`) — downloads the three artifacts and:

   **(a) History tag** — create + push an immutable lightweight tag `alpha-<shortsha>`
   at the merged commit. Guarded so a re-run on the same commit skips without failing.
   These tags get **no** release of their own — they exist purely as traceable git refs.

   **(b) Rolling `alpha` release** — the moving pointer:
   - Force-update the git tag `alpha` to the current commit
     (`git tag -f alpha && git push -f origin alpha`).
   - Update the release in place: if a release `alpha` exists, delete its stale assets
     and upload the new ones (`gh release upload --clobber`); otherwise create it.
   - Marked `--prerelease`, title `ownCloud Sync (alpha)`, body noting it is an
     auto-updated bleeding-edge build pointing at commit `<shortsha>`, dated via a
     passed-in timestamp.
   - The release stays attached to the moving `alpha` tag so download URLs
     (`/releases/download/alpha/...`) are stable. Result: one visible prerelease on the
     Releases page, full build history under Tags.

## Error handling & edge cases

- **Idempotent history tag** — re-running on the same commit must not fail on an
  existing `alpha-<shortsha>` tag.
- **Asset clobber** — `--clobber` overwrites by filename; when `version_label` changes
  (new SHA), stale assets from the prior alpha are explicitly deleted first so the
  release never accumulates multiple SHAs' files.
- **First run** — publish job creates the `alpha` release if absent, updates if present;
  same path covers the very first merge.
- **Quick successive merges** — serialized by the `concurrency` group; the later commit
  wins the rolling pointer, both get history tags.
- **`workflow_dispatch` on an old commit** — produces correct artifacts for that commit
  and would move `alpha` backward; acceptable as a manual override.
- **macOS/Windows partial builds** — `|| true` on unsigned Swift steps preserved.

## Testing strategy

CI workflows can't be unit-tested; verification is staged:

- `actionlint` syntax pass on all four files (run locally if available).
- The PR introducing this exercises the reusable workflow via `package.yml`'s
  `pull_request` trigger — confirm that run stays green before any merge-to-main alpha
  publish fires.
- Post-merge manual checks (listed in the PR body):
  - exactly one `alpha` prerelease exists with three assets at stable URLs;
  - an `alpha-<shortsha>` tag appears under Tags;
  - a second merge refreshes the same release (no duplicate prerelease) and adds a new
    history tag.
