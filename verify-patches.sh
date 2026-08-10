#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

pass() { echo "✅ PRESENT"; }
fail() { echo "❌ MISSING!"; exit 1; }

READELF_BIN="${READELF_BIN:-$(command -v llvm-readelf || command -v readelf || true)}"
PUBLIC_SANITIZED_TREE="${CODEX_PUBLIC_SANITIZED_TREE-0}"

case "$PUBLIC_SANITIZED_TREE" in
  0|1) ;;
  *)
    echo "CODEX_PUBLIC_SANITIZED_TREE must be 0 or 1" >&2
    exit 1
    ;;
esac

fork_internal_surfaces_match_contract() {
  if [ "$PUBLIC_SANITIZED_TREE" = "1" ]; then
    # Public GitHub trees intentionally omit Forge-only automation and internal
    # device-test reports. Refuse sanitized mode if either path, including a
    # dangling symlink, leaks into the checked-out tree.
    [ ! -e .forgejo ] && [ ! -L .forgejo ] \
      && [ ! -e test-report ] && [ ! -L test-report ]
  else
    # Forge/develop is the canonical full tree and must retain both surfaces.
    [ -f .forgejo/workflows/termux-next-smoke.yml ] \
      && [ -f test-report/README.md ]
  fi
}

printf "Patch #1 (Browser Login): "
if grep -q "termux-open-url" codex-rs/login/src/server.rs; then
  pass
else
  fail
fi

printf "Patch #2 (Release Profile): "
if grep -q 'lto = "thin"' codex-rs/Cargo.toml \
  && grep -q "codegen-units = 16" codex-rs/Cargo.toml \
  && grep -Fq 'zip = { version = "2.4.2", default-features = false, features = ["deflate", "time"] }' codex-rs/Cargo.toml; then
  pass
else
  fail
fi

printf "Patch #4/#5 (Fork Update Channel + Termux Tag Parser): "
if grep -q "DioNanos/codex-termux" codex-rs/tui/src/updates.rs \
  && grep -q 'current_update_source' codex-rs/tui/src/updates.rs \
  && grep -q 'pub(crate) source: Option<String>' codex-rs/tui/src/updates_cache.rs \
  && grep -q "split('-')" codex-rs/tui/src/update_versions.rs \
  && grep -q "strip_prefix(\"rust-v\")" codex-rs/tui/src/update_versions.rs \
  && grep -q "strip_prefix('v')" codex-rs/tui/src/update_versions.rs; then
  pass
else
  fail
fi

printf "Patch #6 (Termux npm Package Name): "
if grep -q "@mmmbuto/codex-cli-termux@latest" codex-rs/tui/src/update_action.rs; then
  pass
else
  fail
fi

printf "Patch #10 (Launcher Hardening): "
# Both launchers must exec the BUNDLED binary via the absolute "$SCRIPT_DIR"
# path, never a bare name resolved through PATH. The standalone codex-exec.bin
# was dropped, so the codex-exec wrapper now dispatches codex.bin with the exec
# subcommand; the hardening property is preserved, just re-pointed.
if grep -q 'exec "\$SCRIPT_DIR/codex.bin"' npm-package/bin/codex \
  && grep -q 'exec "\$SCRIPT_DIR/codex.bin" exec' npm-package/bin/codex-exec \
  && grep -Fq 'CODEX_SELF_EXE="$SCRIPT_DIR/codex.bin"' npm-package/bin/codex \
  && grep -Fq 'CODEX_SELF_EXE="$SCRIPT_DIR/codex.bin"' npm-package/bin/codex-exec \
  && grep -Fq "env.CODEX_SELF_EXE = nativeBinaryPath" npm-package/bin/codex.js \
  && grep -Fq "env.CODEX_SELF_EXE = nativeBinaryPath" npm-package/bin/codex-exec.js \
  && grep -q '"bin/codex.bin"' npm-package/package.json; then
  pass
else
  fail
fi

printf "Patch #10b (Android ELF Runpath): "
if grep -q 'link-arg=-Wl,-rpath,$ORIGIN' codex-rs/.cargo/config.toml; then
  if [ -x npm-package/bin/codex.bin ] && [ -n "$READELF_BIN" ]; then
    if "$READELF_BIN" -d npm-package/bin/codex.bin | grep -Eq '(RUNPATH|RPATH).*\$ORIGIN'; then
      pass
    else
      fail
    fi
  else
    echo "✅ PRESENT (source configured; binary validation deferred until packaged Android ELFs are staged)"
  fi
else
  fail
fi

# Patch #11 (Android Realtime Audio + oboe-shared-stdcxx) RETIRED at
# rust-v0.140.0-alpha.18: upstream removed the entire TUI realtime voice feature
# (openai/codex#27801), so the fork's cpal/oboe Android enablement toggle had
# nothing left to gate and was dropped with it. The feature was never usable from
# the Termux CLI anyway (needs an Android JavaVM/Activity). Termux-native audio is
# tracked on the Codex VL roadmap.

printf "Patch #12 (Dynamic Subcommand Routing): "
if grep -q 'detectSubcommands' npm-package/bin/codex.js \
  && grep -q 'spawnSync(binaryPath' npm-package/bin/codex.js \
  && grep -Fq 'first && !isOption ? detectSubcommands() : null' npm-package/bin/codex.js \
  && grep -Fq 'process.kill(process.pid, signal)' npm-package/bin/codex.js \
  && grep -Fq 'process.kill(process.pid, signal)' npm-package/bin/codex-exec.js \
  && grep -Fq 'process.exit(code ?? 1)' npm-package/bin/codex.js \
  && grep -Fq 'process.exit(code ?? 1)' npm-package/bin/codex-exec.js; then
  pass
else
  fail
fi

printf "Patch #13 (Fork-safe Managed Updates): "
if grep -q "@mmmbuto/codex-cli-termux@latest" codex-rs/tui/src/update_action.rs \
  && grep -q "@mmmbuto/codex-cli-termux@latest" codex-rs/app-server-daemon/src/lib.rs \
  && grep -q "@mmmbuto/codex-cli-termux@latest" codex-rs/app-server-daemon/README.md \
  && grep -q "auto_update_enabled: false" codex-rs/app-server-daemon/src/lib.rs \
  && ! grep -R -q "chatgpt.com/codex/install" codex-rs/tui/src/update_action.rs codex-rs/app-server-daemon; then
  pass
else
  fail
fi

printf "Patch #14 (Fork-owned Public Install Surfaces): "
if grep -q "DioNanos/codex-termux" scripts/install/install.sh \
  && grep -q "DioNanos/codex-termux" scripts/install/install.ps1 \
  && grep -q "DioNanos/codex-termux" scripts/stage_npm_packages.py \
  && grep -q "@mmmbuto/codex-cli-termux" codex-rs/README.md \
  && ! grep -R -q "github.com/openai/codex/releases\\|api.github.com/repos/openai/codex\\|@openai/codex" scripts/install scripts/stage_npm_packages.py codex-rs/README.md; then
  pass
else
  fail
fi

printf "Patch #15 (Fork-owned Feedback Surfaces): "
if grep -q "DioNanos/codex-termux/issues/new" codex-rs/tui/src/bottom_pane/feedback_view.rs \
  && grep -q "DioNanos/codex-termux/main/announcement_tip.toml" codex-rs/tui/src/tooltips.rs \
  && grep -q "github.com/DioNanos/codex-termux/releases/latest" announcement_tip.toml \
  && grep -q "@mmmbuto/codex-cli-termux" .github/ISSUE_TEMPLATE/3-cli.yml \
  && grep -q "DioNanos/codex-termux/discussions" .github/ISSUE_TEMPLATE/4-bug-report.yml \
  && grep -q "DioNanos/codex-termux/blob/main/docs/contributing.md" .github/ISSUE_TEMPLATE/5-feature-request.yml \
  && grep -q "DioNanos/codex-termux/blob/main/docs/contributing.md" .github/pull_request_template.md \
  && ! grep -R -q "github.com/openai/codex/issues\\|github.com/openai/codex/discussions\\|github.com/openai/codex/releases/latest\\|npmjs.com/package/@openai/codex\\|raw.githubusercontent.com/openai/codex/main/announcement_tip.toml" announcement_tip.toml .github/ISSUE_TEMPLATE .github/pull_request_template.md codex-rs/tui/src/bottom_pane/feedback_view.rs codex-rs/tui/src/tooltips.rs; then
  pass
else
  fail
fi

printf "Patch #16 (Android Remote-Control Daemon): "
if grep -q 'CODEX_SELF_EXE' codex-rs/app-server-daemon/src/managed_install.rs \
  && grep -q 'target_os = "android"' codex-rs/app-server-daemon/src/managed_install.rs \
  && grep -q '/proc/' codex-rs/app-server-daemon/src/backend/pid.rs \
  && grep -q 'target_os = "android"' codex-rs/app-server-daemon/src/backend/pid.rs \
  && grep -q 'temp_dir()' codex-rs/cli/src/remote_control_cmd.rs \
  && ! grep -q 'tempdir_in("/tmp")' codex-rs/cli/src/remote_control_cmd.rs; then
  pass
else
  fail
fi

printf "Patch #6b (Fork Identity Across UI/Doctor/NPM Surfaces): "
if grep -q "DioNanos/codex-termux/releases/latest" codex-rs/cli/src/doctor/updates.rs \
  && grep -q "@mmmbuto/codex-cli-termux" codex-rs/cli/src/doctor/updates.rs \
  && grep -q "@mmmbuto/codex-cli-termux" codex-rs/cli/src/doctor.rs \
  && grep -q "@mmmbuto%2fcodex-cli-termux" codex-rs/tui/src/npm_registry.rs \
  && grep -q "DioNanos/codex-termux" codex-rs/tui/src/update_prompt.rs \
  && grep -q "DioNanos/codex-termux" codex-rs/tui/src/history_cell/notices.rs \
  && grep -Fq '#[cfg(not(test))]' codex-rs/tui/src/version.rs \
  && grep -Fq '#[cfg(test)]' codex-rs/tui/src/version.rs \
  && grep -Fq 'pub const CODEX_CLI_VERSION: &str = "0.0.0";' codex-rs/tui/src/version.rs \
  && ! grep -R -q "api.github.com/repos/openai/codex\|@openai%2fcodex\|@openai/codex" \
      codex-rs/cli/src/doctor/updates.rs \
      codex-rs/cli/src/doctor.rs \
      codex-rs/tui/src/npm_registry.rs \
      codex-rs/tui/src/update_prompt.rs \
      codex-rs/tui/src/history_cell/notices.rs \
  && ! grep -R -q "formulae.brew.sh/api/cask/codex.json" \
      codex-rs/cli/src/doctor/updates.rs \
      codex-rs/tui/src/updates.rs; then
  pass
else
  fail
fi

printf "Patch #17 (flock ENOTSUP/EOPNOTSUPP Tolerance): "
if grep -q 'raw_os_error().*libc::ENOTSUP' codex-rs/app-server-daemon/src/backend/pid.rs \
  && grep -q 'raw_os_error().*libc::EOPNOTSUPP' codex-rs/app-server-daemon/src/backend/pid.rs \
  && grep -q 'raw_os_error().*libc::ENOTSUP' codex-rs/app-server-daemon/src/lib.rs \
  && grep -q 'raw_os_error().*libc::EOPNOTSUPP' codex-rs/app-server-daemon/src/lib.rs; then
  pass
else
  fail
fi

printf "Patch #18 (Android Runtime Compat Shims): "
if grep -q 'CODEX_SELF_EXE' codex-rs/arg0/src/lib.rs \
  && grep -q 'resolve_codex_self_exe' codex-rs/arg0/src/lib.rs \
  && grep -q 'is_unsupported_file_lock_error' codex-rs/core/src/installation_id.rs \
  && grep -q 'ErrorKind::Unsupported' codex-rs/core/src/installation_id.rs \
  && grep -q 'pub unsafe extern "C" fn openpty' codex-rs/utils/pty/src/pty.rs \
  && grep -q 'target_os = "android"' codex-rs/utils/pty/src/pty.rs; then
  pass
else
  fail
fi

printf "Patch #18a (All 8 Cross-Fork ENOTSUP Lock Guards): "
if grep -q '&& !is_unsupported_file_lock_error(&err)' codex-rs/app-server-transport/src/transport/unix_socket.rs \
  && grep -q '&& !is_unsupported_file_lock_error(&err)' codex-rs/core/src/installation_id.rs \
  && [ "$(grep -c 'if is_unsupported_file_lock_error(e)' codex-rs/message-history/src/lib.rs)" -eq 2 ] \
  && grep -q 'if is_unsupported_file_lock_error(error)' codex-rs/message-history/src/batch.rs \
  && grep -q 'if !is_unsupported_file_lock_error(&io_err)' codex-rs/arg0/src/lib.rs \
  && grep -q 'if is_unsupported_file_lock_error(&io_err)' codex-rs/arg0/src/lib.rs \
  && grep -q '&& !is_unsupported_file_lock_error(&source)' codex-rs/execpolicy/src/amend.rs; then
  pass
else
  fail
fi

printf "Patch #18b (Termux MCP Environment Propagation): "
termux_env_block="$(awk '
  index($0, "pub(crate) const TERMUX_ENV_VARS: &[&str] = &[") { capture = 1 }
  capture { print }
  capture && /^];$/ { exit }
' codex-rs/rmcp-client/src/utils.rs)"
termux_env_count="$(printf '%s\n' "$termux_env_block" | grep -c '^    "[A-Z_]*",$' || true)"
termux_env_complete=1
for termux_var in \
  PREFIX TERMUX_VERSION TERMUX_APP_PID TERMUX_MAIN_PACKAGE_FORMAT \
  LD_PRELOAD LD_LIBRARY_PATH NPM_CONFIG_PREFIX \
  ANDROID_DATA ANDROID_ROOT ANDROID_RUNTIME_ROOT BOOTCLASSPATH \
  XDG_RUNTIME_DIR XDG_DATA_HOME XDG_CACHE_HOME XDG_CONFIG_HOME; do
  if [[ "$termux_env_block" != *"\"${termux_var}\""* ]]; then
    termux_env_complete=0
  fi
done
if [ "$termux_env_count" -eq 15 ] \
  && [ "$termux_env_complete" -eq 1 ] \
  && grep -q 'env::var_os("TERMUX_VERSION").is_some()' codex-rs/rmcp-client/src/utils.rs \
  && grep -q 'chain(termux_env_vars.iter().copied())' codex-rs/rmcp-client/src/utils.rs; then
  pass
else
  fail
fi

printf "Patch #19 (Android UI cfg Gates): "
# The app_event.rs android cfg gate was attached to the realtime audio event
# retired with upstream openai/codex#27801 (see Patch #11 note); the clipboard
# paste android gate is the remaining fork UI cfg surface.
if grep -q 'cfg(not(target_os = "android"))' codex-rs/tui/src/clipboard_paste.rs; then
  pass
else
  fail
fi

printf "Patch #20 (Android Code-Mode real, upstream-aligned): "
# 0.136.0: the Android code-mode stub was reverted. code-mode now uses the real
# in-process V8 runtime on Android too (stub files removed, v8 enabled for all
# targets), relying on the fork-owned aarch64-linux-android rusty_v8 prebuild.
#
# rust-v0.147.0 moved the V8 runtime out of code-mode into the new
# code-mode-runtime crate, so the anchors moved with it. The property asserted is
# unchanged: no stubs anywhere, no target_os gate, and v8 pulled in
# unconditionally by whichever crate owns the runtime.
if [ ! -f codex-rs/code-mode/src/runtime_stub.rs ] \
  && [ ! -f codex-rs/code-mode/src/service_stub.rs ] \
  && ! grep -rq 'runtime_stub\|service_stub' codex-rs/code-mode/src codex-rs/code-mode-runtime/src \
  && ! grep -q 'target_os = "android"' codex-rs/code-mode/Cargo.toml \
  && ! grep -q 'target_os = "android"' codex-rs/code-mode-runtime/Cargo.toml \
  && grep -q '^v8 = { workspace = true' codex-rs/code-mode-runtime/Cargo.toml \
  && grep -q 'codex-code-mode-runtime = { workspace = true }' codex-rs/code-mode-host/Cargo.toml; then
  pass
else
  fail
fi

printf "Patch #21 (Android Vendored OpenSSL): "
if grep -q 'aarch64-linux-android' codex-rs/core/Cargo.toml \
  && grep -q '"vendored"' codex-rs/core/Cargo.toml; then
  pass
else
  fail
fi

printf "Patch #22 (V8 Android Prebuilt Infrastructure): "
# Read the version the workflow actually defaults to instead of naming one. A
# literal here goes stale at every v8 bump and turns this into a check on the
# calendar rather than on the infrastructure.
#
# The read is confined to the `v8_version` stanza and stops at the next input at
# the same indentation. Scanning forward from the stanza would otherwise accept
# a neighbouring input's default -- a workflow whose v8_version carries no
# default at all would then be judged by an unrelated one, and a non-empty
# result is not evidence that it came from the right place. Exactly one default
# inside the stanza, or nothing.
v8_workflow_version="$(awk '
  /^      v8_version:[[:space:]]*$/ { in_stanza = 1; next }
  in_stanza && /^      [^[:space:]]/ { in_stanza = 0 }
  in_stanza && /^        default:/ { value = $2; gsub(/[^0-9.]/, "", value); found++ }
  END { if (found == 1) print value }
' .github/workflows/rusty-v8-android-release.yml 2>/dev/null)"
# Ask the manifest for the sandbox profile *of that version*, by full header. A
# substring search for the profile name passes on any leftover entry, so a bump
# to a version published plain-only would keep an old sandbox record vouching
# for it -- which is the exact substitution this release exists to prevent.
v8_sandbox_pin="[versions.\"${v8_workflow_version}\".targets.\"aarch64-linux-android\".profiles.ptrcomp_sandbox_release]"
if [ -f scripts/fetch_rusty_v8_android.py ] \
  && [ -f scripts/prepare_rusty_v8_android_source.py ] \
  && [ -f .github/workflows/rusty-v8-android-release.yml ] \
  && [ -f third_party/v8/android-artifacts.toml ] \
  && grep -q 'aarch64-linux-android' third_party/v8/android-artifacts.toml \
  && [ -n "${v8_workflow_version}" ] \
  && grep -Fq "[versions.\"${v8_workflow_version}\"]" third_party/v8/android-artifacts.toml \
  && grep -Fq "${v8_sandbox_pin}" third_party/v8/android-artifacts.toml \
  && grep -q 'RUSTY_V8_ARCHIVE' scripts/fetch_rusty_v8_android.py \
  && grep -q 'missing pinned rusty_v8 Android manifest entry' scripts/fetch_rusty_v8_android.py \
  && grep -q 'invalid lowercase SHA-256' scripts/fetch_rusty_v8_android.py \
  && grep -q 'urlopen(url, timeout=120)' scripts/fetch_rusty_v8_android.py \
  && grep -q 'shlex.quote' scripts/fetch_rusty_v8_android.py \
  && grep -Fq 'DEFAULT_PROFILE = "ptrcomp_sandbox_release"' scripts/fetch_rusty_v8_android.py \
  && [ -f scripts/check_v8_sandbox.py ] \
  && grep -Fq 'v8__V8__IsSandboxEnabled' scripts/check_v8_sandbox.py \
  && grep -Fq 'scripts/check_v8_sandbox.py' .github/workflows/termux-npm-build-publish.yml \
  && grep -Fq 'scripts/check_v8_sandbox.py' .github/workflows/rusty-v8-android-release.yml \
  && grep -Fq 'CODEX_ALLOW_SUPERSEDED_V8_BUILDER' scripts/build_rusty_v8_android.sh; then
  pass
else
  fail
fi

printf "Patch #23 (Fork-Owned Workflows + CI Guards): "
if grep -q "github.repository == 'openai/codex'" .github/workflows/repo-checks.yml \
  && [ -f .github/workflows/termux-npm-build-publish.yml ] \
  && [ -f .github/workflows/rusty-v8-android-release.yml ] \
  && grep -q 'actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'actions/upload-artifact@bbbca2ddaa5d8feaa63e36b76fdaad77386f024f' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'persist-credentials: false' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'contents: read' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'git rev-parse HEAD.*GITHUB_SHA' .github/workflows/termux-npm-build-publish.yml \
  && ! grep -q 'source_ref\|create_release\|gh release' .github/workflows/termux-npm-build-publish.yml \
  && ! grep -Eq '^[[:space:]]*npm publish' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd' .github/workflows/rusty-v8-android-release.yml \
  && grep -q 'actions/setup-python@a309ff8b426b58ec0e2a45f0f869d46889d02405' .github/workflows/rusty-v8-android-release.yml \
  && grep -q 'dtolnay/rust-toolchain@e081816240890017053eacbb1bdf337761dc5582' .github/workflows/rusty-v8-android-release.yml \
  && grep -q 'actions/upload-artifact@bbbca2ddaa5d8feaa63e36b76fdaad77386f024f' .github/workflows/rusty-v8-android-release.yml \
  && grep -q 'persist-credentials: false' .github/workflows/rusty-v8-android-release.yml \
  && grep -q 'contents: read' .github/workflows/rusty-v8-android-release.yml \
  && ! grep -q 'publish_release\|release_repository\|GH_TOKEN\|gh release' .github/workflows/rusty-v8-android-release.yml \
  && fork_internal_surfaces_match_contract; then
  pass
else
  fail
fi

printf "Patch #24 (Termux TLS roots, RETIRED at rust-v0.147.0 — guard inverted): "
# The trend the earlier note tracked reached its end. Upstream moved the OAuth and
# streamable-HTTP MCP paths onto the injected reqwest-0.12 adapters one at a time,
# and rust-v0.147.0 removed the last reqwest-0.13 client this crate built itself
# (discover_streamable_http_oauth_with_headers). Discovery now runs on the injected
# codex-http-client, which never constructs rustls-platform-verifier, so the Android
# panic is unreachable here; apply_termux_tls had nothing left to wrap and reqwest is
# no longer a dependency of this crate, so it was removed with it.
#
# So this no longer checks that the guard is present — it checks the condition that
# makes its absence safe. If rmcp-client ever takes a direct reqwest dependency again,
# a client that can hit the Android panic is back and the guard must come back with it.
if ! grep -qE '^[[:space:]]*reqwest[[:space:]]*=' codex-rs/rmcp-client/Cargo.toml; then
  pass
elif grep -q "apply_termux_tls" codex-rs/rmcp-client/src/utils.rs \
  && grep -q "tls_certs_only" codex-rs/rmcp-client/src/utils.rs \
  && grep -q "webpki-root-certs" codex-rs/rmcp-client/Cargo.toml; then
  pass
else
  fail
fi

printf "Patch #25 (Plugin Hook Description Compatibility): "
if grep -q 'serde(deny_unknown_fields)' codex-rs/config/src/hook_config.rs \
  && grep -q 'pub description: Option<String>' codex-rs/config/src/hook_config.rs \
  && grep -q 'hooks_file_accepts_top_level_description' codex-rs/config/src/hooks_tests.rs \
  && grep -q 'unknown fields other than description must be rejected' codex-rs/config/src/hooks_tests.rs; then
  pass
else
  fail
fi

printf "Patch #26 (Model Catalog Instruction Fallback): "
# rust-v0.147.0: upstream added its own legacy `base_instructions` layer
# (`ModelInfoWithLegacyBaseInstructions*`), which flattens `ModelInfo` and declares
# a `base_instructions` key of its own. With the fork field also serialized the
# catalog JSON carried the key twice and stopped parsing, so the field moved to
# `#[serde(skip)]`. This check used to require `#[serde(default)]` — the pre-0.147
# form — which means it went red on the correct code and would have gone green on
# the shape that reintroduces the duplicate-field bug. It now asserts the two
# halves the patch actually rests on: the field stays out of serde, and
# `get_model_instructions()` still falls back to it instead of returning an empty
# string.
#
# The fallback half is anchored to a behavioural test rather than to the source
# text: `get_model_instructions_falls_back_to_base_instructions_when_template_missing`
# gives the field a value and asserts it comes back, so an else branch quietly
# emptied out fails the gate even if the string this check greps for survives
# somewhere else in the file.
if grep -B1 -F 'pub base_instructions: String' codex-rs/protocol/src/openai_models.rs | grep -Fq '#[serde(skip)]' \
  && grep -q 'self.base_instructions.clone()' codex-rs/protocol/src/openai_models.rs \
  && grep -q 'fn get_model_instructions_falls_back_to_base_instructions_when_template_missing' codex-rs/protocol/src/openai_models.rs \
  && ! grep -q 'returning empty instructions' codex-rs/protocol/src/openai_models.rs \
  && grep -q 'ensure_catalog_instructions(&mut model)' codex-rs/models-manager/src/model_info.rs \
  && grep -q 'model catalog omitted usable instructions' codex-rs/models-manager/src/model_info.rs \
  && grep -q 'missing_catalog_instructions_use_builtin_fallback' codex-rs/models-manager/src/model_info_tests.rs \
  && grep -q 'catalog_template_allows_missing_base_instructions' codex-rs/models-manager/src/model_info_tests.rs \
  && grep -q 'explicit_empty_base_instruction_override_is_preserved' codex-rs/models-manager/src/model_info_tests.rs; then
  pass
else
  fail
fi

printf "Patch #27 (Pairing Names The Daemon Command): "
if grep -q 'fn ensure_pairing_daemon_socket' codex-rs/app-server-daemon/src/lib.rs \
  && grep -q 'ensure_pairing_daemon_socket(&daemon.socket_path)?' codex-rs/app-server-daemon/src/lib.rs \
  && grep -q 'run `remote-control start` first' codex-rs/app-server-daemon/src/lib.rs \
  && grep -q 'pairing_without_a_running_daemon_says_how_to_start_one' codex-rs/app-server-daemon/src/lib.rs; then
  pass
else
  fail
fi

printf "Patch #28 (musl ripgrep in the musl aarch64 payload): "
# Version-agnostic on purpose: anchoring this to a ripgrep version would go stale at the
# next bump and quietly stop guarding. What must never come back is the gnu artifact in a
# manifest whose consumers are musl targets — a merge that takes upstream's file restores
# it with no compilation signal, and rg then fails at runtime on the device.
if ! grep -q 'aarch64-unknown-linux-gnu' scripts/codex_package/rg \
  && [ "$(grep -c 'aarch64-unknown-linux-musl' scripts/codex_package/rg)" -ge 2 ]; then
  pass
else
  fail
fi

printf "Patch #29 (advisory locks degrade where the filesystem lacks them): "
# Termux storage under /data/data/com.termux/files does not implement advisory
# file locks: File::lock and File::try_lock return ErrorKind::Unsupported there.
# The fork's rule is that a missing lock degrades to running unlocked, never to
# a fatal error — upstream added the thread writer lock in 0.147.0 and it was the
# one lock site the rule had never been extended to, which took the whole CLI
# down at startup on the device. Version-agnostic on purpose: this checks the
# behaviour is wired, not which upstream release introduced the call.
writer_lock=codex-rs/thread-store/src/local/writer_lock.rs
if [ -f "$writer_lock" ] \
  && grep -q 'fn is_unsupported_file_lock_error' "$writer_lock" \
  && grep -q 'ErrorKind::Unsupported' "$writer_lock" \
  && grep -q 'is_unsupported_file_lock_error(&err)' "$writer_lock" \
  && [ "$(grep -c 'is_unsupported_file_lock_error(&err)' "$writer_lock")" -ge 2 ] \
  && grep -q 'unsupported_kind_is_classified_as_unsupported_lock_error' "$writer_lock"; then
  pass
else
  fail
fi

printf "Patch #30 (code-mode host travels in the package): "
# Since rust-v0.147.0 upstream runs code mode out-of-process: the CLI spawns
# codex-code-mode-host next to itself and FAILS CLOSED when it is absent —
# "Code Mode is unavailable ... host executable was not found". Nothing about
# that is visible to the compiler or the test suite: the crate builds, every
# test passes, and code mode is simply dead on the device. The 0.147.x release
# shipped without it on both forks for exactly that reason. Anchored to the
# three places that must agree, not to a version.
# The post-pack anchor targets the `for required in ...` list itself, not the
# bare path: `npm-package/bin/codex-code-mode-host` on the copy/strip/chmod
# lines CONTAINS `package/bin/codex-code-mode-host`, so a generic grep stayed
# green with the host removed from the list the assertion actually iterates.
# The three declarations are necessary but NOT sufficient: `npm pack` omits a
# listed-but-absent file and still exits 0, so the build must also read the
# finished archive back. Without that last check the guard goes green on a
# tarball that has no host in it — verified by an independent negative test.
if grep -q 'p codex-code-mode-host' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'release/codex-code-mode-host npm-package/bin/codex-code-mode-host' .github/workflows/termux-npm-build-publish.yml \
  && node -p "require('./npm-package/package.json').files.includes('bin/codex-code-mode-host')" | grep -q true \
  && grep -qE '^ *for required in .*(^| )package/bin/codex-code-mode-host( |;)' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'packaged tarball is missing' .github/workflows/termux-npm-build-publish.yml; then
  pass
else
  fail
fi

printf "release version contract (Cargo/npm/notes/changelog): "
cargo_workspace_version="$(awk '
  /^\[workspace.package\]$/ { workspace_package = 1; next }
  workspace_package && /^version = "/ {
    gsub(/^version = "/, "")
    gsub(/"$/, "")
    print
    exit
  }
' codex-rs/Cargo.toml)"
npm_package_version="$(node -p "require('./npm-package/package.json').version")"
# The fork version is NOT always an upstream version: 0.147.1 is a fork-only patch
# release and upstream has no rust-v0.147.1. Requiring "rust-v<fork version>" here
# would force the package description to name an upstream release that does not
# exist. What must hold is that the upstream base named in the description is a tag
# this repository actually has.
upstream_base_tag="$(node -p "(require('./npm-package/package.json').description.match(/rust-v[0-9]+\\.[0-9]+\\.[0-9]+/)||[''])[0]")"
# Cross-checked against the release notes so the base is stated deliberately in
# two places rather than typed once into a description nobody re-reads.
notes_base_tag="$(grep -oE 'rust-v[0-9]+\.[0-9]+\.[0-9]+' ".release/v${npm_package_version}.md" 2>/dev/null | head -1)"
# The tag itself is only checkable where the clone actually has tags. CI checks
# out at depth 1 without them, and a guard that silently fails there is worse
# than one that says which mode it ran in.
if [ -n "$(git tag --list 'rust-v*' 2>/dev/null | head -1)" ]; then
  upstream_tags_available=yes
else
  upstream_tags_available=no
  printf "(no upstream tags in this checkout: base tag existence not verified) "
fi
if [ -n "$cargo_workspace_version" ] \
  && [ "$cargo_workspace_version" = "$npm_package_version" ] \
  && [ -f ".release/v${npm_package_version}.md" ] \
  && grep -q "^# \[${npm_package_version}\]" CHANGELOG.md \
  && [ -n "$upstream_base_tag" ] \
  && [ "$upstream_base_tag" = "$notes_base_tag" ] \
  && { [ "$upstream_tags_available" = "no" ] \
       || git rev-parse -q --verify "refs/tags/${upstream_base_tag}^{commit}" >/dev/null; } \
  && grep -q '@mmmbuto/codex-cli-termux@latest' README.md \
  && grep -q '@mmmbuto/codex-cli-termux@latest' docs/install.md \
  && grep -q '@mmmbuto/codex-cli-termux@latest' npm-package/README.md; then
  pass
else
  fail
fi

printf "Android API 29 package/support contract: "
if grep -q 'aarch64-linux-android29-clang' codex-rs/.cargo/config.toml \
  && grep -q 'aarch64-linux-android29-clang' .github/workflows/termux-npm-build-publish.yml \
  && grep -q 'aarch64-linux-android29-clang' BUILDING.md \
  && grep -q 'Android 10+ / API 29+' README.md \
  && grep -q 'Android 10+ / API 29+' docs/install.md \
  && grep -q 'Android 10+ / API 29+' npm-package/README.md \
  && [ "$(node -p "JSON.stringify(require('./npm-package/package.json').os)")" = '["android"]' ] \
  && [ "$(node -p "JSON.stringify(require('./npm-package/package.json').cpu)")" = '["arm64"]' ]; then
  pass
else
  fail
fi

printf "npm package LICENSE/NOTICE payload: "
if cmp -s LICENSE npm-package/LICENSE \
  && cmp -s NOTICE npm-package/NOTICE \
  && grep -q '"LICENSE"' npm-package/package.json \
  && grep -q '"NOTICE"' npm-package/package.json; then
  pass
else
  fail
fi

printf "Bazel patch/integration inventory present: "
# rust-v0.146.0 moves AWS-LC from the fork-specific crate patch to the upstream
# BCR/rules_rs integration. Keep the Windows linker patch, and assert the new
# explicit aws-lc dependency, additive build file, and injected repository.
if [ -f patches/windows-link.patch ] \
  && grep -q 'bazel_dep(name = "aws-lc"' MODULE.bazel \
  && grep -q '@rules_rs//3rd_party/aws-lc-sys:additive.BUILD.bazel' MODULE.bazel \
  && grep -q 'inject_repo(crate, "aws-lc")' MODULE.bazel; then
  pass
else
  fail
fi
