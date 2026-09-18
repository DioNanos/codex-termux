//! Child environment admission excludes loader hooks, plugin search paths, and app credentials.

use pretty_assertions::assert_eq;

use super::child_environment;

#[test]
fn forwards_only_explicit_device_network_and_os_inputs() {
    let input = [
        ("SystemRoot", "C:\\Windows"),
        ("https_proxy", "http://proxy"),
        ("HOME", "/home/user"),
        ("LD_PRELOAD", "loader"),
        ("DYLD_INSERT_LIBRARIES", "loader"),
        ("PATH", "project"),
        ("GST_PLUGIN_PATH", "plugins"),
        ("GST_REGISTRY", "untrusted-registry"),
        ("GST_REGISTRY_FORK", "yes"),
        ("OPENAI_API_KEY", "secret"),
    ];
    assert_eq!(
        child_environment(
            input
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
        ),
        input[..3]
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .chain(crate::RUNTIME_ENVIRONMENT.map(|(key, value)| (key.into(), value.into())))
            .collect()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn cancelling_initialization_terminates_the_owned_helper() -> anyhow::Result<()> {
    let spawned = codex_utils_pty::spawn_pipe_process(
        std::path::Path::new("/bin/sleep"),
        &["30".to_owned()],
        std::path::Path::new("/"),
        &child_environment(std::iter::empty()),
        /*arg0*/ &None,
        &[],
    )
    .await?;
    drop(spawned.stderr_rx);
    let (_, unused_exit) = tokio::sync::oneshot::channel();
    let host = super::VoiceHost {
        process: spawned.session,
        output: crate::message_reader::MessageReader::new(spawned.stdout_rx),
        exit: unused_exit,
    };
    let mut initialization = Box::pin(host.initialize_runtime());
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(initialization.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(initialization);
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 1), spawned.exit_rx).await??;
    Ok(())
}
