use anyhow::Result;
use codex_windows_sandbox_service::RunMode;

fn main() -> Result<()> {
    codex_windows_sandbox_service::run(parse_mode(std::env::args().skip(1))?)
}

fn parse_mode(arguments: impl Iterator<Item = String>) -> Result<RunMode> {
    let arguments = arguments.collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => Ok(RunMode::Service),
        [argument] if argument == "--service" => Ok(RunMode::Service),
        #[cfg(debug_assertions)]
        [argument] if argument == "--foreground" => Ok(RunMode::Foreground),
        [argument] => anyhow::bail!("unrecognized service argument {argument:?}"),
        _ => anyhow::bail!("expected at most one service argument"),
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
