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
    eprintln!("PTY mode={mode}, status={status}, marker={reached}");
    (status, reached)
}

#[test]
fn restore_guard_survives_disconnected_pty() {
    for mode in ["explicit", "drop"] {
        let (status, reached) = run_pty_child("terminal_restore_tests::pty_cleanup_child", mode);
        assert!(reached.contains("restore-preflight-error:"), "{reached}");
        assert!(
            reached.contains("restore-preflight-stderr-eio"),
            "{reached}"
        );
        assert!(reached.contains("cleanup-reached"), "{reached}");
        assert!(
            status.success(),
            "mode={mode}, status={status}, marker={reached}"
        );
        assert!(reached.contains("cleanup-returned"), "{reached}");
    }
}

#[test]
#[ignore = "subprocess entry point; invoked only by the PTY regression"]
fn pty_cleanup_child() {
    color_eyre::install().expect("install production error reporter");
    await_terminal_disconnect();
    let mode = std::env::var(MODE_ENV).expect("child mode");
    if mode == "stderr-control" {
        record("stderr-control-reached");
        eprintln!("disconnected terminal reporting control");
        record("stderr-control-returned");
        std::process::exit(0);
    }
    if mode == "cursor-reset" || mode == "cursor-show" {
        let backend = ratatui::backend::CrosstermBackend::new(CursorWriter {
            fail_during_reset: mode == "cursor-reset",
        });
        let mut terminal =
            crate::custom_terminal::Terminal::with_screen_size_and_cursor_position_for_test(
                backend,
                ratatui::layout::Size {
                    width: 80,
                    height: 24,
                },
                ratatui::layout::Position { x: 0, y: 0 },
            );
        terminal.hidden_cursor = mode == "cursor-show";
        record("terminal-drop-reached");
        drop(terminal);
        record("terminal-drop-returned");
        std::process::exit(0);
    }
    let restore_error = crate::tui::restore_after_exit().expect_err("disconnected PTY restore");
    record(&format!(
        "restore-preflight-error:{:?}",
        restore_error.raw_os_error()
    ));
    let stderr_error = std::io::stderr()
        .lock()
        .write_all(b"before restore report\n")
        .expect_err("stderr remains disconnected before guard cleanup");
    assert_eq!(stderr_error.raw_os_error(), Some(libc::EIO));
    record("restore-preflight-stderr-eio");
    let mut guard = super::TerminalRestoreGuard::new();
    record("cleanup-reached");
    if std::env::var(MODE_ENV).expect("child mode") == "explicit" {
        guard.restore_silently();
        assert!(!guard.active);
    }
    drop(guard);
    record("cleanup-returned");
    std::process::exit(0);
}

struct CursorWriter {
    fail_during_reset: bool,
}

impl Write for CursorWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.fail_during_reset {
            record("reset-cursor-reached");
            let result = std::io::stderr().lock().write(bytes);
            if let Err(err) = &result {
                record(&format!("reset-cursor-error:{:?}", err.raw_os_error()));
            }
            result
        } else {
            Ok(bytes.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        record("show-cursor-reached");
        let result = std::io::stderr().lock().write_all(b"cursor flush\n");
        if let Err(err) = &result {
            record(&format!("show-cursor-error:{:?}", err.raw_os_error()));
        }
        result
    }
}

#[test]
fn terminal_drop_survives_failed_cursor_reset() {
    let (status, marker) =
        run_pty_child("terminal_restore_tests::pty_cleanup_child", "cursor-reset");
    assert!(marker.contains("terminal-drop-reached"), "{marker}");
    assert!(marker.contains("reset-cursor-reached"), "{marker}");
    assert!(
        marker.contains(&format!("reset-cursor-error:Some({})", libc::EIO)),
        "{marker}"
    );
    assert!(status.success(), "status={status}, marker={marker}");
    assert!(marker.contains("terminal-drop-returned"), "{marker}");
}

#[test]
fn terminal_drop_survives_failed_cursor_show() {
    let (status, marker) =
        run_pty_child("terminal_restore_tests::pty_cleanup_child", "cursor-show");
    assert!(marker.contains("terminal-drop-reached"), "{marker}");
    assert!(marker.contains("show-cursor-reached"), "{marker}");
    assert!(
        marker.contains(&format!("show-cursor-error:Some({})", libc::EIO)),
        "{marker}"
    );
    assert!(status.success(), "status={status}, marker={marker}");
    assert!(marker.contains("terminal-drop-returned"), "{marker}");
}

#[test]
fn disconnected_stderr_control_reaches_standard_reporting() {
    // This control must panic with both implementations: standard reporting
    // must reach the broken descriptor rather than a libtest capture buffer.
    let (status, marker) = run_pty_child(
        "terminal_restore_tests::pty_cleanup_child",
        "stderr-control",
    );
    assert!(marker.contains("stderr-control-reached"), "{marker}");
    assert!(!status.success(), "status={status}, marker={marker}");
    assert!(!marker.contains("stderr-control-returned"), "{marker}");
}
