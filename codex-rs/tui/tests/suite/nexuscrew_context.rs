//! Real daemon and embedded MCP environment isolation across cell launches.

use super::focus_palette::PtyCodex;
use super::focus_palette::write_test_config;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::Message;

const MCP_PROBE: &str = r#"
import json, os, sys
receipt = sys.argv[1]
def identity():
    return {'session': os.environ.get('NEXUSCREW_MCP_SESSION'),
            'pane': os.environ.get('TMUX_PANE')}
for raw in sys.stdin:
    request = json.loads(raw)
    if 'id' not in request:
        continue
    method = request['method']
    if method == 'initialize':
        with open(receipt, 'a') as stream:
            stream.write(json.dumps(identity()) + '\n')
        result = {'protocolVersion': request['params']['protocolVersion'],
                  'capabilities': {'tools': {}},
                  'serverInfo': {'name': 'identity-probe', 'version': '0.0.0'}}
    elif method == 'tools/list':
        result = {'tools': [{'name': 'identity', 'description': 'Report this process context',
                            'inputSchema': {'type': 'object', 'properties': {}}}]}
    elif method == 'tools/call':
        result = {'content': [{'type': 'text', 'text': json.dumps(identity())}], 'isError': False}
    else:
        result = {}
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
"#;

fn configure_probe(home: &Path, workspace: &Path) -> Result<()> {
    write_test_config(home, workspace)?;
    let script = home.join("identity-probe.py");
    std::fs::write(&script, MCP_PROBE)?;
    let receipt = home.join("identity-callback.jsonl");
    let config = home.join("config.toml");
    let mut contents = std::fs::read_to_string(&config)?;
    contents.push_str(&format!(
        "\n[mcp_servers.identity]\ncommand = 'python3'\nargs = [{}, {}]\nenv_vars = ['NEXUSCREW_MCP_SESSION', 'TMUX_PANE', 'NEXUSCREW_IDENTITY_FD']\nstartup_timeout_sec = 10\n",
        serde_json::to_string(&script.display().to_string())?,
        serde_json::to_string(&receipt.display().to_string())?,
    ));
    std::fs::write(config, contents)?;
    Ok(())
}

async fn request(
    socket: &mut tokio_tungstenite::WebSocketStream<UnixStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
                .to_string()
                .into(),
        ))
        .await?;
    tokio::time::timeout(Duration::from_secs(/*secs*/ 20), async {
        while let Some(message) = socket.next().await {
            if let Message::Text(text) = message? {
                let value: Value = serde_json::from_str(&text)?;
                if value.get("id") == Some(&json!(id)) {
                    ensure!(value.get("error").is_none(), "{value}");
                    return Ok(value["result"].clone());
                }
            }
        }
        anyhow::bail!("daemon closed the probe connection")
    })
    .await?
}

async fn observe_daemon(socket_path: &Path) -> Result<Value> {
    let stream = UnixStream::connect(socket_path).await?;
    let (mut socket, _) = tokio_tungstenite::client_async("ws://localhost", stream).await?;
    request(&mut socket, /*id*/ 1, "initialize", json!({"clientInfo":{"name":"identity-probe", "version":"0.0.0"}, "capabilities":{"experimentalApi":true}})).await?;
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0", "method":"initialized"})
                .to_string()
                .into(),
        ))
        .await?;
    let thread = request(&mut socket, /*id*/ 2, "thread/start", json!({})).await?;
    let response = request(&mut socket, /*id*/ 3, "mcpServer/tool/call", json!({"threadId":thread["thread"]["id"], "server":"identity", "tool":"identity", "arguments":{}})).await?;
    let text = response["content"][0]["text"]
        .as_str()
        .context("identity result")?;
    Ok(serde_json::from_str(text)?)
}

fn callbacks(home: &Path) -> Result<Vec<Value>> {
    let text = std::fs::read_to_string(home.join("identity-callback.jsonl")).unwrap_or_default();
    text.lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

async fn identity_scenario(
    action: &str,
    warm: &str,
    caller: &str,
    fd_only: bool,
    concurrent: bool,
) -> Result<()> {
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let workspace = codex_utils_cargo_bin::repo_root()?;
    let home = tempfile::tempdir_in("/tmp")?;
    configure_probe(home.path(), &workspace)?;
    let session = app_test_support::create_fake_rollout(
        home.path(),
        "2025-01-02T10-00-00",
        "2025-01-02T10:00:00Z",
        "Identity fixture",
        Some("openai"),
        /*git_info*/ None,
    )?;
    let stderr = std::fs::File::create(home.path().join("daemon-stderr.log"))?;
    let mut daemon = tokio::process::Command::new(&codex)
        .args(["app-server", "--listen", "unix://"])
        .env("CODEX_HOME", home.path())
        .env("NEXUSCREW_MCP_SESSION", warm)
        .env("TMUX_PANE", "%warm")
        .env_remove("NEXUSCREW_IDENTITY_FD")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .kill_on_drop(true)
        .spawn()?;
    let path = codex_app_server_client::app_server_control_socket_path(home.path())?;
    tokio::time::timeout(Duration::from_secs(/*secs*/ 20), async {
        while UnixStream::connect(path.as_path()).await.is_err() {
            ensure!(
                daemon.try_wait()?.is_none(),
                "daemon exited: {}",
                std::fs::read_to_string(home.path().join("daemon-stderr.log"))?
            );
            tokio::time::sleep(Duration::from_millis(/*millis*/ 20)).await;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    let expected_warm = json!({"session":warm, "pane":"%warm"});
    assert_eq!(observe_daemon(path.as_path()).await?, expected_warm);
    let count = callbacks(home.path())?.len();
    let mut args = vec!["--no-alt-screen"];
    if action != "new" {
        args = vec![action, &session, "--no-alt-screen"];
    }
    let home_value = home.path().to_str().context("test home")?;
    let caller_value = if fd_only { "" } else { caller };
    let pane = if caller == "missing-pane" {
        ""
    } else {
        "%caller"
    };
    let mut env = vec![
        ("CODEX_HOME", home_value),
        ("NEXUSCREW_MCP_SESSION", caller_value),
        ("TMUX_PANE", pane),
    ];
    if fd_only {
        env.push(("NEXUSCREW_IDENTITY_FD", "3:4"));
    }
    let mut terminal = PtyCodex::start_cli_with_env(&workspace, tempfile::tempdir()?, &args, &env)?;
    let mut other = if concurrent {
        Some(PtyCodex::start_cli_with_env(
            &workspace,
            tempfile::tempdir()?,
            &args,
            &[
                ("CODEX_HOME", home_value),
                ("NEXUSCREW_MCP_SESSION", "cell-c"),
                ("TMUX_PANE", "%other"),
            ],
        )?)
    } else {
        None
    };
    terminal.wait_for_startup()?;
    if let Some(other) = other.as_mut() {
        other.wait_for_startup()?;
    }
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 15);
    let expected = json!({"session":caller_value, "pane":pane});
    loop {
        terminal.read_output(Duration::from_millis(/*millis*/ 20))?;
        let values = callbacks(home.path())?;
        let fresh = &values[count..];
        if !fresh.is_empty() {
            assert!(
                fresh.iter().all(|identity| identity != &expected_warm),
                "{action}: MCP inherited the warm daemon instead of the caller: {fresh:?}"
            );
            if fresh.contains(&expected)
                && (!concurrent || fresh.contains(&json!({"session":"cell-c", "pane":"%other"})))
            {
                break;
            }
        }
        ensure!(
            Instant::now() < deadline,
            "missing cell identity callback; screen: {}",
            terminal.screen_contents()
        );
    }
    drop(other);
    drop(terminal);
    ensure!(
        daemon.try_wait()?.is_none(),
        "caller altered the existing daemon"
    );
    assert_eq!(observe_daemon(path.as_path()).await?, expected_warm);
    daemon.kill().await?;
    daemon.wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_cell_mcp_observes_caller_instead_of_warm_daemon() -> Result<()> {
    identity_scenario(
        "new", "cell-a", "cell-b", /*fd_only*/ false, /*concurrent*/ false,
    )
    .await?;
    identity_scenario(
        "new", "cell-b", "cell-a", /*fd_only*/ false, /*concurrent*/ false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resumed_cell_mcp_observes_caller_instead_of_warm_daemon() -> Result<()> {
    identity_scenario(
        "resume", "cell-a", "cell-b", /*fd_only*/ false, /*concurrent*/ false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forked_cell_mcp_observes_caller_instead_of_warm_daemon() -> Result<()> {
    identity_scenario(
        "fork", "cell-a", "cell-b", /*fd_only*/ false, /*concurrent*/ false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn declared_identity_channel_also_isolates_the_mcp_context() -> Result<()> {
    identity_scenario(
        "new", "cell-a", "", /*fd_only*/ true, /*concurrent*/ false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_cells_have_separate_mcp_callbacks() -> Result<()> {
    identity_scenario(
        "new", "cell-a", "cell-b", /*fd_only*/ false, /*concurrent*/ true,
    )
    .await
}

#[test]
fn cell_context_rejects_unbound_remote_and_agents() -> Result<()> {
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let home = tempfile::tempdir()?;
    let workspace = codex_utils_cargo_bin::repo_root()?;
    configure_probe(home.path(), &workspace)?;
    for args in [
        vec!["--remote", "unix:///tmp/unbound-codex-probe.sock"],
        vec!["agents"],
    ] {
        let output = std::process::Command::new(&codex)
            .args(args)
            .env("CODEX_HOME", home.path())
            .env("NEXUSCREW_MCP_SESSION", "cell-b")
            .env_remove("NEXUSCREW_IDENTITY_FD")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut child = output;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                anyhow::bail!("unbound launch did not reject within the test deadline");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output()?;
        assert!(!output.status.success());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("NexusCrew") && diagnostic.contains("identity"),
            "{diagnostic}"
        );
    }
    Ok(())
}

#[test]
fn cell_context_rejects_agents_before_daemon_start_with_pty() -> Result<()> {
    use std::os::fd::FromRawFd;
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let workspace = codex_utils_cargo_bin::repo_root()?;
    for args in [
        vec!["agents"],
        vec!["--remote", "unix:///tmp/unbound-codex-probe.sock"],
    ] {
        let home = tempfile::tempdir_in("/tmp")?;
        configure_probe(home.path(), &workspace)?;
        let path = codex_app_server_client::app_server_control_socket_path(home.path())?;
        assert!(!path.as_path().exists());
        std::fs::create_dir_all(path.as_path().parent().context("control socket parent")?)?;
        let listener = std::os::unix::net::UnixListener::bind(path.as_path())?;
        listener.set_nonblocking(true)?;
        let mut daemon_contacted = false;
        let mut master = -1;
        let mut slave = -1;
        // SAFETY: openpty writes two owned descriptors on success; unused optional arguments are null.
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        ensure!(result == 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: each descriptor was initialized by openpty and is owned exactly once.
        let _master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        let diagnostic_path = home.path().join("launch-stderr.log");
        let mut child = std::process::Command::new(&codex)
            .args(args)
            .env("CODEX_HOME", home.path())
            .env("TERM", "xterm-256color")
            .env("NEXUSCREW_MCP_SESSION", "cell-b")
            .env_remove("NEXUSCREW_IDENTITY_FD")
            .stdin(slave.try_clone()?)
            .stdout(slave)
            .stderr(std::fs::File::create(&diagnostic_path)?)
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Ok((connection, _)) = listener.accept() {
                daemon_contacted = true;
                drop(connection);
                // Stop a baseline or mutant at its first daemon contact, before any startup.
                let _ = child.kill();
                break Some(child.wait()?);
            }
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        daemon_contacted |= listener.accept().is_ok();
        drop(listener);
        std::fs::remove_file(path.as_path())?;
        assert!(
            !daemon_contacted,
            "unbound cell launch contacted shared daemon before rejection"
        );
        ensure!(
            status.is_some_and(|status| !status.success()),
            "unbound PTY launch did not reject"
        );
        let diagnostic = std::fs::read_to_string(diagnostic_path)?;
        assert!(
            diagnostic.contains("NexusCrew") && diagnostic.contains("identity"),
            "{diagnostic}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_cell_context_does_not_borrow_daemon_identity() -> Result<()> {
    identity_scenario(
        "new",
        "cell-a",
        "missing-pane",
        /*fd_only*/ false,
        /*concurrent*/ false,
    )
    .await
}

#[test]
fn cell_context_skips_shared_daemon_autostart() -> Result<()> {
    let workspace = codex_utils_cargo_bin::repo_root()?;
    let home = tempfile::tempdir_in("/tmp")?;
    configure_probe(home.path(), &workspace)?;
    let config = home.path().join("config.toml");
    let contents = std::fs::read_to_string(&config)?;
    std::fs::write(
        &config,
        contents.replace(
            "features.daemon_auto_start = false",
            "features.daemon_auto_start = true",
        ),
    )?;
    let socket = codex_app_server_client::app_server_control_socket_path(home.path())?;
    std::fs::create_dir_all(socket.as_path().parent().context("control socket parent")?)?;
    let listener = std::os::unix::net::UnixListener::bind(socket.as_path())?;
    listener.set_nonblocking(true)?;
    let home_value = home.path().to_str().context("test home")?;
    let mut terminal = PtyCodex::start_cli_with_env(
        &workspace,
        tempfile::tempdir()?,
        &["--no-alt-screen"],
        &[
            ("CODEX_HOME", home_value),
            ("NEXUSCREW_MCP_SESSION", "cell-b"),
            ("TMUX_PANE", "%caller"),
        ],
    )?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut daemon_contacted = false;
    let mut identity = None;
    loop {
        if let Ok((connection, _)) = listener.accept() {
            // Observe the first attempt, before any package lookup or real daemon startup.
            daemon_contacted = true;
            drop(connection);
            break;
        }
        if let Some(value) = callbacks(home.path())?.into_iter().next() {
            identity = Some(value);
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        terminal.read_output(Duration::from_millis(20))?;
        terminal.answer_startup_queries()?;
    }
    let diagnostic = terminal.screen_contents();
    // Drop kills and collects the child before evaluating either behavioral oracle.
    drop(terminal);
    daemon_contacted |= listener.accept().is_ok();
    drop(listener);
    std::fs::remove_file(socket.as_path())?;
    assert!(
        !daemon_contacted,
        "cell launch contacted shared daemon before embedded startup"
    );
    ensure!(
        identity.is_some(),
        "missing embedded identity callback: {diagnostic}"
    );
    assert_eq!(
        identity,
        Some(json!({"session":"cell-b", "pane":"%caller"}))
    );
    Ok(())
}
