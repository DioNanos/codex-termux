// Keep the ACL probe independent of test-harness startup, which writes CODEX_HOME.
#[cfg(target_os = "windows")]
mod win;

#[cfg(target_os = "windows")]
fn main() {
    win::main();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("codex-windows-managed-deny-probe is Windows-only");
}
