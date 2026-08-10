#!/usr/bin/env bash
# build_rusty_v8_android.sh
#
# Build the rusty_v8 (`v8` crate) static archive + bindgen output for
# aarch64-linux-android FROM SOURCE, on a Linux x86_64 host.
#
# WHY THIS SCRIPT EXISTS
#   denoland/rusty_v8 ships prebuilt archives for DESKTOP targets only
#   (apple-darwin / linux-gnu / windows-msvc). There is no android prebuilt
#   anywhere. denoland's own from-source build (V8_FROM_SOURCE=1 + recursive
#   submodules, NO gclient) is exercised only for desktop in their CI, so the
#   android cross path hits several gaps that this script patches.
#
#   The fork's scripts/prepare_rusty_v8_android_source.py does the git checkout
#   but only inits 4 submodules and leaves the android-specific gn/bindgen setup
#   to the caller. This script wraps it and fills every gap discovered while
#   getting a real 149.2.0 Android build through on the maintainer runner.
#
# STATUS (2026-06-08)
#   COMPLETE end-to-end: V8 compiles from source for aarch64-linux-android
#   (librusty_v8.a ~167MB), bindgen generates src_binding.rs, and the v8 crate
#   compiles 100% for the target. All 7 obstacles resolved.
#
# Usage:
#   build_rusty_v8_android.sh --tag v149.2.0 [--ndk r28c] [--jobs N]
#
set -euo pipefail

# SUPERSEDED (2026-08-09) by .github/workflows/rusty-v8-android-release.yml.
#
# This script builds the PLAIN profile. Since rust-v0.147.0, code mode enables
# the v8 crate's `v8_enable_sandbox` feature, and a plain archive links and runs
# with the sandbox absent while saying nothing about it -- which is exactly the
# defect the 0.147.x corrective exists to fix. Publishing what this produces
# under the current release layout would put that archive back in front of the
# consumers.
#
# The workflow supersedes it in full: it builds either profile, verifies the
# result by decoding what `v8__V8__IsSandboxEnabled` returns, and publishes with
# the profile in the artifact name. The obstacles documented above are solved
# there too, which is why this file is kept readable rather than deleted.
#
# Refuse by default rather than sit here as a working path to the wrong artifact.
if [ "${CODEX_ALLOW_SUPERSEDED_V8_BUILDER:-0}" != "1" ]; then
  cat >&2 <<'SUPERSEDED'
scripts/build_rusty_v8_android.sh is superseded and builds the PLAIN V8 profile.
Code mode requires the sandbox profile; a plain archive links, runs, and reports
nothing while the sandbox is absent.

Use .github/workflows/rusty-v8-android-release.yml (input `sandbox: true`).
Set CODEX_ALLOW_SUPERSEDED_V8_BUILDER=1 only to study this path, never to
produce an artifact meant for release.
SUPERSEDED
  exit 1
fi

TAG=""
NDK_REV="r28c"                 # NDK package revision (r28c == 28.2.13676358)
JOBS="$(nproc)"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

while [ $# -gt 0 ]; do
  case "$1" in
    --tag) TAG="$2"; shift 2;;
    --ndk) NDK_REV="$2"; shift 2;;
    --jobs) JOBS="$2"; shift 2;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done
[ -n "$TAG" ] || { echo "usage: $0 --tag vX.Y.Z [--ndk r28c] [--jobs N]" >&2; exit 2; }

HOME_TOOLS="${HOME}/.cache/rusty_v8_android_build"
mkdir -p "$HOME_TOOLS"
NDK_HOME="${HOME_TOOLS}/android-ndk-${NDK_REV}"
GN_BIN="${HOME_TOOLS}/gn/gn"

log() { echo "=== $* ($(date +%H:%M:%S)) ==="; }

# --- prerequisite tools -------------------------------------------------------
log "tooling: gn (CIPD) + NDK ${NDK_REV}"

# OBSTACLE 1: apt's gn is too old for V8 149.x (missing path_exists() used in
# siso.gni). Fetch a recent gn prebuilt from chrome-infra CIPD.
if [ ! -x "$GN_BIN" ]; then
  mkdir -p "${HOME_TOOLS}/gn"
  curl -fsSL "https://chrome-infra-packages.appspot.com/dl/gn/gn/linux-amd64/+/latest" -o /tmp/gn.zip
  unzip -o /tmp/gn.zip -d "${HOME_TOOLS}/gn" >/dev/null
  chmod +x "$GN_BIN"
fi
"$GN_BIN" --version

# NDK: download the standalone zip if not present (r28c == 28.2.13676358).
if [ ! -d "$NDK_HOME" ]; then
  curl -fsSL "https://dl.google.com/android/repository/android-ndk-${NDK_REV}-linux.zip" -o /tmp/ndk.zip
  unzip -q -o /tmp/ndk.zip -d "$HOME_TOOLS"
fi
NDK_REV_FULL="$(grep Pkg.Revision "${NDK_HOME}/source.properties" | awk '{print $3}')"
NDK_MAJOR="${NDK_REV_FULL%%.*}"          # e.g. 28
echo "NDK ${NDK_REV_FULL} (major ${NDK_MAJOR})"

rustup target add aarch64-linux-android >/dev/null 2>&1 || true

# --- checkout + submodules ----------------------------------------------------
log "checkout rusty_v8 ${TAG}"
python3 "${REPO_ROOT}/scripts/prepare_rusty_v8_android_source.py" --tag "${TAG}"
CO="${REPO_ROOT}/.artifacts/rusty_v8-src/${TAG}"

# OBSTACLE 2: the prepare script only inits 4 submodules (build, v8,
# tools/clang, buildtools). V8 from-source needs the FULL set rusty_v8 ships
# as its own submodules (icu, abseil-cpp, jinja2, markupsafe, simdutf, fp16,
# libc++, ...). Init them all.
log "init ALL submodules (recursive)"
git -C "$CO" submodule update --init --recursive --depth 1

# OBSTACLE 3: build/config/android/BUILD.gn uses android_ndk_version but
# nothing declares it (chromium_build only declares android_ndk_root for the
# standalone case, and the gclient-runhooks path that would define the version
# is never run). Declare it next to android_ndk_root for our NDK.
log "patch config.gni: declare android_ndk_version"
CFG="${CO}/build/config/android/config.gni"
if ! grep -q 'android_ndk_version =' "$CFG"; then
  sed -i 's#\(  android_ndk_root = "//third_party/android_toolchain/ndk"\)#\1\n  android_ndk_version = "r'"${NDK_MAJOR}"'"#' "$CFG"
fi

# Wire the downloaded NDK into the checkout (prepare script created the
# third_party/android_toolchain/ndk -> ../android_ndk symlink; point
# android_ndk at the real NDK).
ln -sfn "$NDK_HOME" "${CO}/third_party/android_ndk"

# --- build env ----------------------------------------------------------------
export ANDROID_NDK_HOME="$NDK_HOME"
export ANDROID_NDK_ROOT="$NDK_HOME"
export PATH="${NDK_HOME}/toolchains/llvm/prebuilt/linux-x86_64/bin:${PATH}"
export CC_aarch64_linux_android="aarch64-linux-android29-clang"
export CXX_aarch64_linux_android="aarch64-linux-android29-clang++"
export AR_aarch64_linux_android="llvm-ar"
export GN="$GN_BIN"
export NINJA="$(command -v ninja)"
export V8_FROM_SOURCE=1

# OBSTACLE 4: gn requires the amd64 host sysroot to build the rust host build
# tools (clang_x64_for_rust_host_build_tools). The prepare script lists this as
# a manual "next step"; do it.
log "install host sysroot (amd64)"
( cd "$CO" && python3 build/linux/sysroot_scripts/install-sysroot.py --arch=amd64 )

# OBSTACLE 5 + 6: build.rs only sets up the bindgen sysroot for macos/linux,
# never android, so bindgen parses V8 headers against the HOST glibc and fails.
# And the NDK's own libclang is clang 19, too old for V8 149's libc++ (needs
# clang 21+ builtins like __builtin_clzg). Point bindgen at:
#   - the android NDK sysroot (BINDGEN_EXTRA_CLANG_ARGS), and
#   - a recent libclang (rusty_v8 downloads clang 23 into third_party/rust-
#     toolchain during the first build, so we do a throwaway build pass first
#     to populate it, then set LIBCLANG_PATH and build for real).
RT_LIB="${CO}/third_party/rust-toolchain/lib"
if [ ! -e "${RT_LIB}/libclang.so" ]; then
  log "priming build (downloads gn/ninja + rust-toolchain clang)"
  ( cd "$CO" && nice -n 12 cargo build --lib --release --target aarch64-linux-android -j "$JOBS" ) || true
fi
export LIBCLANG_PATH="$RT_LIB"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-linux-android --sysroot=${NDK_HOME}/toolchains/llvm/prebuilt/linux-x86_64/sysroot"

# OBSTACLE 7: on the android cross build bindgen 0.72 emits WriteFlags_* instead
# of the namespace-qualified v8_String_WriteFlags_* for the anonymous enum inside
# v8::String::WriteFlags, so the v8 crate fails to link. Alias the two affected
# constants in src/binding.rs (idempotent). Only WriteFlags is affected.
if ! grep -q 'v8_String_WriteFlags_kNullTerminate' "${CO}/src/binding.rs"; then
  cat >> "${CO}/src/binding.rs" <<'PATCH'

pub const v8_String_WriteFlags_kNullTerminate: WriteFlags__bindgen_ty_1 = WriteFlags_kNullTerminate;
pub const v8_String_WriteFlags_kReplaceInvalidUtf8: WriteFlags__bindgen_ty_1 = WriteFlags_kReplaceInvalidUtf8;
PATCH
fi

log "build rusty_v8 (V8 from source) — this is the long step (~hours on few cores)"
( cd "$CO" && nice -n 12 cargo build --lib --release --target aarch64-linux-android -j "$JOBS" -vv )

# --- collect artifacts --------------------------------------------------------
A="$(find "${CO}/target/aarch64-linux-android/release/gn_out/obj" -name 'librusty_v8.a' | head -1)"
B="${CO}/target/aarch64-linux-android/release/gn_out/src_binding.rs"
OUT="${REPO_ROOT}/.artifacts/rusty-v8-android-${TAG}"
mkdir -p "$OUT"
cp "$A" "${OUT}/librusty_v8_release_aarch64-linux-android.a"
cp "$B" "${OUT}/src_binding_release_aarch64-linux-android.rs"
# OBSTACLE 7, second half: the patch above lands in the checkout's src/binding.rs,
# which is what makes the crate compile HERE. The artifact we publish is the
# GENERATED binding from gn_out, which never saw that patch — consumers fetch it
# verbatim (scripts/fetch_rusty_v8_android.py pins its sha256), so without the
# aliases the fork's android build fails on undeclared v8_String_WriteFlags_*.
# The v149.2.0 artifact was fixed up by hand; do it here so it cannot be forgotten.
if ! grep -q 'v8_String_WriteFlags_kNullTerminate' "${OUT}/src_binding_release_aarch64-linux-android.rs"; then
  cat >> "${OUT}/src_binding_release_aarch64-linux-android.rs" <<PATCH

// Compatibility aliases: v8 crate ${TAG#v} string.rs uses v8_String_WriteFlags_ prefix
pub const v8_String_WriteFlags_kNullTerminate: WriteFlags__bindgen_ty_1 = WriteFlags_kNullTerminate;
pub const v8_String_WriteFlags_kReplaceInvalidUtf8: WriteFlags__bindgen_ty_1 = WriteFlags_kReplaceInvalidUtf8;
PATCH
fi
gzip -kf9 "${OUT}/librusty_v8_release_aarch64-linux-android.a"
( cd "$OUT" && sha256sum librusty_v8_release_aarch64-linux-android.a.gz src_binding_release_aarch64-linux-android.rs )
log "artifacts in ${OUT}"

echo "DONE. Mechanism complete: librusty_v8.a + src_binding.rs produced and the"
echo "v8 crate compiles for aarch64-linux-android. Artifacts in ${OUT}"
