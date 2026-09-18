#![cfg(unix)]

use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

struct TestDaemon {
    home: TempDir,
    codex: PathBuf,
    unmanaged: Option<Child>,
}

impl TestDaemon {
    fn new() -> Result<Self> {
        let home = tempfile::Builder::new().tempdir_in("/tmp")?;
        let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
        let codex_source = std::fs::canonicalize(&codex)?;
        let target = if cfg!(target_os = "macos") {
            format!("{}-apple-darwin", std::env::consts::ARCH)
        } else {
            format!("{}-unknown-linux-musl", std::env::consts::ARCH)
        };
        let standalone = home.path().join("packages/standalone");
        let release_name = format!("0.0.0-{target}");
        let managed = standalone
            .join("releases")
            .join(&release_name)
            .join("bin/codex");
        std::fs::create_dir_all(managed.parent().context("managed bin parent")?)?;
        std::fs::hard_link(&codex_source, &managed)
            .or_else(|_| std::fs::copy(&codex_source, managed).map(|_| ()))?;
        std::fs::write(standalone.join("auto-update-version"), &release_name)?;
        std::os::unix::fs::symlink(
            PathBuf::from("releases").join(release_name),
            standalone.join("current"),
        )?;
        Ok(Self {
            home,
            codex,
            unmanaged: None,
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.codex);
        command.env("CODEX_HOME", self.home.path());
        command
    }

    fn lifecycle(&self, action: &str) -> Result<Value> {
        let output = self.lifecycle_raw(action)?;
        ensure!(
            output.status.success(),
            "daemon {action} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    /// The fork makes `daemon update` fail closed, so its outcome is only
    /// observable without `lifecycle`'s success assertion.
    fn lifecycle_raw(&self, action: &str) -> Result<Output> {
        let output = self
            .command()
            .args(["app-server", "daemon", action])
            .output()?;
        Ok(output)
    }

    fn pid(&self, name: &str) -> Result<u32> {
        let record = std::fs::read(self.home.path().join("app-server-daemon").join(name))
            .with_context(|| format!("failed to read {name}"))?;
        Ok(serde_json::from_slice::<Value>(&record)?["pid"]
            .as_u64()
            .context("pid missing")? as u32)
    }

    fn updater_pid_path(&self) -> PathBuf {
        self.home
            .path()
            .join("app-server-daemon/app-server-updater.pid")
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(mut child) = self.unmanaged.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.lifecycle("stop");
    }
}

/// The fork ships no self-updater: a managed start must bring the daemon up and
/// never spawn an updater process, including across a restart.
#[test]
fn managed_start_never_spawns_updater() -> Result<()> {
    let daemon = TestDaemon::new()?;
    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let server_pid = daemon.pid("app-server.pid")?;
    assert!(daemon.pid("app-server-updater.pid").is_err());
    assert!(!daemon.updater_pid_path().exists());

    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, server_pid);
    assert!(!daemon.updater_pid_path().exists());

    assert_eq!(daemon.lifecycle("restart")?["status"], "restarted");
    assert_ne!(daemon.pid("app-server.pid")?, server_pid);
    assert!(daemon.pid("app-server-updater.pid").is_err());
    assert!(!daemon.updater_pid_path().exists());
    Ok(())
}

/// Upstream's `daemon update` starts an updater; the fork instead refuses with
/// its npm channel message and must leave no updater record behind. Both setups
/// upstream covered are kept: a daemon-owned installation, and an app server the
/// daemon does not manage already listening.
#[test]
fn manual_update_is_rejected_by_fork_policy() -> Result<()> {
    let daemon = TestDaemon::new()?;
    std::fs::remove_file(
        daemon
            .home
            .path()
            .join("packages/standalone/auto-update-version"),
    )?;

    let output = daemon.lifecycle_raw("update")?;
    assert_fork_update_rejection(&output)?;
    assert!(daemon.pid("app-server.pid").is_err());
    assert!(daemon.pid("app-server-updater.pid").is_err());
    assert!(!daemon.updater_pid_path().exists());
    drop(daemon);

    let mut daemon = TestDaemon::new()?;
    daemon.unmanaged = Some(
        daemon
            .command()
            .args(["app-server", "--listen", "unix://"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !daemon
        .command()
        .args(["app-server", "daemon", "version"])
        .output()?
        .status
        .success()
    {
        ensure!(Instant::now() < deadline, "app server did not become ready");
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = daemon.lifecycle("start")?;
    assert_eq!(output["status"], "alreadyRunning");
    assert_eq!(output["backend"], Value::Null);

    let output = daemon.lifecycle_raw("update")?;
    assert_fork_update_rejection(&output)?;
    assert!(daemon.pid("app-server-updater.pid").is_err());
    assert!(!daemon.updater_pid_path().exists());
    Ok(())
}

fn assert_fork_update_rejection(output: &Output) -> Result<()> {
    ensure!(
        !output.status.success(),
        "daemon update must fail closed in the fork"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        stderr.contains("does not self-update"),
        "daemon update must explain that the fork does not self-update: {stderr}"
    );
    ensure!(
        stderr.contains("@mmmbuto/codex-cli-termux@latest"),
        "daemon update must name the fork npm channel: {stderr}"
    );
    Ok(())
}

#[test]
fn managed_start_succeeds_when_updater_record_is_invalid() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let state_dir = daemon.home.path().join("app-server-daemon");
    std::fs::create_dir_all(&state_dir)?;
    std::fs::write(state_dir.join("app-server-updater.pid"), "not a PID record")?;

    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let server_pid = daemon.pid("app-server.pid")?;
    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, server_pid);
    Ok(())
}
