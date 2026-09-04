# Termux Patch Inventory

This fork tracks upstream OpenAI Codex and keeps only the compatibility delta
required to publish a working Android Termux package.

- Fork repo: `DioNanos/codex-termux`
- Upstream base for this release: `rust-v0.147.0`
- Current fork release target: `v0.147.0`

## Runtime patches

### Patch #1 - Browser login on Android
- File: `codex-rs/login/src/server.rs`
- Uses `termux-open-url` on Android instead of the desktop browser path.

### Patch #2 - Release profile for Termux builds
- File: `codex-rs/Cargo.toml`
- Keeps the Android release profile explicit for reproducible maintainer builds
  and limits `zip` to the `deflate`/`time` features needed by this tree, avoiding
  unused compression backends in the Android dependency graph.

### Patch #4 - Update source points to fork releases
- Files: `codex-rs/tui/src/updates.rs`, `codex-rs/tui/src/updates_cache.rs`
- Update checks point to `DioNanos/codex-termux` releases instead of
  `openai/codex`; cached results carry their npm/GitHub source so changing
  installation channel cannot reuse a stale result from another source.

### Patch #5 - Version parser accepts Termux tag shapes
- Files: `codex-rs/tui/src/updates.rs`, `codex-rs/tui/src/update_versions.rs`
- Release parser strips both `rust-v` (upstream) and `v` (Termux) tag prefixes,
  and splits on `-` so suffixes like `-termux` collapse to a clean semver
  triple for comparison.

### Patch #6 - Correct package name for self-update
- File: `codex-rs/tui/src/update_action.rs`
- Uses `@mmmbuto/codex-cli-termux@latest` for npm/bun/Unix arms.

### Patch #6b - Fork identity across UI/doctor/npm surfaces
- Files: `codex-rs/cli/src/doctor/updates.rs`,
  `codex-rs/cli/src/doctor.rs`,
  `codex-rs/tui/src/npm_registry.rs`,
  `codex-rs/tui/src/update_prompt.rs`,
  `codex-rs/tui/src/history_cell/notices.rs`,
  `codex-rs/tui/src/version.rs`
- Replaces every upstream-identity reference on user-visible UI/doctor/registry
  surfaces with the fork identity. Covers: `GITHUB_LATEST_RELEASE_URL` →
  `DioNanos/codex-termux/releases/latest`, npm registry URL →
  `@mmmbuto/codex-cli-termux`, doctor labels and path joins →
  `@mmmbuto/codex-cli-termux`, update prompt and notice cells →
  `DioNanos/codex-termux` release URLs. Unit tests pin the displayed CLI version
  to the upstream-neutral `0.0.0`, while production continues to embed the
  workspace package version; this keeps identity snapshots release-independent.

### Patch #10 - Launcher hardening
- Files: `npm-package/bin/codex`, `npm-package/bin/codex-exec`, `npm-package/bin/*.js`
- Packaged launchers preserve `LD_LIBRARY_PATH` and set `CODEX_SELF_EXE` to the
  native `codex.bin`, not the shell wrapper. This keeps bundled
  `libc++_shared.so` reachable while hidden arg0 aliases retain their special
  executable name.

### Patch #10b - Android ELF runpath hardening
- File: `codex-rs/.cargo/config.toml`
- Adds `-Wl,-rpath,$ORIGIN` so packaged Android ELFs can resolve sibling
  `libc++_shared.so` even without wrapper-provided `LD_LIBRARY_PATH`.

### Patch #11 - Android realtime audio (RETIRED at rust-v0.140.0-alpha.18)
- **Retired**: upstream removed the entire TUI realtime voice feature in
  [openai/codex#27801](https://github.com/openai/codex/pull/27801) (deleted
  `chatwidget/realtime.rs`, the WebRTC/audio events, and the `/realtime`
  `/settings` audio commands). This fork's contribution was only a 4-line cfg
  toggle plus an Android `cpal`/`oboe-shared-stdcxx` link fix on top of that
  upstream feature — with the feature gone, the toggle had nothing left to gate
  and was dropped with it (the `cpal`/`oboe` Android dependency is no longer
  pulled in). The path was never usable from a plain Termux CLI process anyway
  (the audio backend needs an Android `JavaVM`/`Activity`). Termux-native audio
  is tracked on the Codex VL roadmap. `verify-patches.sh` no longer checks #11.

### Patch #12 - Dynamic npm wrapper routing
- File: `npm-package/bin/codex.js`
- Detects root subcommands from `codex --help` and avoids misrouting valid
  commands to `codex exec`. Empty invocations and option-first calls bypass
  discovery, avoiding a redundant native-process startup for interactive use,
  `--help`, and `--version`.

### Patch #13 - Fork-safe managed updates
- Files: `codex-rs/tui/src/update_action.rs`, `codex-rs/app-server-daemon/*`
- Keeps update commands on `@mmmbuto/codex-cli-termux@latest`, disables daemon
  auto-update fetches, neutralises `install_latest_standalone()` to `Ok(())`,
  and blocks the upstream installer URL from reappearing in the daemon's user
  guidance message.

### Patch #14 - Fork-owned public install surfaces
- Files: `scripts/install/*`, `scripts/stage_npm_packages.py`, `codex-rs/README.md`
- Keeps inherited installer and release-staging code contained to
  `DioNanos/codex-termux`, while the supported Termux install guidance uses
  `@mmmbuto/codex-cli-termux`. The GitHub release does not claim to publish the
  upstream desktop standalone-asset matrix.

### Patch #15 - Fork-owned feedback surfaces
- Files: `.github/ISSUE_TEMPLATE/*`, `.github/pull_request_template.md`,
  `announcement_tip.toml`, `codex-rs/tui/src/bottom_pane/feedback_view.rs`,
  `codex-rs/tui/src/tooltips.rs`
- Keeps public feedback, issue, contribution, and announcement-tip links on
  `DioNanos/codex-termux`.

### Patch #16 - Android remote-control daemon support
- Files: `codex-rs/app-server-daemon/src/managed_install.rs`, `codex-rs/app-server-daemon/src/backend/pid.rs`, `codex-rs/cli/src/remote_control_cmd.rs`
- Enables `codex remote-control` daemon mode (`start`/`stop`) on Android/Termux.
  Three sub-fixes, all gated on `#[cfg(target_os = "android")]`:
  1. **`managed_codex_bin`** (`managed_install.rs`): on Android, resolves the daemon
     ELF via `CODEX_SELF_EXE` (set by the npm launcher, Patch #10) instead of
     the standalone installer path `~/.codex/packages/standalone/current/codex`
     which does not exist on npm-based Termux installs. The ELF resolves
     `libc++_shared.so` via `RUNPATH=$ORIGIN` (Patch #10b).
  2. **`read_process_start_time`** (`pid.rs`): on Android, reads process start time
     from `/proc/<pid>/stat` field 22 (starttime in jiffies since boot) instead of
     `ps -o lstart=`, which is not available in Android toybox.
  3. **Foreground socket dir** (`remote_control_cmd.rs`): uses `std::env::temp_dir()`
     (honours `$TMPDIR`) instead of hardcoding `/tmp`, which does not exist on
     stock Android. Applied unconditionally; correct on all Unix platforms.

### Patch #17 - flock ENOTSUP/EOPNOTSUPP tolerance for Termux storage
- Files: `codex-rs/app-server-daemon/src/backend/pid.rs`, `codex-rs/app-server-daemon/src/lib.rs`
- Some Android/Termux storage backends rooted at `/data/data/com.termux/...`
  reject `flock(2)` with `ENOTSUP` / `EOPNOTSUPP` instead of acquiring or
  refusing the lock. Both `try_lock_file` helpers — the daemon operation lock
  and the pid reservation lock — match the same permissive degradation already
  used elsewhere (see Patch #18 on `installation_id.rs`) and treat the
  unsupported class as "lock acquired" so `codex remote-control` proceeds.
  Linux ext4/btrfs/xfs continue to enforce `flock` unchanged; Windows is
  unaffected (`#[cfg(not(unix))]` paths untouched).

### Patch #18 - Android runtime compatibility shims
- Files: `codex-rs/arg0/src/lib.rs`, `codex-rs/core/src/installation_id.rs`, `codex-rs/utils/pty/src/pty.rs`
- Three runtime shims required by the Android package; compile-time gates are
  used only where the platform API differs, while file-lock support is detected
  from the runtime error:
  1. **`arg0/src/lib.rs`**: `CODEX_SELF_EXE` resolution so subprocess re-exec
     flows pick up the npm-launcher-provided native ELF path rather than the
     shell wrapper (paired with Patch #10), plus permissive degradation when
     `try_lock` on the codex aliases lock file returns
     `ErrorKind::Unsupported`.
  2. **`core/src/installation_id.rs`**: tolerates `ErrorKind::Unsupported` on
     the installation-id lockfile so first-run on Termux storage does not
     abort installation-id bootstrap. This is the original source of the
     `is_unsupported_file_lock_error` pattern reused by Patches #17 and the
     `arg0` shim above.
  3. **`utils/pty/src/pty.rs`**: provides an `openpty` C symbol on Android,
     since Bionic does not export it. The fork implementation uses
     `posix_openpt` + `grantpt` + `unlockpt` + `ptsname_r` + `open` to
     produce master/slave fds compatible with the upstream pty handling.

### Patch #18a - Complete cross-fork ENOTSUP lock coverage
- Files: `codex-rs/app-server-transport/src/transport/unix_socket.rs`,
  `codex-rs/core/src/installation_id.rs`,
  `codex-rs/message-history/src/lib.rs`,
  `codex-rs/message-history/src/batch.rs`, `codex-rs/arg0/src/lib.rs`,
  `codex-rs/execpolicy/src/amend.rs`
- Preserves all eight advisory-lock callsite guards introduced by the shared
  Termux compatibility fix: one app-server startup lock, one installation-id
  lock, three message-history locks, two arg0 directory locks, and one
  execpolicy lock. Each callsite degrades only for `ErrorKind::Unsupported`;
  other lock errors remain fatal. The two daemon `flock(2)` guards are tracked
  separately by Patch #17.

### Patch #18b - Termux MCP subprocess environment
- File: `codex-rs/rmcp-client/src/utils.rs`
- When `TERMUX_VERSION` is present, propagates the exact 15-variable Termux
  allowlist needed by stdio MCP servers launched through `npx`, `npm`, `pip`,
  and native helpers: `PREFIX`; `TERMUX_VERSION`, `TERMUX_APP_PID`, and
  `TERMUX_MAIN_PACKAGE_FORMAT`; `LD_PRELOAD`, `LD_LIBRARY_PATH`, and
  `NPM_CONFIG_PREFIX`; the four Android/bootstrap roots; and the four XDG
  paths. Outside Termux, this extra allowlist is empty, so unrelated desktop
  variables such as `PREFIX=/usr/local` do not leak into child processes.

### Patch #19 - Android UI cfg gates
- Files: `codex-rs/tui/src/clipboard_paste.rs`
- Adds `#[cfg(not(target_os = "android"))]` around clipboard paste paths that
  depend on platform clipboard primitives unavailable on Termux, so the
  no-clipboard build configuration links cleanly. (The former `app_event.rs`
  android cfg gate belonged to the realtime audio event retired with Patch #11
  / openai/codex#27801.)

### Patch #20 - Android code-mode (real, upstream-aligned)
- Files: `codex-rs/code-mode-runtime/Cargo.toml`,
  `codex-rs/code-mode-host/Cargo.toml`, `codex-rs/code-mode/Cargo.toml`
- The Android code-mode stub was reverted in `0.136.0`. `code-mode` uses the
  real upstream runtime with the in-process V8 engine on Android too: the
  fork-owned `runtime_stub.rs`/`service_stub.rs` were removed and V8 is pulled
  in ungated. This relies on the fork-owned `aarch64-linux-android` `rusty_v8`
  prebuild (see Patch #22), so `exec`/`wait` code-mode is no longer a no-op on
  the published Termux package.
- **Anchors moved at `rust-v0.147.0`**: upstream extracted the V8 runtime from
  `code-mode` into the new `code-mode-runtime` crate, so `v8 = { workspace =
  true }` no longer appears in `code-mode/Cargo.toml`. Nothing about the
  guarantee changed — `code-mode-runtime` carries V8 with no `target_os` gate
  and `code-mode-host` depends on it unconditionally — but the
  `verify-patches.sh` check had to follow the code. A check that fails because
  the code moved is a check to repoint, not to delete.

### Patch #21 - Android cross-build vendored OpenSSL
- File: `codex-rs/core/Cargo.toml`
- Adds a `[target.aarch64-linux-android.dependencies]` block that pulls
  `openssl-sys` with the `vendored` feature, so Android cross-builds compile
  OpenSSL from source instead of looking for system libssl. Non-Android
  targets are unaffected.

### Patch #22 - V8 Android prebuilt infrastructure
- Files: `.github/workflows/rusty-v8-android-release.yml`,
  `scripts/fetch_rusty_v8_android.py`,
  `scripts/prepare_rusty_v8_android_source.py`,
  `third_party/v8/android-artifacts.toml`
- `rusty_v8` does not provide official Android arm64 binary releases. The fork
  ships its own prebuilt path: `fetch_rusty_v8_android.py` reads the
  required `rusty_v8` version from `Cargo.lock`, looks it up in
  `android-artifacts.toml`, downloads the matching prebuilt static library
  and binding from a fork-owned GitHub release, requires both pinned SHA-256
  values and verifies them before exporting
  `RUSTY_V8_ARCHIVE` + `RUSTY_V8_SRC_BINDING_PATH` so Cargo skips compiling
  V8 from source. `prepare_rusty_v8_android_source.py` is the maintainer-side
  companion used to produce a new prebuilt when upstream bumps the V8 pin. The
  GitHub workflow is read-only and uploads build artifacts only; publication of
  an audited prebuilt release remains coordinator-owned.

### Patch #23 - Fork-owned workflows and CI guards
- Files: `.github/workflows/repo-checks.yml`,
  `.github/workflows/rusty-v8-android-release.yml`,
  `.github/workflows/termux-npm-build-publish.yml`,
  `.github/workflows/termux-gate.yml`,
  `.forgejo/workflows/termux-next-smoke.yml`
- Upstream `repo-checks.yml` stages an npm package and uploads it as an artifact;
  these steps are gated with
  `if: ${{ github.repository == 'openai/codex' }}`
  so a fork clone of CI never publishes upstream-flavoured artifacts.
  `termux-npm-build-publish.yml` is the fork's release pipeline:
  workflow_dispatch, builds the Android arm64 binaries with the V8 prebuilt
  flow (Patch #22), binds checkout HEAD to the workflow run `GITHUB_SHA`,
  assembles the npm package, and uploads the immutable tarball. The workflow
  uses read-only repository permissions and does not publish npm packages or
  create GitHub releases; both mutations are intentionally local from the
  maintainer host after the downloaded artifact passes audit.
  `.forgejo/workflows/termux-next-smoke.yml` is the Forge mirror used for
  develop-side smoke tests. Public candidates omit both `.forgejo/**` and the
  internal `test-report/**` device logs; `verify-patches.sh` enforces the full
  versus sanitized-tree contract explicitly. `termux-gate.yml` is the
  fork-owned workflow-dispatch gate for formatting, Cargo lock cleanliness
  under shear and deny, and targeted Termux regression tests.

### Patch #24 - Termux TLS roots (RETIRED at rust-v0.147.0)
- **What it did**: reqwest 0.13 (pulled in by the rmcp 1.7.0 upgrade) routes TLS
  verification through `rustls-platform-verifier`, which on
  `target_os = "android"` requires an initialized JVM Context and panics with
  `Expect rustls-platform-verifier to be initialized` in a plain Termux CLI
  process at the first TLS handshake (issue #11). `apply_termux_tls()` supplied
  the embedded Mozilla roots (`webpki-root-certs`) via
  `ClientBuilder::tls_certs_only()`, runtime-gated on `TERMUX_VERSION`, so
  reqwest built its `WebPkiServerVerifier` and never constructed the platform
  verifier.
- **Retired**: upstream moved these paths onto the injected reqwest 0.12
  adapters one at a time, and rust-v0.147.0 removed the last reqwest 0.13
  client this crate built for itself — `discover_streamable_http_oauth_with_headers`
  was folded into `discover_streamable_http_oauth_with_headers_and_http_client`,
  which takes an `Arc<dyn HttpClient>` from `codex-http-client`. With the client
  gone, `reqwest` is no longer a dependency of `codex-rmcp-client` at all: the
  guard did not merely become redundant, it stopped compiling. It was removed
  along with `webpki-root-certs` and its two tests.
- **What is not settled**: this only establishes that the *panic* is unreachable
  on this path. `codex-http-client` builds with `rustls-tls-native-roots`, and
  whether the native trust store is populated under Termux has not been measured
  on a device — a failure there would surface as a TLS connection error, not a
  panic. That question belongs to `codex-http-client`, not to this patch.
- `verify-patches.sh` still checks #24, but inverted: it now asserts that
  `codex-rs/rmcp-client/Cargo.toml` has no direct `reqwest` dependency. Should
  one reappear, an Android-panicking client is back and the guard must be
  restored with it.

### Patch #25 - Plugin hook-file description compatibility
- Files: `codex-rs/config/src/hook_config.rs`,
  `codex-rs/config/src/hooks_tests.rs`
- Accepts the optional top-level `description` emitted by current plugin hook
  files while retaining `deny_unknown_fields` for every other unexpected key.

### Patch #26 - Model-catalog instruction fallback
- Files: `codex-rs/protocol/src/openai_models.rs`,
  `codex-rs/models-manager/src/model_info.rs`,
  `codex-rs/models-manager/src/model_info_tests.rs`
- Deserializes catalogs that omit `base_instructions` and supplies the built-in
  instructions only when neither a non-empty base nor an instruction template
  is available. Explicit empty configuration overrides remain explicit.

## Verification

Run from repo root:

```bash
bash verify-patches.sh
```

### Patch #27 - Pairing tells the user how to start the daemon
- Files: `codex-rs/app-server-daemon/src/lib.rs`
- `remote-control pair` attaches to a daemon that is already running and never
  starts one. With nothing listening the user only saw a transport error naming
  a socket path, which does not say what to do next. The guard refuses early and
  names both the missing socket and the command that fixes it.
- Reported as issue #15. Upstream does not treat this as a defect, but it is a
  Termux user's first contact with remote control, so it belongs in the fork.
- Covered by `pairing_without_a_running_daemon_says_how_to_start_one`, which
  exercises the behaviour rather than reading the source.

### Patch #28 - musl ripgrep in the musl aarch64 payload

- File: `scripts/codex_package/rg`
- The DotSlash manifest's `linux-aarch64` entry pointed at
  `ripgrep-…-aarch64-unknown-linux-gnu`, so the Linux arm64 package — whose target is
  `aarch64-unknown-linux-musl`, and whose `codex` and `bwrap` are statically linked —
  bundled a `rg` that requires `libc.so.6` and `ld-linux-aarch64.so.1`. On a musl-only
  system it does not start and file search silently degrades. The `linux-x86_64` entry
  already used the musl artifact; only aarch64 was inconsistent, and upstream does not
  publish an aarch64-musl package of its own, so the mismatch never surfaced there.
- The entry now points at the musl artifact, whose size and digest were taken from the
  downloaded file rather than copied.
- **Why this is guarded**: a future merge that accepts upstream's manifest — by
  auto-merge or by resolving the file wholesale — restores the glibc artifact with **no
  compilation signal at all**. The package builds; `rg` breaks at runtime on the device.
  The check is deliberately version-agnostic: pinning it to a ripgrep version would make
  it go stale at the next bump and stop guarding anything.
