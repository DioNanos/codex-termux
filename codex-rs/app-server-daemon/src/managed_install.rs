//! Resolves both package and legacy standalone layouts and compares installed executables.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use sha2::Digest;
use sha2::Sha256;
use tokio::fs;
use tokio::process::Command;

/// Returns the packaged executable when present, otherwise an existing legacy executable.
/// If neither exists, returns the expected packaged path on Windows and preserves the
/// historical legacy fallback on Unix. This is path selection, not existence validation:
/// launch operations reject a missing executable separately, while commands such as
/// stop can still run after the managed install has been removed.
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
    let current = codex_home
        .join("packages")
        .join("standalone")
        .join("current");
    let packaged = current.join("bin").join(managed_codex_file_name());
    let legacy = current.join(managed_codex_file_name());
    if packaged.is_file() || (cfg!(windows) && !legacy.is_file()) {
        packaged
    } else {
        legacy
    }
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
    let output = command.arg("--version").output().await.with_context(|| {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutableIdentity {
    digest: [u8; 32],
}

pub(crate) async fn executable_identity(executable: &Path) -> Result<ExecutableIdentity> {
    let bytes = fs::read(executable)
        .await
        .with_context(|| format!("failed to read executable {}", executable.display()))?;
    Ok(executable_identity_from_bytes(&bytes))
}

pub(crate) fn executable_identity_from_bytes(bytes: &[u8]) -> ExecutableIdentity {
    ExecutableIdentity {
        digest: Sha256::digest(bytes).into(),
    }
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
