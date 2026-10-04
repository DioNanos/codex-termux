//! Resolves both package and legacy standalone layouts and compares installed executables.

use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde::Deserialize;
use serde::Serialize;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;

/// New daemons own their packages, regardless of how the calling CLI was installed.
/// codex-termux fork (L2): the fork owns a dedicated packages namespace, so it
/// never reads or overwrites the packages the upstream daemon (or the other
/// fork) stages under `packages/app-server-daemon` or `packages/standalone`.
/// The first start on an existing CODEX_HOME sees this root empty and
/// installs the fork's own package from npm.
pub(crate) fn package_root(codex_home: &Path) -> PathBuf {
    codex_home.join("packages/app-server-daemon-termux")
}

/// Resolve both packaged and legacy binaries without requiring a valid install.
/// codex-termux fork (L1): a managed selection must carry the fork's own
/// codex-package.json manifest with variant "codex-termux". Upstream and the
/// other fork stage their binaries into the same CODEX_HOME; a selection
/// without our manifest (upstream install or legacy layout) or with a foreign
/// variant must never be executed by this daemon.
pub(crate) fn ensure_selected_variant_is_fork(selected: &Path) -> Result<()> {
    let Some(current) = selected.parent().and_then(Path::parent) else {
        anyhow::bail!("managed selection {selected:?} has no packages root");
    };
    let manifest_path = current.join("codex-package.json");
    let manifest_bytes = std::fs::read(&manifest_path).with_context(|| {
        format!(
            "the selected daemon binary at {selected:?} has no {} manifest: \
             reinstall codex-termux so the daemon executes the fork, not upstream",
            manifest_path.display()
        )
    })?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    let variant = manifest["variant"].as_str().unwrap_or_default();
    anyhow::ensure!(
        variant == "codex-termux",
        "the selected daemon package variant is {variant:?}: this daemon executes only \
         codex-termux; reinstall codex-termux"
    );
    Ok(())
}

pub(crate) fn managed_codex_bin(codex_home: &Path) -> PathBuf {
    // On Android/Termux the binary is installed via npm, not the standalone
    // installer. CODEX_SELF_EXE is set by the npm package launcher (Patch #10)
    // and points to the bundled ELF. The launcher keeps this path separate
    // from its shell wrapper so hidden arg0 aliases retain their special
    // argv[0]. The ELF has RUNPATH=$ORIGIN so libc++_shared.so resolves
    // correctly when the daemon starts it directly (Patch #10b).
    #[cfg(target_os = "android")]
    if let Ok(self_exe) = std::env::var("CODEX_SELF_EXE") {
        return PathBuf::from(self_exe);
    }
    let root = package_root(codex_home);
    let current = root.join("current");
    let packaged = current.join("bin").join(managed_codex_file_name());
    let legacy = current.join(managed_codex_file_name());
    if packaged.is_file()
        || !legacy.is_file() && (cfg!(windows) || root.ends_with("app-server-daemon-termux"))
    {
        packaged
    } else {
        legacy
    }
}

/// Only latest-channel stable releases may run the public latest-version updater.
pub(crate) fn is_stable_standalone_release(codex_home: &Path, codex_bin: &Path) -> bool {
    let standalone = package_root(codex_home);
    let Ok(releases) = std::fs::canonicalize(standalone.join("releases")) else {
        return false;
    };
    let Ok(release) = std::fs::canonicalize(standalone.join("current")) else {
        return false;
    };
    if release.parent() != Some(releases.as_path()) {
        return false;
    }
    let Some(release_name) = release.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    // GNU packages can seed the new directory; retain legacy updater eligibility.
    if standalone.ends_with("standalone") && release_name.ends_with("-gnu") {
        return false;
    }
    let targets = [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
        "aarch64-pc-windows-msvc",
        "x86_64-pc-windows-msvc",
    ];
    let Some(version) = targets
        .iter()
        .find_map(|target| release_name.strip_suffix(&format!("-{target}")))
    else {
        return false;
    };
    let components: Vec<_> = version.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
        && std::fs::read_to_string(standalone.join("auto-update-version"))
            .is_ok_and(|selected| selected == release_name)
        && std::fs::canonicalize(codex_bin).is_ok_and(|bin| bin.starts_with(&release))
}

/// Older managed binaries can serve app-server requests without owning an updater.
pub(crate) async fn supports_daemon_update_loop(codex_bin: &Path) -> bool {
    supports_daemon_command(codex_bin, &["pid-update-loop", "--help"]).await
}

/// Probe an internal daemon command without running a long-lived process.
pub(crate) async fn supports_daemon_command(codex_bin: &Path, args: &[&str]) -> bool {
    let mut command = Command::new(codex_bin);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    timeout(
        Duration::from_secs(5),
        command
            .args(["app-server", "daemon"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .is_ok_and(|result| result.is_ok_and(|status| status.success()))
}

pub(crate) async fn resolved_managed_codex_bin(codex_bin: &Path) -> Result<PathBuf> {
    fs::canonicalize(codex_bin).await.with_context(|| {
        format!(
            "failed to resolve managed Codex binary {}",
            codex_bin.display()
        )
    })
}

pub(crate) async fn managed_codex_version(codex_bin: &Path) -> Result<String> {
    let mut command = Command::new(codex_bin);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let output = command
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| {
            format!(
                "failed to invoke managed Codex binary {}",
                codex_bin.display()
            )
        })?;
    if !output.status.success() {
        return Err(anyhow!(
            "managed Codex binary {} exited with status {}",
            codex_bin.display(),
            output.status
        ));
    }

    let stdout = String::from_utf8(output.stdout).with_context(|| {
        format!(
            "managed Codex version was not utf-8: {}",
            codex_bin.display()
        )
    })?;
    parse_codex_version(&stdout)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExecutableIdentity {
    digest: [u8; 32],
}

pub(crate) async fn executable_identity(executable: &Path) -> Result<ExecutableIdentity> {
    let executable = executable.to_path_buf();
    // Debug executables can be hundreds of MB. Stream the digest off the async
    // runtime instead of allocating the whole file and blocking a runtime thread.
    tokio::task::spawn_blocking(move || {
        std::fs::File::open(&executable)
            .and_then(executable_identity_from_reader)
            .with_context(|| format!("failed to read executable {}", executable.display()))
    })
    .await
    .context("executable identity task failed")?
}

pub(crate) fn executable_identity_from_reader(
    reader: impl std::io::Read,
) -> std::io::Result<ExecutableIdentity> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(reader)?;
    Ok(ExecutableIdentity {
        digest: *hasher.finalize().as_bytes(),
    })
}

fn managed_codex_file_name() -> &'static str {
    if cfg!(windows) { "codex.exe" } else { "codex" }
}

fn parse_codex_version(output: &str) -> Result<String> {
    let version = output
        .split_whitespace()
        .nth(1)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| anyhow!("managed Codex version output was malformed"))?;
    Ok(version.to_string())
}

#[cfg(test)]
#[path = "managed_install_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "managed_install_path_tests.rs"]
mod path_tests;
