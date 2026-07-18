# Codex CLI for Termux

> Android Termux package built from upstream OpenAI Codex `rust-v0.144.5`.

This package publishes the latest Termux-focused line as `@mmmbuto/codex-cli-termux`.

## Install

```bash
pkg update && pkg upgrade -y
pkg install nodejs-lts -y
npm install -g @mmmbuto/codex-cli-termux@next
codex --version
codex login
```

## Notes

- Android Termux ARM64 only
- Built from upstream `rust-v0.144.5`
- Carries only the Termux compatibility delta needed for packaging and runtime
- Real code-mode (`exec`/`wait`) is enabled on the native Android build via the in-process V8 runtime (no longer stubbed) — this is the meaningful capability gain on Termux
- Realtime voice/audio: **not usable in Termux CLI.** The audio backend (cpal → oboe → `ndk-context`) needs an Android `JavaVM`/`Activity` that a command-line process in Termux does not have, so the experimental `/realtime` and `/settings` commands cannot open an audio device. The feature is off by default; do not enable it on Termux. This fork deliberately does **not** modify the audio backend (it stays on the upstream Codex path); a Termux-native audio backend (PulseAudio / `termux-api`) is tracked on the Codex VL roadmap, not here.
- Packaged launchers preserve bundled `libc++_shared.so` visibility
- Android ELFs are hardened with `RUNPATH=$ORIGIN`
- Fork-owned Android `rusty_v8` prebuilds are used for maintainer cross-builds
- Maintainer publish path is the repository GitHub Actions workflow from `develop`, with GitHub `main` and release promotion after build/package verification

See the main repository for release notes and patch inventory:

- https://github.com/DioNanos/codex-termux
- https://github.com/DioNanos/codex-termux/blob/main/patches/README.md
