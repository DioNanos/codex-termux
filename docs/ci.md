# Continuous integration

This fork runs a reduced CI surface on GitHub Actions. What runs, what is
disabled, and why:

## What runs

- **blocking-ci** (pull requests and main): the Linux Bazel test leg, the
  Linux argument-comment lint, codespell, and the fork-owned package checks
  (`repo-checks`, including the V8 sandbox-archive guard for the package
  builder). The Android release build and the npm payload checks run in
  `termux-npm-build-publish` and stay green on main.
- **postmerge-ci** (pushes to main): Linux-only signal via reusable
  workflows, plus codespell.

## What is disabled, and why

Two groups of inherited jobs cannot run on this fork's GitHub account, so
they are disabled at the workflow level instead of showing permanent red:

- **Windows legs** (Bazel test shards, Bazel clippy, verify-release-build,
  and the Windows argument-comment lint): they require the upstream-only
  private runner group `codex-termux-runners`, which does not exist on this
  fork. The jobs fail to schedule at all.
- **macOS legs** (Bazel test/clippy/verify and the macOS argument-comment
  lint): they need paid GitHub runners, and the account spending limit stops
  them from starting.

Both groups are marked with `matrix: exclude:` blocks or `if: false` in
`.github/workflows/bazel.yml` and
`.github/workflows/rust-release-argument-comment-lint.yml`, each with a
comment explaining the reason and how to re-enable them. The Linux and
Android legs are untouched.

## Known postmerge limitation

`postmerge-ci` currently reports `startup_failure` with zero jobs. The three
workflows it calls are active and their YAML parses cleanly, so the likely
cause is the same account-level scheduling limit that blocks the macOS legs.
Rerunning the workflow manually shows whether it clears.
