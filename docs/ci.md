# Continuous integration

This fork runs a reduced CI surface on GitHub Actions. What runs, what is
disabled, and why:

## What runs

- **blocking-ci** (pull requests and main): the Linux Bazel clippy and
  verify-release-build legs, the Linux argument-comment lint, codespell,
  and the fork-owned package checks (`repo-checks`, including the V8
  sandbox-archive guard for the package builder). The Android release
  build and the npm payload checks run in `termux-npm-build-publish` and
  stay green on main.
- **postmerge-ci** (pushes to main): calls the `rust-ci-full.yml` and
  `v8-canary.yml` reusable workflows. On this fork only their Linux legs
  run (see "What is disabled, and why"); codespell runs in `blocking-ci`,
  not in postmerge.

## What is disabled, and why

Several inherited legs cannot run on this fork's GitHub account, so they
are disabled instead of showing permanent red. GitHub applies
`matrix: include:` entries after `matrix: exclude:` entries, so an
`exclude:` cannot remove an `include:` entry; the unrunnable matrix legs
are therefore commented out of the `include:` lists (the same pattern the
arm64 Bazel entries already used), while fully inherited jobs are turned
off with `if: false` at the job level. Every disabled block carries a
comment with the reason and how to re-enable it. Linux Bazel clippy,
verify-release-build, and the Android legs stay on.

Per workflow:

- **`bazel.yml`, Bazel test job**: the two Linux x64 legs (gnu and musl)
  are turned off with `if: false`. This fork has no upstream remote cache,
  so the GitHub-hosted runner runs out of disk, and the exec-server and
  git-utils tests need a runtime environment that runner does not provide.
  Remove the `if` to restore them. The two macOS legs (paid GitHub runners;
  the account spending limit stops them from starting) and the two arm64
  Linux legs (flaky in CI, see the note in the file) stay commented out in
  the `matrix: include:` list. `CI required` still needs the Bazel workflow
  to succeed, so Linux clippy and verify-release-build stay strict; that
  gate does not list this test job. `CI results` in `rust-ci.yml` does not
  depend on it.
- **`bazel.yml`, Bazel clippy and verify-release-build jobs**: the Linux
  x64 leg runs. The macOS leg (paid GitHub runners) and the Windows
  gnullvm leg (it requires the upstream-only private runner group
  `codex-termux-runners`, which does not exist on this fork, so the job
  cannot schedule at all) are commented out in the `matrix: include:`
  lists.
- **`rust-release-argument-comment-lint.yml`**: the two Linux legs (x64
  and arm64) build the lint library. The macOS leg (paid GitHub runners)
  and the Windows leg (`codex-termux-runners`) are commented out in the
  `matrix: include:` list.
- **`rust-ci.yml`, PR argument-comment lint**: the Linux leg runs. The
  macOS and Windows legs are commented out in the `matrix: include:` list,
  same reasons as above.
- **`rust-ci-full.yml`** (called by postmerge-ci): the Linux x64 (remote)
  and arm64 test jobs stay strict in the `results` gate. The macOS test
  job (paid runners) and the two Windows test jobs (`codex-termux-runners`)
  are turned off with `if: false`, and the gate accepts `skipped` only for
  those three; the macOS and Windows legs of the prebuilt
  argument-comment lint and of the `lint_build` job are commented out of
  the `matrix: include:` lists — `lint_build` keeps running on its Linux
  legs, so the `results` gate keeps requiring it.
- **`v8-canary.yml`** (called by postmerge-ci): only the Linux legs (x64
  and arm64, release and ptrcomp-sandbox variants) run; the four macOS
  legs (paid GitHub runners) are commented out of the `matrix: include:`
  list. The `build-windows-source` job keeps its upstream conditional
  gate (`windows_source_required`), so it runs only when the metadata job
  asks for it.
- **Three inherited Windows Bazel jobs in `bazel.yml`**: turned off with
  `if: false` at the job level; they show up as skipped checks. The Linux
  Bazel test job above is disabled the same way. Linux clippy and
  verify-release-build are not in this set.

## Known postmerge limitation

`postmerge-ci` currently reports `startup_failure` with zero jobs. The two
workflows it calls are active and their YAML parses cleanly, so the likely
cause is the same account-level scheduling limit that blocks the macOS legs.
Rerunning the workflow manually shows whether it clears.
