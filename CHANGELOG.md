# [0.148.1] - 2026-08-20

Fork-only patch on the upstream OpenAI Codex `rust-v0.148.0` base. The native
payload and the existing Android V8 build are unchanged.

## Fixed

- Android/Termux npm launchers no longer depend on `/usr/bin/env`, which is not
  present in the Termux userland. The Android-only postinstall rewrites the
  installed launcher shebangs to the active Node and shell interpreters.
- Both npm entrypoints now spawn `codex.bin` directly while preserving the
  managed-package markers and `LD_LIBRARY_PATH` sanitization.

## Verification

- Cargo and npm versions are synchronized at `0.148.1`.
- The package workflow runs targeted `node --check` and `node --test` launcher
  checks.
- Static release-contract checks passed; no build or device validation is
  claimed by this candidate note.

# [0.148.0] - 2026-08-20

Candidate Termux release synchronized to final upstream OpenAI Codex
`rust-v0.148.0`. Publication remains gated on the sanitized artifact audit and
device validation.

## Changed

- Integrated the final upstream `rust-v0.148.0` release: Markdown conversation
  export with `/export`, session forking via `codex exec fork` with archive and
  restore from the resume picker, prompt drafting during TUI startup, estimated
  thread credits or cost for eligible workspaces, Amazon Bedrock Runtime as a
  built-in provider, and asynchronous hook commands with MCP tool invocation.
  Upstream fixes include resumed sessions restoring their persisted working
  directory and approval policy, turn reconnection through provider outages,
  MCP recovery after OAuth reauthentication, fail-closed sandbox paths, and
  composer handling of CRLF pastes and long URLs.
- Preserved the verified Android/Termux compatibility and release-profile
  contracts, including the fork-owned updater and installer channels and the
  sandboxed Android V8 code-mode payload.
- Bumped the Cargo workspace and npm package versions to `0.148.0`.

# [0.147.3] - 2026-08-10

Fork-only patch release on top of the upstream OpenAI Codex `rust-v0.147.0`
stable release. Upstream has no `rust-v0.147.3`.

## Fixed

- **Code mode ran without the V8 sandbox.** `code-mode-runtime` depends on the
  `v8` crate with `v8_enable_sandbox`, and the v8 build script derives the
  prebuilt's name from the enabled features. Setting `RUSTY_V8_ARCHIVE` overrides
  that name without checking it, and the Android build pointed it at the plain
  prebuilt. Everything compiled, linked and ran, code mode worked, and
  `v8::V8::is_sandbox_enabled()` was `false` — with no error, no warning and
  nothing at runtime that said so. Every 0.147.0, 0.147.1 and 0.147.2 install is
  affected.

  The Android package now links a prebuilt built with the sandbox feature,
  pinned by checksum, and the build verifies the archive it actually linked by
  decoding what `v8__V8__IsSandboxEnabled` returns rather than trusting the file
  name.

## Added

- **An Android V8 prebuilt with the sandbox.** No such archive existed anywhere,
  upstream or otherwise, so it is now built from source for
  `aarch64-linux-android` in CI and published with its checksums, alongside the
  plain one.

# [0.147.2] - 2026-08-08

Fork-only patch release on top of the upstream OpenAI Codex `rust-v0.147.0`
stable release. Upstream has no `rust-v0.147.2`.

## Fixed

- **Code mode did not work.** Since `rust-v0.147.0` upstream runs code mode out
  of process: the CLI spawns `codex-code-mode-host` next to its own binary and
  fails closed when it is absent, reporting `Code Mode is unavailable ... host
  executable was not found`. That binary was never built or packaged, so code
  mode was dead on every install of 0.147.0 and 0.147.1. It now ships with the
  package, and the build reads the finished tarball back to prove it is there.
# [0.147.1] - 2026-08-08

Fork-only patch release on top of the upstream OpenAI Codex `rust-v0.147.0`
stable release. No upstream changes are included: upstream has no `rust-v0.147.1`.

## Fixed

- **Codex could not start on Termux.** Storage under
  `/data/data/com.termux/files` does not implement advisory file locks, and the
  thread writer lock added upstream in `rust-v0.147.0` treated a missing lock as
  a fatal error. Starting a session failed with
  `thread-store internal error: failed to acquire thread writer coordination
  lock ...: lock() not supported`. A lock the filesystem cannot provide now
  degrades to running without it, matching how the rest of this fork already
  handles advisory locks.

  Where the lock is unavailable, single-writer ownership of a thread is no
  longer enforced and a second writer on the same thread is not detected. Every
  other lock failure stays fatal.

`0.147.0` remains available on the `next` channel; it does not start on Termux.
# [0.147.0] - 2026-08-07

Termux release synchronized to the upstream OpenAI Codex `rust-v0.147.0` stable
release, published on the `latest` channel.

## Changed

- Integrated upstream `rust-v0.147.0`. Upstream tagged it a day after
  `0.147.0-alpha.13`; the two are siblings off the same parent and differ only
  in the workspace version, so the fork tracks the stable directly.
- Moved the Android V8 prebuild to `150.4.0`, matching the `v8 = "=150.4.0"` the
  upstream workspace now requires.
- Bumped the Cargo workspace and npm package versions to `0.147.0`.

## Fixed

- An `AGENTS.md` created during a session — which is what `/init` does — is now
  discovered. Discovery cached its result under the environment selection, which
  does not change when a file appears, so the session kept reporting that none
  existed (issue #14).
- The model catalog parses again. Upstream added its own legacy
  `base_instructions` key whose serializer also flattens `ModelInfo`, so with the
  fork field serialized too the catalog carried the key twice. The field is now
  outside serde.
- Models whose catalog entry ships no instruction template get the fork's
  fallback instructions again. The merge had replaced that branch with upstream's,
  which returns an empty string; a behavioural test now fails if it is emptied
  again.
- Pairing reports how to start the daemon when none is listening.

## Removed

- The Termux TLS root patch (#24). Upstream removed the last `reqwest` client
  this crate built for itself, and the dependency went with it, so the guard no
  longer compiled. MCP OAuth discovery now runs on the injected
  `codex-http-client`, which never constructs the platform verifier that panics
  on Android. Whether that path's native root store works under Termux has not
  been measured on a device.

# [0.146.1] - 2026-08-07

Termux release synchronized to upstream OpenAI Codex `rust-v0.146.1`. Published
on the `next` channel; `latest` moved to `0.147.0`. (This entry originally read
as a gated candidate; it is amended here to record what was actually shipped.)

## Changed

- Integrated the upstream `rust-v0.146.1` patch release (backported safer
  cyber-model auto-review defaults). No fork-owned path was touched by the
  merge.
- Bumped the Cargo workspace and npm package versions to `0.146.1`.

## Fixed

- The Android V8 prebuild script now ships the `v8_String_WriteFlags_*`
  compatibility aliases in the published binding, not only in the checkout used
  to build it. Consumers fetch that file verbatim, so a prebuild produced from
  a clean run previously failed the Android build on undeclared symbols.

# [0.146.0] - 2026-07-29

Candidate Termux release synchronized to final upstream OpenAI Codex
`rust-v0.146.0`. Publication remains gated on the sanitized artifact audit and
device validation.

## Changed

- Integrated the final upstream `rust-v0.146.0` release.
- Preserved the verified Android/Termux compatibility and release-profile
  contracts, including the fork-owned updater and installer channels.
- Bumped the Cargo workspace and npm package versions to `0.146.0`.

# [0.145.0] - 2026-07-23

Synced the complete Termux fork directly to the final upstream OpenAI Codex
`rust-v0.145.0` release, without carrying the parallel alpha-only update and
installer surfaces. The Android/Termux compatibility delta remains applied.

## Changed
- Integrated the complete upstream `rust-v0.145.0` stable release.
- Preserved every verified Termux patch, including Android TLS roots, PTY and
  lock compatibility, real in-process V8 code-mode, bundled libc++, and
  `RUNPATH=$ORIGIN` packaging hardening.
- Kept all installer, update, feedback, and release surfaces on the
  `DioNanos/codex-termux` and `@mmmbuto/codex-cli-termux` channels.
- Updated stale update-available snapshots so they assert the fork-owned
  install commands and release URLs.
- Kept hidden `apply_patch` and re-exec aliases bound to the native ELF by
  setting `CODEX_SELF_EXE` to `codex.bin`, not the package shell wrapper.
- Extended safe `ENOTSUP` degradation to the bounded message-history batch
  reader added upstream, while credential and certificate locks remain
  fail-closed.
- Added the complete Apache `LICENSE` and project `NOTICE` to the published npm
  payload.
- Aligned the documented minimum Android version with the API 29 NDK target
  used by the release workflow.
- Bumped the npm package and Cargo workspace versions to `0.145.0`.

# [0.144.5] - 2026-07-17

Synced the complete Termux fork to the final upstream OpenAI Codex
`rust-v0.144.5` patch release for the npm `next` lane. The complete
Android/Termux compatibility delta remains applied.

## Changed
- Integrated the complete upstream `rust-v0.144.5` delta, including expanded
  dangerous-command detection for additional forced recursive deletion forms.
- Preserved all Termux patches, including Android TLS roots, PTY and lock
  compatibility, real in-process V8 code-mode, bundled libc++, and
  `RUNPATH=$ORIGIN` packaging hardening.
- Bumped npm package and Cargo workspace versions to `0.144.5`.

# [0.144.4] - 2026-07-14

Synced the complete Termux fork to the final upstream OpenAI Codex
`rust-v0.144.4` patch release for the npm `next` lane. Upstream reports no
user-facing changes in this patch; the complete Android/Termux compatibility
delta remains applied.

## Changed
- Integrated the complete upstream `rust-v0.144.4` delta.
- Preserved all Termux patches, including Android TLS roots, PTY and lock
  compatibility, real in-process V8 code-mode, bundled libc++, and
  `RUNPATH=$ORIGIN` packaging hardening.
- Added safe compatibility for model catalog entries that omit
  `base_instructions`, with an embedded-instruction fallback for blank catalog
  data.
- Bumped npm package and Cargo workspace versions to `0.144.4`.

# [0.144.3] - 2026-07-13

Synced the complete Termux fork to upstream OpenAI Codex `rust-v0.144.3` for
the npm `next` lane. The complete Android/Termux compatibility delta remains
applied; npm `latest` remains on `0.144.1` and `stable` on `0.143.0`.

## Changed
- Integrated the complete upstream `rust-v0.144.3` delta, including the
  advanced reasoning picker and persisted thread reasoning effort.
- Preserved all Termux patches, including Android TLS roots, PTY and lock
  compatibility, real in-process V8 code-mode, bundled libc++, and
  `RUNPATH=$ORIGIN` packaging hardening.
- Bumped npm package and Cargo workspace versions to `0.144.3`.

# [0.140.0] - 2026-06-16

Synced the Termux fork to upstream OpenAI Codex `rust-v0.140.0`. Validated
on device (AI-guided surface report, PASS) and promoted to npm `latest`;
the `stable` dist-tag stays on `0.135.0`.

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.140.0`.
- Preserved the full Android/Termux runtime delta, including Patch #24
  (Termux TLS roots, no rustls-platform-verifier panic) and real code-mode
  on Android via the in-process V8 runtime.
- Bumped npm package and Cargo workspace versions to `0.140.0`.

# [0.139.0] - 2026-06-11

Synced the Termux fork to upstream OpenAI Codex `rust-v0.139.0`. Validated
on device (AI-guided surface report, PASS) and promoted to npm `latest`;
the `stable` dist-tag stays on `0.135.0`.

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.139.0`.
- Preserved the full Android/Termux runtime delta, including Patch #24
  (Termux TLS roots, no rustls-platform-verifier panic) and real code-mode
  on Android via the in-process V8 runtime.
- Bumped npm package and Cargo workspace versions to `0.139.0`.

# [0.138.0] - 2026-06-09

Synced the Termux fork to upstream OpenAI Codex `rust-v0.138.0`. Validated
on device (AI-guided surface report, PASS) and promoted to npm `latest`;
the `stable` dist-tag stays on `0.135.0`.

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.138.0`.
- Preserved the full Android/Termux runtime delta, including Patch #24
  (Termux TLS roots, no rustls-platform-verifier panic) and real code-mode
  on Android via the in-process V8 runtime (147.4.0).
- Bumped npm package and Cargo workspace versions to `0.138.0`.

# [0.137.0] - 2026-06-04

Synced the Termux fork to upstream OpenAI Codex `rust-v0.137.0`. Validated
on device (AI-guided surface report, PASS) and promoted to npm `latest`;
the `stable` dist-tag stays on `0.135.0`.

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.137.0`
  (119 upstream commits assimilated across `rust-v0.136.0..rust-v0.137.0`).
- Preserved the full Android/Termux runtime delta, including Patch #24
  (Termux TLS roots, no rustls-platform-verifier panic) introduced in 0.136.1.
- Bumped npm package and Cargo workspace versions to `0.137.0`.

# [0.136.1] - 2026-06-03

Hotfix for the 0.136.0 startup crash on Termux ([#11]). Promoted to npm
`latest` — the `stable` dist-tag stays on the 0.135.0 line.

## Fixed
- **Startup TLS panic on Termux** (`Expect rustls-platform-verifier to be
  initialized`): reqwest 0.13 (rmcp 1.7.0 upgrade) verifies TLS through
  `rustls-platform-verifier`, which on Android requires an initialized JVM
  Context that a Termux CLI process does not have, so the TUI crashed at the
  first TLS handshake right after startup. The Termux build now supplies the
  embedded Mozilla roots (`webpki-root-certs`) via
  `ClientBuilder::tls_certs_only()`, runtime-gated on `TERMUX_VERSION`
  (Patch #24); desktop targets keep the default platform verifier and custom
  CA bundles keep precedence. ([#11])

[#11]: https://github.com/DioNanos/codex-termux/issues/11

# [0.136.0] - 2026-06-02

Synced the Termux fork to upstream OpenAI Codex `rust-v0.136.0`. Published on
npm `next` for on-device validation; npm `latest` keeps tracking `0.135.0`
until `0.136.0` is promoted.

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.136.0`.
- Bumped npm package and Cargo workspace versions to `0.136.0`.

## Restored (Android)
- **code-mode (`exec`/`wait`)**: reverted the Android code-mode stub. The real
  in-process V8 runtime is now enabled on Android via the fork-owned
  `aarch64-linux-android` `rusty_v8` prebuild, so code-mode is no longer a
  no-op on the published Termux package. This is the meaningful capability gain.

## Known limitation (Android / Termux)
- **Realtime voice/audio is not usable in Termux CLI.** The `voice`/`audio_device`
  modules now build for Android (cpal links `c++_shared` via `oboe-shared-stdcxx`,
  see openai/codex#24507), but the audio backend (cpal → oboe → `ndk-context`)
  requires an Android `JavaVM`/`Activity` to initialize, which a Termux CLI
  process does not have. The experimental `/realtime` and `/settings` commands
  cannot open an audio device under Termux (the feature is off by default; do
  not enable it on Termux). This fork intentionally does not change the audio
  backend; a Termux-native audio backend (PulseAudio / `termux-api`) is tracked
  on the Codex VL roadmap.

# [0.135.0] - 2026-05-29

Stable release. Promotion from the validated `0.135.0-alpha.0` lane is
version-only: upstream `rust-v0.135.0` differs from `rust-v0.135.0-alpha.2`
solely by the release version bump (no code change). The fork ships the same
binaries already built in CI and smoke-tested on the alpha `next` lane, now
published on npm `latest`.

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.135.0`
  (86 upstream commits assimilated across `rust-v0.134.0..rust-v0.135.0`).
- Preserved the Android/Termux runtime delta: browser login via
  `termux-open-url`, fork-owned update channels, npm wrapper hardening,
  ELF `RUNPATH=$ORIGIN`, Android no-voice policy, Termux-compatible
  release profile.
- Preserved 0.134.1 cross-fork fixes: `flock` ENOTSUP/EOPNOTSUPP tolerance
  across 7 callsite guards (`app-server-transport`,
  `core/installation_id`, `message-history`, `arg0`, `execpolicy`,
  `app-server-daemon/lib` + `pid`), TERMUX_ENV_VARS allowlist for
  `npx`-spawned MCP servers in `rmcp-client/utils.rs`, and the
  remote-control account-id flaky-test timeout raise to 950 ms.
- Kept public install, source-build, update, and release-staging surfaces on
  `DioNanos/codex-termux` and `@mmmbuto/codex-cli-termux`.
- Synchronised README upstream-base references (root, npm package,
  `docs/install.md`, `BUILDING.md`) to `rust-v0.135.0`.
- Aligned Cargo workspace and lockfile package versions to `0.135.0`.

## Verify
- Patch inventory: `bash verify-patches.sh` passes the runtime patches
  (#1, #2, #4/#5, #6, #6b, #10, #10b, #11, #12, #13, #14, #15, #16, #17,
  #18, #19, #20, #21, #22, #23) and the Bazel patch inventory check.
- Patch #18 check updated to the renamed helper
  `is_unsupported_file_lock_error` (kind-based) introduced in 0.134.1.

# [0.134.0-termux] - 2026-05-26

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.134.0` (stable).
- Preserved the Android/Termux runtime delta: browser login via
  `termux-open-url`, fork-owned update channels, npm wrapper hardening,
  ELF `RUNPATH=$ORIGIN`, Android no-voice policy, Termux-compatible
  release profile, and the `flock` ENOTSUP/EOPNOTSUPP tolerance for
  `codex remote-control` on Termux storage backends.
- Kept public install, source-build, update, and release-staging surfaces on
  `DioNanos/codex-termux` and `@mmmbuto/codex-cli-termux`.
- Synchronised README upstream-base references (root, npm package,
  `docs/install.md`, `BUILDING.md`) to `rust-v0.134.0`.
- Aligned Cargo workspace and lockfile package versions with the upstream
  `0.134.0` release.
- `rusty_v8` Android prebuilt manifest is unchanged: V8 stays at 147.4.0.

## Documented
- Surfaced 8 previously-undocumented Android/Termux compatibility patches
  in `patches/README.md` and added matching checks to `verify-patches.sh`
  (Patches #6b, #17, #18, #19, #20, #21, #22, #23). No new code; the
  inventory now reflects the existing fork delta accurately.

## Upstream
- Search across local conversation history (case-insensitive content matches
  with result previews).
- `--profile` becomes the primary profile selector across CLI, TUI
  permissions, and sandbox flows; legacy profile configs are rejected with
  migration guidance.
- MCP setup gained per-server environment targeting and OAuth options for
  streamable HTTP servers.
- Connector tool schemas more reliable: preserves local `$ref`/`$defs`
  structures and compacts oversized schemas before exposure.
- Read-only MCP tools can run concurrently when they advertise `readOnlyHint`.
- Richer extension and hook context: conversation history for extension
  tools, subagent identity in hook inputs.
- Goal accounting restored after thread resume; memory state moved to a
  dedicated SQLite DB.

# [0.133.1-termux] - 2026-05-23

## Fixed
- `codex remote-control` no longer aborts daemon startup with
  `lock() not supported` on Android Termux storage backends that
  reject `flock(2)` with `ENOTSUP` / `EOPNOTSUPP`. The two
  `try_lock_file` helpers in `app-server-daemon` now match the
  permissive degradation pattern already used by
  `core::installation_id::is_unsupported_file_lock_error` and return
  "lock acquired" when the OS reports the primitive is unsupported, so
  the daemon start path proceeds and the app-server can bind its
  socket. The race the locks were guarding against is best-effort on
  these filesystems; refusing to start the daemon was the real
  blocker.

## Upstream
- OpenAI Codex `rust-v0.133.0` (unchanged from `0.133.0`'s parent release).

# [0.133.0-termux] - 2026-05-22

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.133.0`.
- Preserved the Android/Termux runtime delta: browser login via
  `termux-open-url`, fork-owned update channels, npm wrapper hardening,
  ELF `RUNPATH=$ORIGIN`, Android no-voice policy, and Termux-compatible
  release profile.
- Kept public install, source-build, update, and release-staging surfaces on
  `DioNanos/codex-termux` and `@mmmbuto/codex-cli-termux`.
- Updated Android `rusty_v8` prebuilt artifacts to v147.4.0.
- Aligned Cargo workspace and lockfile package versions with the upstream
  `0.133.0` release.

## Upstream
- Goals are now enabled by default, backed by dedicated storage, and track
  progress across active turns.
- `codex remote-control` now runs like a foreground command, waits for
  readiness, reports machine status, and keeps explicit daemon-style
  `start`/`stop` commands.
- Permission profiles gained list APIs, inheritance, managed
  `requirements.toml` support, runtime refresh behavior, and stronger Windows
  sandbox integration.
- Plugin discovery is easier to inspect, with marketplace-aware list output,
  installed versions, visible marketplace roots, and remote collection support.
- Extensions can observe more lifecycle events, including subagent start/stop,
  tool execution, turn metadata, and async approval/turn processing.

# [0.132.0-termux] - 2026-05-20

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.132.0`.
- Preserved the Android/Termux runtime delta: browser login via
  `termux-open-url`, fork-owned update channels, npm wrapper hardening,
  ELF `RUNPATH=$ORIGIN`, Android no-voice policy, code-mode Android support,
  and Termux-compatible release profile.
- Kept public install, source-build, update, and release-staging surfaces on
  `DioNanos/codex-termux` and `@mmmbuto/codex-cli-termux`.
- Aligned Cargo workspace and lockfile package versions with the upstream
  `0.132.0` release.

## Upstream
- Python SDK authentication now includes API key login, ChatGPT browser and
  device-code flows, account inspection, and logout APIs.
- Python turn APIs accept plain string input for text-only workflows and return
  richer `TurnResult` metadata for handle-based runs.
- `codex exec resume` supports `--output-schema` for structured resumed
  automations.
- TUI startup probes are batched for faster first-frame rendering.
- Remote executor registration can use standard Codex auth.
- App-server turns preserve requested image fidelity, including
  original-resolution local images.

# [0.131.1-termux] - 2026-05-19

## Changed
- Completed fork-safety coverage for diagnostic update guidance: `codex doctor`
  now points npm and bun users to `@mmmbuto/codex-cli-termux`, and release
  checks read `DioNanos/codex-termux` tags.
- Kept TUI update notices on the fork release channel so failed update probes no
  longer send users to upstream OpenAI Codex releases.
- Reworked the Android no-voice policy to use target-OS cfg gates instead of a
  workspace crate feature, matching upstream manifest validation rules while
  keeping voice and realtime audio disabled for Termux builds.
- Updated patch verification so the fork-safety and no-voice invariants are
  checked together before release.

## Upstream
- Upstream base remains OpenAI Codex `rust-v0.131.0`; this is a Termux fork
  patch release with no upstream base change.

# [0.131.0-termux] - 2026-05-19

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.131.0`.
- Preserved the Android/Termux runtime delta: browser login via `termux-open-url`, fork-owned update channels, npm wrapper hardening, ELF `RUNPATH=$ORIGIN`, Android no-voice policy, code-mode Android stubs, and Termux-compatible release profile.
- Hardened fork update paths so standalone update actions and app-server daemon guidance stay on `@mmmbuto/codex-cli-termux@latest`.
- Disabled daemon automatic standalone updater fetches for the Termux fork and kept `autoUpdateEnabled` false by default.
- Aligned Cargo.lock with the upstream `rust-v0.131.0` dependency resolution while preserving workspace version `0.131.0`.

## Upstream
- OpenAI Codex `rust-v0.131.0` is the upstream base for this Termux package.
  The fork includes the upstream CLI/TUI improvements that are compatible with
  Android Termux, while preserving the Termux packaging and no-voice policy.
- Upstream TUI updates include richer session controls and display metadata:
  service-tier commands, blended token usage, permission/approval display,
  effective workspace roots, and responsive Markdown tables.
- Upstream `@` mentions now cover files, directories, plugins, and skills in a
  unified picker backed by app-server plugin metadata.
- Upstream plugin workflows add marketplace commands, version-aware sharing,
  clearer shared-workspace buckets, and default-enabled plugin hooks.
- Upstream remote workflows add daemon-managed `codex remote-control`, runtime
  enable/disable APIs, status reads, and configured remote environments.
- Upstream SDK and diagnostics updates include the `openai-codex` /
  `openai_codex` Python line, approval-mode coverage, and `codex doctor`.

# [0.130.0-termux] - 2026-05-09

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.130.0`.
- Preserved all 9 Termux patches: browser login, release profile, update channel, npm scope, launcher, ELF runpath, no-voice, dynamic subcommand routing, Bazel inventory.
- Upstream highlights: plugin details/show bundled hooks, plugin share discoverability, `codex remote-control` headless entrypoint, thread pagination APIs, Bedrock AWS console-login auth, `view_image` multi-environment support, built-in MCPs as first-class runtime servers.

# [0.128.0-termux] - 2026-04-30

## Changed
- Synced the Termux fork to upstream OpenAI Codex `rust-v0.128.0`.
- Preserved Termux packaging, update URLs, Android runtime patches, and fork npm scope `@mmmbuto/codex-cli-termux`.

# [0.126.0-termux] - 2026-04-30

- Merged current upstream OpenAI Codex `main` into the Termux fork.
- Kept Android/Termux packaging, rusty_v8 Android artifact fetching, voice/realtime stubs, and npm self-update targeting `@mmmbuto/codex-cli-termux`.
- Prepared GitHub Actions Android build/publish pipeline for staged `next` testing before any `latest` or public release promotion.

# [0.125.0-termux] - 2026-04-26

### Upstream
- OpenAI Codex `rust-v0.125.0` release: https://github.com/openai/codex/releases/tag/rust-v0.125.0
- Fork line rebuilt cleanly from upstream `rust-v0.125.0`.

### Termux Patches
- Kept Android browser login via `termux-open-url`.
- Kept the fork update channel and `-termux` version parsing for self-update UX.
- Kept Termux npm package/update commands targeting `@mmmbuto/codex-cli-termux`.
- Kept launcher hardening via wrapped entrypoints and `CODEX_SELF_EXE`.
- Kept Android ELF `RUNPATH=$ORIGIN` hardening so direct native invocation still resolves bundled `libc++_shared.so`.
- Kept the Android no-voice policy for the published Termux package.
- Kept Android `exec`/code-mode disabled in the published Termux package.
- Kept the Android `openpty` shim for PTY compatibility on Bionic.
- Kept Android tolerance for unsupported file locks used by `installation_id` and arg0 helper setup.
- Aligned code-mode Android stubs with upstream 0.125.0 runtime restructuring (new `runtime/` dir, `WaitOutcome`, `CodeModeNestedToolCall`).
