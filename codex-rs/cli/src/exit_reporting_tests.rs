use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

const MARKER_ENV: &str = "CODEX_TERMINAL_REPORT_TEST_MARKER";
const MODE_ENV: &str = "CODEX_TERMINAL_REPORT_TEST_MODE";

fn record(stage: &str) {
    let path = std::env::var_os(MARKER_ENV).expect("child marker path");
    let mut marker = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .expect("open separate marker");
    writeln!(marker, "{stage}").expect("write separate marker");
}

fn await_terminal_disconnect() {
    if cfg!(panic = "abort") {
        record("panic-strategy-abort");
    }
    record("ready");
    std::io::stdin()
        .read_exact(&mut [0])
        .expect("parent handshake");
    let stdout_error = std::io::stdout().write_all(b"probe\n").unwrap_err();
    let stderr_error = std::io::stderr().write_all(b"probe\n").unwrap_err();
    assert_eq!(stdout_error.raw_os_error(), Some(libc::EIO));
    assert_eq!(stderr_error.raw_os_error(), Some(libc::EIO));
    record("disconnected-eio");
}

fn run_pty_child(name: &str, mode: &str) -> (std::process::ExitStatus, String) {
    let dir = tempfile::tempdir().expect("private marker directory");
    let marker = dir.path().join("reached");
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty initializes both descriptors; no optional output buffers are used.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(result, 0, "openpty: {}", std::io::Error::last_os_error());
    for fd in [master, slave] {
        // SAFETY: fd is an open descriptor owned by this fixture. Prevent the
        // child from inheriting the master and keeping its own PTY alive.
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    // SAFETY: successful openpty returned two owned, distinct descriptors.
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", name, "--ignored", "--nocapture"]);
    if cfg!(panic = "abort") {
        command.args(["-Z", "unstable-options", "--force-run-in-process"]);
    }
    let mut child = command
        .env(MARKER_ENV, &marker)
        .env(MODE_ENV, mode)
        .stdin(Stdio::piped())
        .stdout(slave.try_clone().expect("clone slave"))
        .stderr(slave)
        .spawn()
        .expect("spawn private PTY child");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !std::fs::read_to_string(&marker)
        .unwrap_or_default()
        .contains("ready")
    {
        if Instant::now() >= deadline || child.try_wait().expect("poll child").is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY child did not reach handshake");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(master);
    child
        .stdin
        .take()
        .expect("handshake pipe")
        .write_all(b"x")
        .expect("release child");
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY child did not finish");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let reached = std::fs::read_to_string(marker).expect("read separate marker");
    assert!(reached.contains("disconnected-eio"), "{reached}");
    if cfg!(panic = "abort") {
        assert!(reached.contains("panic-strategy-abort"), "{reached}");
    }
    (status, reached)
}

#[test]
fn fatal_exit_survives_disconnected_pty_and_keeps_failure_status() {
    let (status, reached) = run_pty_child("exit_reporting_tests::pty_exit_child", "fatal");
    assert!(reached.contains("exit-reached"), "{reached}");
    assert_eq!(status.code(), Some(1), "status={status}, marker={reached}");
    assert!(!reached.contains("exit-returned"), "{reached}");
}

#[test]
fn exit_messages_survive_disconnected_pty_and_return_io_error() {
    let (status, reached) = run_pty_child("exit_reporting_tests::pty_exit_child", "messages");
    assert!(reached.contains("exit-reached"), "{reached}");
    assert!(status.success(), "status={status}, marker={reached}");
    assert!(reached.contains("exit-returned-io-error"), "{reached}");
}

#[test]
#[ignore = "subprocess entry point; invoked only by the PTY regressions"]
fn pty_exit_child() {
    color_eyre::install().expect("install production error reporter");
    await_terminal_disconnect();
    let mut info = super::AppExitInfo::fatal("terminal task failed");
    info.token_usage = codex_tui::TokenUsage {
        total_tokens: 1,
        output_tokens: 1,
        ..Default::default()
    };
    let fatal = std::env::var(MODE_ENV).expect("child mode") == "fatal";
    if !fatal {
        info.exit_reason = super::ExitReason::UserRequested;
    }
    record("exit-reached");
    let result = super::handle_app_exit(info, /*cli_executable*/ None);
    record("exit-returned");
    let error = result.expect_err("nonfatal reporting must propagate the I/O failure");
    assert_eq!(
        error
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    record("exit-returned-io-error");
    std::process::exit(0);
}
