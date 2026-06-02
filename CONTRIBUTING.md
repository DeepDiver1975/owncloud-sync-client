# Contributing

Thanks for your interest in improving the ownCloud Sync Client! This document
explains how to get set up, the checks your change must pass, and the workflow
for landing it.

By participating in this project you agree to abide by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

This is a [Cargo workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html)
of focused crates. See the **Project layout** table in the
[README](README.md) for what lives where.

We use [`just`](https://github.com/casey/just) as the task runner. The recipes
mirror what CI runs, so you can reproduce the gate locally:

```sh
just build    # or: cargo build --workspace
just test     # run the workspace test suite
just lint     # clippy with warnings denied
just fmt      # format all sources
just ci       # fmt-check + lint + test (the CI gate)
```

Run `just ci` before opening a pull request — it must pass.

For end-to-end acceptance tests (requires Docker and a display server):

```sh
just acceptance-setup   # one-time: install Playwright + Chromium
just acceptance
```

## Making changes

1. **Work on a feature branch** — never push directly to `main`.
2. Keep changes focused; one logical change per pull request.
3. Add or update tests for the behavior you change.
4. Make sure `just ci` passes.

## Commit requirements

- **Sign off every commit (DCO).** Pass `-s` to `git commit`:

  ```sh
  git commit -s -m "your message"
  ```

  This adds a `Signed-off-by` trailer certifying you have the right to submit
  the work under the project's license (see the
  [Developer Certificate of Origin](https://developercertificate.org/)).

- Write clear, descriptive commit messages. We follow the
  [Conventional Commits](https://www.conventionalcommits.org/) style
  (e.g. `feat(gui): …`, `fix: …`, `docs: …`) — see the git history for examples.

## Pull requests

- Open your pull request against `main` from your feature branch.
- Fill in the pull request template: what changed, why, and any linked issues.
- Ensure CI is green. Maintainers review and merge once checks pass.

## Reporting bugs and requesting features

Use the [issue templates](https://github.com/DeepDiver1975/owncloud-sync-client/issues/new/choose)
when opening an issue.

**Do not report security vulnerabilities in public issues.** Please follow our
[Security Policy](.github/SECURITY.md) instead.
