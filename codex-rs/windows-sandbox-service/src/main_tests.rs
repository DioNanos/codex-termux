use anyhow::Result;

use super::RunMode;
use super::parse_mode;

#[test]
fn defaults_to_service_mode() -> Result<()> {
    assert_eq!(parse_mode(std::iter::empty())?, RunMode::Service);
    Ok(())
}

#[test]
fn accepts_explicit_service_mode() -> Result<()> {
    let arguments = [String::from("--service")].into_iter();
    assert_eq!(parse_mode(arguments)?, RunMode::Service);
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn accepts_foreground_mode() -> Result<()> {
    let arguments = [String::from("--foreground")].into_iter();
    assert_eq!(parse_mode(arguments)?, RunMode::Foreground);
    Ok(())
}

#[test]
#[cfg(not(debug_assertions))]
fn rejects_foreground_mode_in_release_builds() {
    let arguments = [String::from("--foreground")].into_iter();
    assert!(parse_mode(arguments).is_err());
}

#[test]
fn rejects_unknown_arguments() {
    let arguments = [String::from("--unknown")].into_iter();
    assert!(parse_mode(arguments).is_err());
}

#[test]
fn rejects_multiple_arguments() {
    let arguments = [String::from("--service"), String::from("--foreground")].into_iter();
    assert!(parse_mode(arguments).is_err());
}
