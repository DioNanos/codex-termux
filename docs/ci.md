# Continuous integration

This fork runs a reduced CI surface on GitHub Actions. What runs, what is
disabled, and why:

## What runs

- **blocking-ci** (pull requests and main): codespell, the fork-owned
  package checks (`repo-checks`, including the V8 sandbox-archive guard
  for the package builder), and the cargo gates (format, cargo shear,
  cargo-deny, sdk, blob size). Bazel does not run. The Android release
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
comment with the reason and how to re-enable it. The cargo gates stay
on. The Android legs stay on.

Per workflow:

- **`bazel.yml`, test, clippy, and verify-release-build**: turned off
  with `if: false` on every platform. Bazel is upstream's build system
  and uses a remote cache with credentials this fork does not have. This
  fork publishes with cargo and the Termux gate. Remove the `if` to
  restore a job. `CI required` does not list the Bazel workflow, so a
  skipped Bazel job cannot fail that gate. Cargo jobs stay strict.
- **`rust-release-argument-comment-lint.yml`**: this workflow builds the
  lint library with cargo, not Bazel, so it stays. The two Linux legs
  (x64 and arm64) run. The macOS leg (paid GitHub runners) and the
  Windows leg (`codex-termux-runners`) are commented out in the
  `matrix: include:` list.
- **`rust-ci.yml`, Bazel argument-comment lint**: the prebuilt job is
  turned off with `if: false`, for the same Bazel reason as above.
  Remove the `if` to restore it. `CI results` does not require that job.
  The cargo package job and the cargo general and shear checks stay
  strict. The macOS and Windows legs stay commented out.
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
  `if: false` at the job level; they show up as skipped checks, together
  with the test, clippy, and verify-release-build jobs above.

## Known postmerge limitation

`postmerge-ci` currently reports `startup_failure` with zero jobs. The two
workflows it calls are active and their YAML parses cleanly, so the likely
cause is the same account-level scheduling limit that blocks the macOS legs.
Rerunning the workflow manually shows whether it clears.
