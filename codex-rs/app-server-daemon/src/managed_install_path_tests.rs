use pretty_assertions::assert_eq;

#[test]
fn discovers_package_and_legacy_installs() {
    let home = tempfile::TempDir::new().expect("home");
    let current = home.path().join("packages/standalone/current");
    let legacy = current.join(super::managed_codex_file_name());
    let expected = if cfg!(windows) {
        current.join("bin").join("codex.exe")
    } else {
        legacy.clone()
    };
    assert_eq!(super::managed_codex_bin(home.path()), expected);
    std::fs::create_dir_all(&current).expect("current directory");
    std::fs::write(&legacy, b"legacy").expect("legacy executable");
    assert_eq!(super::managed_codex_bin(home.path()), legacy);
    let packaged = current.join("bin").join(super::managed_codex_file_name());
    std::fs::create_dir(current.join("bin")).expect("bin directory");
    std::fs::write(&packaged, b"packaged").expect("packaged executable");
    assert_eq!(super::managed_codex_bin(home.path()), packaged);
}
