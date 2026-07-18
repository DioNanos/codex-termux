# Codex Termux

> Native Codex CLI for **Termux / Android ARM64**.
> This fork tracks upstream OpenAI Codex main and carries only the Android/Termux compatibility delta needed to package and run it.

[![npm termux](https://img.shields.io/npm/v/@mmmbuto/codex-cli-termux?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/codex-cli-termux)
[![latest release](https://img.shields.io/github/v/release/DioNanos/codex-termux?style=flat-square)](https://github.com/DioNanos/codex-termux/releases/latest)

<p align="center">
  <img src="./.github/termux-robot.png" alt="Termux robot" width="80%" />
</p>

## Install

### Termux (Android ARM64)

```bash
pkg update && pkg upgrade -y
pkg install nodejs-lts -y
npm install -g @mmmbuto/codex-cli-termux@next
codex --version
codex login
```

Requirements:

- Android 7+ / API 24+
- ARM64 device
- Node.js >= 18

## Scope

What this fork does:

- tracks upstream OpenAI Codex closely
- builds native Android ARM64 binaries for Termux
- applies only the compatibility patches upstream does not ship
- publishes GitHub release assets and an npm package for Termux users

What this fork does not do:

- maintain a broad feature fork
- replace upstream Codex
- carry fork-only product features unrelated to Termux compatibility

## Current Termux Delta

- browser login uses `termux-open-url`
- self-update points to `DioNanos/codex-termux` and `@mmmbuto/codex-cli-termux`
- packaged wrappers preserve `CODEX_SELF_EXE`, sanitize `LD_LIBRARY_PATH`, and bundle `libc++_shared.so`
- Android binaries are linked with `RUNPATH=$ORIGIN`
- `exec`/code-mode now runs for real on Android via the in-process V8 runtime (no longer a stub) — the meaningful capability gain on Termux
- realtime voice/audio is no longer part of this build: upstream removed the TUI realtime voice feature (openai/codex#27801), so the fork's Android cpal/oboe enablement toggle (never usable from the Termux CLI anyway, as the backend needs an Android JavaVM/Activity) was dropped with it. Termux-native audio remains tracked on the Codex VL roadmap.
- Android PTY and lock-handling compatibility patches remain enabled where upstream behavior still breaks on Bionic/Termux

## Releases and Updates

- Latest GitHub release: [releases/latest](https://github.com/DioNanos/codex-termux/releases/latest)
- Upstream base: OpenAI Codex `rust-v0.144.5`, packaged as `0.144.5` on npm `next`. The `latest` dist-tag remains on `0.144.4`; `stable` stays on `0.143.0` for conservative installs.
- npm package: [`@mmmbuto/codex-cli-termux`](https://www.npmjs.com/package/@mmmbuto/codex-cli-termux)
- Legacy `@mmmbuto/codex-cli-lts` (OpenAI Codex 0.80.x) is archived; current builds live in this package or in [`@mmmbuto/codex-vl`](https://www.npmjs.com/package/@mmmbuto/codex-vl) (multi-platform).

Maintainer publish flow:

- land validated changes on `develop`
- publish the tested npm package to `next`
- promote the tested commit to clean GitHub `main`
- publish the GitHub release from `main`
- add post-release Termux validation reports after device testing

## Documentation

- [Changelog](./CHANGELOG.md)
- [Patch inventory](./patches/README.md)
- [Building from source](./BUILDING.md)
- Latest runtime validation report: 0.144.5 validation pending; previous report [v0.142.0 Termux device validation](./test-report/CODEX_TEST_REPORT_v0.142.0_run_20260623-2007.md)
- [Install docs](./docs/install.md)
- [Authentication](./docs/authentication.md)
- [Configuration](./docs/config.md)

## Security

This is a community fork of OpenAI Codex. Security-relevant properties of this build:

- **Network**: agents bind to loopback by default; nothing is exposed externally unless you opt in.
- **Supply chain**: builds and releases come only from fork-owned CI and the `@mmmbuto/...` npm
  scope. This package does not silently fetch or run the upstream installer; updates flow through
  the fork's own channel.
- **Termux**: TLS trust uses bundled webpki roots (no Android platform-verifier dependency), and
  advisory file locks degrade safely on filesystems that don't support them.

For sensitive work, prefer the official Codex CLI on Linux/macOS over SSH.

To report a vulnerability, see [SECURITY.md](./SECURITY.md).

## Community guides

- [OpenAI Codex CLI on Android via Termux](https://timharbakon.com/openai-codex-cli-android-termux/)
  — independent third-party walkthrough covering Termux setup, the Bionic-libc incompatibility
  this package solves, and a small Hono test project.

## License

This project remains under the Apache 2.0 license inherited from OpenAI Codex.

- Original work: OpenAI
- Termux port: minimal Android compatibility patches

See [LICENSE](./LICENSE).
