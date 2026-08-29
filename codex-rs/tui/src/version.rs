/// The current Codex CLI version as embedded at compile time.
#[cfg(not(test))]
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Keep unit-test snapshots independent from the release version in Cargo.toml.
#[cfg(test)]
pub const CODEX_CLI_VERSION: &str = "0.0.0";
