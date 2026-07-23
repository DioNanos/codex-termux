## Installing Codex CLI for Termux

This package is for Android Termux on ARM64 devices.

### Requirements

| Requirement | Details |
| --- | --- |
| Android | Android 10+ / API 29+ (release target) |
| CPU | ARM64 |
| Shell | Termux |
| Node.js | 18+ |

### Install from npm

```bash
pkg update && pkg upgrade -y
pkg install nodejs-lts -y
npm install -g @mmmbuto/codex-cli-termux@latest
codex --version
codex login
```

The npm package includes one native Android ARM64 `codex` binary, `codex` and
`codex-exec` launcher scripts, and the bundled `libc++_shared.so` runtime
library. The `codex-exec` launcher dispatches the native binary's `exec`
subcommand instead of duplicating the V8-linked ELF.

### Install from GitHub release

Download the `mmmbuto-codex-cli-termux-<version>.tgz` asset from the matching
GitHub release, then install it with npm:

```bash
npm install -g ./mmmbuto-codex-cli-termux-0.145.0.tgz
codex --version
```

Each release also publishes a `.sha256` checksum file for the npm tarball.

### Build from source

For source builds and maintainer cross-build notes, see [BUILDING.md](../BUILDING.md).

## Logging

Codex honors the `RUST_LOG` environment variable. The TUI writes logs under the
Codex log directory by default, and `codex exec` prints error-level messages
inline for non-interactive runs.
