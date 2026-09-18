//! Stops sandbox work before removing its protections, then continues independent cleanup.

use std::path::Path;

use anyhow::Result;
use anyhow::anyhow;

use crate::setup::OFFLINE_USERNAME;
use crate::setup::ONLINE_USERNAME;

mod firewall;
mod principals;
mod processes;

/// Removes sandbox resources created for one authenticated packaged installation.
/// Keep a supplied home and its ancestors pinned until `clean_up_desktop` starts.
/// That callback removes user-owned desktop files while the sandbox accounts remain disabled.
pub fn clean_up_packaged_windows_sandbox(
    codex_home: Option<&Path>,
    clean_up_desktop: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let _setup_lock = crate::setup_mutex::acquire_sandbox_setup_lock(/*timeout_ms*/ 5_000)?;
    let mut errors = Vec::new();
    let mut users = principals::DisabledSandboxUsers::default();
    if let Err(error) = users.disable().and_then(|()| processes::stop(&users)) {
        errors.push(format!("{error:#}"));
        // No network protections have been removed, so failed preparation can restore these flags.
        if let Err(error) = users.restore() {
            errors.push(format!("{error:#}"));
        }
        return Err(anyhow!(errors.join("; ")));
    }

    if let Some(codex_home) = codex_home {
        for directory in [
            crate::setup::sandbox_dir(codex_home),
            crate::setup::sandbox_secrets_dir(codex_home),
            crate::setup::sandbox_bin_dir(codex_home),
        ] {
            match std::fs::remove_dir_all(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    errors.push(format!("remove {}: {error}", directory.display()));
                }
            }
        }
    }

    if let Err(error) = clean_up_desktop() {
        errors.push(format!("{error:#}"));
    }

    for result in [
        crate::wfp::remove_wfp_filters(),
        firewall::cleanup_firewall_rules(),
        principals::remove_sandbox_principal("CodexSandboxUsers"),
        crate::hide_users::unhide_sandbox_users(&[OFFLINE_USERNAME, ONLINE_USERNAME]),
        // Keep accounts disabled and setup locked until shared cleanup and account deletion finish.
        principals::remove_sandbox_principal(OFFLINE_USERNAME),
        principals::remove_sandbox_principal(ONLINE_USERNAME),
    ] {
        if let Err(error) = result {
            errors.push(format!("{error:#}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("; ")))
    }
}
