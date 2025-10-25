# Automated Release Pipeline - Codex CLI for Termux

## Overview

This document describes the automated pipeline that synchronizes OpenAI Codex, compiles it for ARM64 Termux, packages it for npm, and publishes releases.

---

## Pipeline Flow

```
1. Sync Upstream          → Fetch latest from github.com/openai/codex
2. Extract Version        → Read version from upstream tags (rust-vX.Y.Z)
3. Compile ARM64          → cargo build --release for Termux/ARM64
4. Test Binary            → Verify codex --help and --version
5. Prepare npm Package    → Copy binary into npm package structure
6. Publish to npm         → npm publish @mmmbuto/codex-cli-termux
7. Create Distribution    → Generate tar.gz + SHA256 archive
8. Push to GitHub         → git push to fork main branch
9. Update README          → Auto-generated with version info
10. Create Release        → GitHub release with changelog
```

**Total Duration**:
- 16GB devices: 20-30 minutes
- 8GB devices: 30-40 minutes
- (first run with compilation)

---

## Version Alignment

The pipeline **automatically aligns with OpenAI Codex upstream**:

1. Detects latest `rust-vX.Y.Z` tag from upstream
2. Extracts version (e.g., `rust-v0.49.0` → `0.49.0`)
3. Appends `-termux` suffix (e.g., `0.49.0-termux`)
4. Uses same version for:
   - Compiled binary
   - npm package
   - GitHub release tag

**Example**:
- Upstream: `rust-v0.49.0`
- Our release: `0.49.0-termux`
- npm: `@mmmbuto/codex-cli-termux@0.49.0-termux`
- GitHub: Release tag `v0.49.0-termux`

---

## Execution

### Prerequisites

- Node.js ≥ 14
- npm ≥ 6
- Rust toolchain + cargo
- Git with SSH support
- Environment variables:
  - `GITHUB_TOKEN` - GitHub PAT for releases
  - `NPM_TOKEN` - npm auth token
  - `GIT_AUTHOR_NAME` - Commit author
  - `GIT_AUTHOR_EMAIL` - Commit email

### Quick Commands

```bash
# Full pipeline
./pipeline.sh --all

# Individual steps
./pipeline.sh --sync          # Fetch upstream only
./pipeline.sh --compile       # Compile binary
./pipeline.sh --test          # Test compiled binary
./pipeline.sh --publish-npm   # Publish to npm
./pipeline.sh --github-release # Create GitHub release

# Dry run
./pipeline.sh --dry-run       # Check prerequisites
```

---

## Compilation Details

**Platform**: ARM64 (Termux/Android)

### Automatic RAM Detection & Optimization

The pipeline automatically detects your device's RAM and applies the optimal compilation settings:

**16GB RAM Configuration:**
- Cargo.toml: `lto = false`, `codegen-units = 16`
- Parallel jobs: `-j 2`
- Estimated time: 20-30 minutes
- RAM usage: ~12-14GB

**8GB RAM Configuration:**
- Cargo.toml: `lto = false`, `codegen-units = 32`, `opt-level = 2`
- Parallel jobs: `-j 1`
- Estimated time: 30-40 minutes
- RAM usage: ~6-8GB

**Build Command** (auto-configured):
```bash
# Automatically detects RAM and applies optimizations
cargo build -p codex-cli --release -j [1-2 based on RAM]
```

**Manual Override**:
```bash
# Force 8GB configuration
RAM_CONFIG=8 ./pipeline.sh --all

# Force 16GB configuration
RAM_CONFIG=16 ./pipeline.sh --all
```

**Output Binary**: `codex-termux/target/release/codex`

### Why RAM Optimization Matters

OpenAI Codex default build settings (`lto=fat`, `codegen-units=1`) require 18-22GB RAM and cause OOM errors on most mobile devices. Our pipeline automatically optimizes for Termux/Android constraints.

---

## npm Package Distribution

**Package Name**: `@mmmbuto/codex-cli-termux`

**Installation**:
```bash
npm install -g @mmmbuto/codex-cli-termux@0.49.0-termux
```

**Contents**:
- Pre-compiled `codex` binary for ARM64
- `postinstall` script for setup
- Full license + attribution

---

## Output Artifacts

After successful pipeline execution:

1. **Binary**: `codex-termux/bin/codex` (ARM64 executable)
2. **npm Package**: Published to registry as `@mmmbuto/codex-cli-termux@X.Y.Z-termux`
3. **Archive**: `codex-npm-package-X.Y.Z-termux.tar.gz` (~22 MB)
4. **Checksum**: `codex-npm-package-X.Y.Z-termux.tar.gz.sha256`
5. **GitHub Release**: `https://github.com/DioNanos/codex-termux/releases/tag/vX.Y.Z-termux`

---

## Security & Compliance

- **No source modifications**: Binary is identical to upstream
- **Full licensing**: Apache 2.0 + OpenAI attribution
- **Version transparency**: All versions traceable to upstream
- **No credentials in commits**: All sensitive data excluded from version control

---

## Troubleshooting

### Compilation fails with OOM errors

The pipeline automatically detects RAM and applies optimizations, but if you still get out-of-memory errors:

```bash
# Force 8GB ultra-optimized config
RAM_CONFIG=8 ./pipeline.sh --all

# Clean build cache
rm -rf codex-rs/target/
./pipeline.sh --compile

# Monitor RAM during compilation
watch -n 1 'free -h'
```

For devices with <8GB RAM, you may need to manually reduce `opt-level` to 1 or 0 in `codex-rs/Cargo.toml`.

### npm publish fails

```bash
# Verify npm auth
npm whoami

# Check token
echo $NPM_TOKEN
```

### GitHub push fails

```bash
# Verify token
echo $GITHUB_TOKEN

# Test connection
git ls-remote origin
```

---

## Maintenance

### Regular Tasks

- **Weekly**: Review upstream changes
- **Monthly**: Verify token expiration
- **Quarterly**: Update dependencies (`cargo update`)

### Documentation

Keep this document synchronized with:
- `pipeline.sh` (main orchestration script)
- `README.md` (user-facing)
- Deployment process changes

---

## References

- **Upstream**: [OpenAI Codex](https://github.com/openai/codex)
- **This Fork**: [DioNanos/codex-termux](https://github.com/DioNanos/codex-termux)
- **npm Package**: [@mmmbuto/codex-cli-termux](https://www.npmjs.com/package/@mmmbuto/codex-cli-termux)

---

**Last Updated**: 2025-10-25
**Pipeline Status**: ✅ Production Ready
**Features**: ✨ Auto RAM detection (8GB/16GB support)
**Current Version Tracking**: OpenAI Codex upstream
