# Translation Tooling Design

**Date:** 2026-05-16
**Branch:** feat/translation

## Goal

Add tooling to the repo that:

1. Detects translation keys used in source but missing from any locale YAML file
2. Detects keys present in locale YAML files but unused in source
3. Detects hardcoded visible strings in the GUI that should be wrapped in `t!()`
4. Scaffolds missing keys as empty stubs for developer workflow
5. Runs as a CI gate (hard fail on any violation)
6. Provides a project-level Claude Code skill for AI-assisted translation of new strings

---

## Architecture

### New crate: `crates/xtask`

A standard Rust xtask binary, added to the workspace. Two subcommands:

```
cargo run -p xtask -- check-keys   # CI gate
cargo run -p xtask -- sync-keys    # developer workflow
```

---

## Subcommand: `check-keys`

Read-only. Exits non-zero if any violation is found. Used in CI.

### Step 1 — Extract `t!()` keys from source

Regex-scan all `*.rs` files under `crates/gui/src/`.

Pattern: `t!\("([^"]+)"` — captures the first string argument.

Named interpolation args like `t!("folder_status_error_other", count = n)` are handled correctly by this pattern (the key is the first quoted string).

### Step 2 — Detect hardcoded visible strings

Same scan. Match calls of the form `text("...")` where the argument string:

- Contains at least one ASCII alphabetic character
- Is not a URL (does not start with `http://` or `https://`)
- Is not a single character or pure symbol

Skip lines that end with `// i18n-ignore`. This is the escape hatch for deliberately untranslated content (product names, copyright/legal text, URLs rendered as links).

### Step 3 — Parse locale files

Parse all 4 files in `crates/gui/locales/`: `en.yml`, `de.yml`, `fr.yml`, `zh.yml`.

YAML structure is flat under a locale prefix:

```yaml
en:
  key_name: "value"
```

Parsed as `BTreeMap<String, BTreeMap<String, String>>`.

### Step 4 — Violations

Fail (exit non-zero, print report) if any of:

- A `t!()` key is missing from **any** locale file
- A key exists in **any** locale file but is not referenced by any `t!()` call
- A hardcoded visible string is found that is not suppressed by `// i18n-ignore`

All violations are collected and printed before exiting, so the developer sees the full picture in one run.

---

## Subcommand: `sync-keys`

Write-back. For developer use only — never called in CI.

1. Runs the same scan as `check-keys`
2. For each key used in source but absent from a locale file: appends `key: ""` as an empty stub at the end of the locale block, grouped under a `# new` comment
3. Preserves all existing file content and comments; never reorders existing keys
4. Prints a summary: which keys were added to which files

This is the workflow for adding new UI strings: write the `t!("new_key")` call, run `just sync-translations`, fill in translations (manually or via `/translate-strings` skill), then `just check-translations` passes in CI.

---

## Just targets

```just
# Check for missing, unused, or hardcoded translation strings
check-translations:
    cargo run -p xtask -- check-keys

# Scaffold missing keys as empty stubs in all locale files
sync-translations:
    cargo run -p xtask -- sync-keys
```

`check-translations` is added to the `ci` recipe:

```just
ci: fmt-check lint test check-translations
```

---

## CI integration

`check-translations` is added as a step to the `test-linux` job in `.github/workflows/ci.yml`:

```yaml
- name: Check translations
  run: just check-translations
```

---

## Claude Code skill: `/translate-strings`

File: `.claude/skills/translate-strings.md`

When invoked, the skill instructs Claude to:

1. Run `just check-translations` to identify empty stubs and missing keys
2. Parse all locale files; find keys where any locale has an empty value `""`
3. Use `en.yml` as the source of truth for meaning; use the `# section` comments for context
4. Translate each empty value into the target language, maintaining tone and style consistent with existing translations in that file
5. Write the completed translations back to the locale files
6. Run `just check-translations` again to verify no violations remain
7. Report what was translated

The skill does **not** hit an external API directly — it uses Claude's built-in knowledge via the Read/Edit tools.

---

## Implementation notes

- YAML library: `serde_yaml` (already widely used in the Rust ecosystem; add to workspace dependencies)
- Key extraction regex: `t!\("([^"]+)"` — simple, no parser needed for this structure
- Hardcoded string regex: `text\("([^"]+)"\)` with alphabetic filter and `// i18n-ignore` line skip
- `BTreeMap` for keys ensures stable, sorted output when writing back stubs
- The xtask crate is not part of the published library surface; it only needs `dev` dependencies

---

## File layout

```
crates/
  xtask/
    Cargo.toml
    src/
      main.rs          # CLI entry, subcommand dispatch
      check.rs         # check-keys logic
      sync.rs          # sync-keys logic
      locale.rs        # YAML parse/write helpers
      source_scan.rs   # t!() key extraction + hardcoded string detection
.claude/
  skills/
    translate-strings.md
```
