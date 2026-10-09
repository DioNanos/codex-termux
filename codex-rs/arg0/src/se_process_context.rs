//! Keeps termux-exec's `execve()` hook from deadlocking forked children on Android.
//!
//! Without `TERMUX__SE_PROCESS_CONTEXT` in the environment, the hook reads the SELinux
//! process context with stdio (`fopen`) in the child, between `fork` and `exec`. If another
//! thread of the parent held the stdio lock at the time of the fork, the child waits for it
//! forever and keeps copies of the parent's descriptors, including the daemon control
//! socket. Exporting the variable before any thread exists removes that code path. The
//! problem is described in the public termux-exec report
//! <https://github.com/termux/termux-exec-package/issues/41>.

#[cfg(target_os = "android")]
const SE_PROCESS_CONTEXT_ENV_VAR: &str = "TERMUX__SE_PROCESS_CONTEXT";
#[cfg(target_os = "android")]
const SE_PROCESS_CONTEXT_FILE: &str = "/proc/self/attr/current";

/// Sets `TERMUX__SE_PROCESS_CONTEXT` from the kernel when it is missing or empty.
///
/// Must run before any thread or runtime is created, because it changes the environment.
#[cfg(target_os = "android")]
pub(crate) fn export_se_process_context() {
    let current = std::env::var_os(SE_PROCESS_CONTEXT_ENV_VAR);
    if current.is_some_and(|value| !value.is_empty()) {
        return;
    }
    let Ok(raw) = std::fs::read(SE_PROCESS_CONTEXT_FILE) else {
        return;
    };
    let Some(label) = se_process_context_from_proc(&raw) else {
        return;
    };
    // It is safe to call set_var() because our process is single-threaded at
    // this point in its execution.
    unsafe { std::env::set_var(SE_PROCESS_CONTEXT_ENV_VAR, label) };
}

/// Returns the context found in the contents of `/proc/self/attr/current`, or `None` when
/// the contents are not a valid Android SELinux process context. Trailing NUL and newline
/// bytes are not part of the context.
#[cfg(any(target_os = "android", test))]
fn se_process_context_from_proc(raw: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(raw).ok()?;
    let label = text.trim_end_matches(['\0', '\n']);
    is_valid_se_process_context(label).then_some(label)
}

/// Mirrors the check termux-exec applies to the variable:
/// `^u:r:[^\n :]+:s0(:c[0-9]+,c[0-9]+(,c[0-9]+,c[0-9]+)?)?$`.
#[cfg(any(target_os = "android", test))]
fn is_valid_se_process_context(label: &str) -> bool {
    let Some(rest) = label.strip_prefix("u:r:") else {
        return false;
    };
    let Some((domain, level)) = rest.split_once(':') else {
        return false;
    };
    if domain.is_empty() || domain.contains(['\n', ' ']) {
        return false;
    }
    let Some(categories) = level.strip_prefix("s0") else {
        return false;
    };
    if categories.is_empty() {
        return true;
    }
    let Some(categories) = categories.strip_prefix(':') else {
        return false;
    };
    let parts: Vec<&str> = categories.split(',').collect();
    matches!(parts.len(), 2 | 4) && parts.iter().all(|part| is_category(part))
}

/// `c` followed by one or more ASCII digits.
#[cfg(any(target_os = "android", test))]
fn is_category(part: &str) -> bool {
    part.strip_prefix('c')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
#[path = "se_process_context_tests.rs"]
mod tests;
