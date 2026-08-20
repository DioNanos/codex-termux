# Codex CLI for Termux

> Android Termux package built from upstream OpenAI Codex `rust-v0.148.0`.

Package metadata for the Termux-focused line `@mmmbuto/codex-cli-termux`.

## Install

```bash
pkg update && pkg upgrade -y
pkg install nodejs-lts -y
npm install -g @mmmbuto/codex-cli-termux@latest
codex --version
codex login
```

## Notes

- Android 10+ / API 29+ on Termux ARM64 (the release binary is built for API 29)
- Built from upstream `rust-v0.148.0`
- Carries only the Termux compatibility delta needed for packaging and runtime
- Real code-mode (`exec`/`wait`) is enabled on the native Android build via the in-process V8 runtime (no longer stubbed) — this is the meaningful capability gain on Termux
- Realtime voice/audio is not part of this build: upstream removed the TUI
  realtime voice surface, so the fork's former Android `cpal`/`oboe` toggle was
  retired as well. A plain Termux CLI process has no Android `JavaVM`/`Activity`
  for that backend; a future Termux-native audio path is tracked separately.
- Packaged launchers preserve bundled `libc++_shared.so` visibility
- Android ELFs are hardened with `RUNPATH=$ORIGIN`
- Fork-owned Android `rusty_v8` prebuilds are used for maintainer cross-builds
- GitHub Actions builds the Android ARM64 tarball from an exact sanitized candidate ref; the maintainer verifies that artifact and publishes the unchanged tarball locally to `next`. GitHub `main`, tags and Releases remain separate promotion steps.

See the main repository for release notes and patch inventory:

- https://github.com/DioNanos/codex-termux
- https://github.com/DioNanos/codex-termux/blob/main/patches/README.md
