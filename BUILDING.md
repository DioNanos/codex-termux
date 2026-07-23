# Building Codex CLI (Termux fork)

This repository packages upstream OpenAI Codex for Android Termux (ARM64) with
only the compatibility delta required for packaging and runtime.

Most users should install the published package:

```bash
npm install -g @mmmbuto/codex-cli-termux@latest
```

## Native Termux build

On an ARM64 Termux device:

```bash
pkg update && pkg upgrade -y
pkg install git clang lld rust pkg-config openssl openssl-tool nodejs-lts -y

git clone https://github.com/DioNanos/codex-termux.git
cd codex-termux/codex-rs
cargo build --release -p codex-cli
```

Expected outputs:

```bash
codex-rs/target/release/codex
```

## Linux-host Android cross-build

On a Linux maintainer host with Android NDK installed:

```bash
export ANDROID_NDK_HOME="/path/to/android-sdk/ndk/28.2.13676358"
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
export LIBLZMA_NO_PKG_CONFIG=1
export PKG_CONFIG_ALLOW_CROSS=1
export OPENSSL_NO_PKG_CONFIG=1
export CODEX_SKIP_VENDORED_BWRAP=1
export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
export CC_aarch64_linux_android="aarch64-linux-android29-clang"
export CXX_aarch64_linux_android="aarch64-linux-android29-clang++"
export AR_aarch64_linux_android="llvm-ar"
export RANLIB_aarch64_linux_android="llvm-ranlib"

eval "$(python3 scripts/fetch_rusty_v8_android.py | grep '^export ' | sed 's/^export //')"
export RUSTY_V8_ARCHIVE RUSTY_V8_SRC_BINDING_PATH

builtins="$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64" \
  -name 'libclang_rt.builtins-aarch64-android.a' -print -quit 2>/dev/null)"
test -n "$builtins"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-Clink-arg=-lc++_shared -Clink-arg=-Wl,-rpath,\$ORIGIN -Clink-arg=$builtins"

cd codex-rs
rustup run 1.95.0 cargo build --target aarch64-linux-android --release -p codex-cli
```

Expected outputs:

```bash
codex-rs/target/aarch64-linux-android/release/codex
```

Notes:

- The maintainer release toolchain is Rust `1.95.0`.
- The GitHub Actions release workflow installs Android NDK `28.2.13676358`.
- Local cross-builds can use a compatible NDK for exploratory work, but release
  parity is checked against the GitHub Actions workflow.

## Package layout smoke test

To test the npm wrapper layout locally after a successful cross-build:

```bash
cd npm-package
cp ../codex-rs/target/aarch64-linux-android/release/codex bin/codex.bin
cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" bin/libc++_shared.so
chmod 0755 bin/codex bin/codex-exec bin/codex.bin
chmod 0644 bin/libc++_shared.so
node ./bin/codex.js --version
node ./bin/codex.js --help >/dev/null
```

## Maintainer release notes

- Workspace version lives in `codex-rs/Cargo.toml`.
- Published npm version lives in `npm-package/package.json`.
- Termux patch verification lives in `verify-patches.sh`.
- The maintainer GitHub Actions workflow is `.github/workflows/termux-npm-build-publish.yml`.
- Fork-owned Android `rusty_v8` assets are described in `third_party/v8/android-artifacts.toml`.
- For the `0.145.0` release, the maintainer flow is: build the sanitized public
  candidate with GitHub Actions, download and audit the immutable npm tarball,
  publish that exact tarball locally, promote the tested commit to GitHub
  `main`, and create the tag and GitHub release with the same tarball and
  checksum. Both `latest` and `next` then point to `0.145.0`; `stable` remains
  on the conservative release documented in the release notes.

If the Android `rusty_v8` pair for the resolved crate version does not exist
yet, bootstrap a source checkout with:

```bash
python3 scripts/prepare_rusty_v8_android_source.py
```
