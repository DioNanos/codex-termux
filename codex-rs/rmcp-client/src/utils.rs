use anyhow::Result;
use anyhow::anyhow;
use codex_config::types::McpServerEnvVar;
use reqwest::ClientBuilder;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use reqwest::header::USER_AGENT;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;

const MCP_USER_AGENT: &str = concat!("codex-mcp-client/", env!("CARGO_PKG_VERSION"));

pub(crate) fn create_env_for_mcp_server(
    extra_env: Option<HashMap<OsString, OsString>>,
    env_vars: &[McpServerEnvVar],
) -> Result<HashMap<OsString, OsString>> {
    let additional_env_vars = local_stdio_env_var_names(env_vars)?;
    let termux_env_vars: &[&str] = if running_on_termux() {
        TERMUX_ENV_VARS
    } else {
        &[]
    };
    let env = DEFAULT_ENV_VARS
        .iter()
        .copied()
        .chain(termux_env_vars.iter().copied())
        .chain(additional_env_vars)
        .filter_map(|var| env::var_os(var).map(|value| (OsString::from(var), value)))
        .chain(extra_env.unwrap_or_default())
        .collect();
    Ok(env)
}

/// codex-termux GitHub issue #10 fix — detect Termux at runtime via the
/// `TERMUX_VERSION` environment variable (set by Termux init scripts and
/// not present on ordinary Linux distributions). Runtime detection keeps the
/// behavior tied to the Termux environment and also covers older package
/// lines that did not use the current Android target triple.
fn running_on_termux() -> bool {
    env::var_os("TERMUX_VERSION").is_some()
}

/// codex-termux GitHub issue #11 fix — reqwest 0.13 (pulled in by the rmcp
/// 1.7.0 upgrade, upstream `910578792f`) verifies TLS through
/// `rustls-platform-verifier`, which on `target_os = "android"` requires an
/// initialized JVM `Context` and panics with `Expect rustls-platform-verifier
/// to be initialized` inside a plain Termux CLI process (no Activity, no
/// JNI). Supplying explicit roots makes reqwest build its
/// `WebPkiServerVerifier` branch instead, so the platform verifier is never
/// constructed. The embedded Mozilla roots match the trust model of the
/// reqwest 0.12 `rustls-tls` (webpki-roots) used elsewhere in the workspace.
///
/// Runtime-gated on `TERMUX_VERSION` like the issue #10 fix above — NOT
/// `cfg!(target_os = "android")` — so current and older Termux package lines
/// behave the same, while desktop builds keep the default platform verifier
/// untouched.
pub(crate) fn apply_termux_tls(builder: ClientBuilder) -> ClientBuilder {
    if !running_on_termux() {
        return builder;
    }
    builder.tls_certs_only(webpki_root_certificates())
}

fn webpki_root_certificates() -> Vec<reqwest::Certificate> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|der| reqwest::Certificate::from_der(der.as_ref()).ok())
        .collect()
}

pub(crate) fn create_env_overlay_for_remote_mcp_server(
    extra_env: Option<HashMap<OsString, OsString>>,
    env_vars: &[McpServerEnvVar],
) -> HashMap<OsString, OsString> {
    // Remote stdio should inherit PATH/HOME/etc. from the executor side, not
    // from the orchestrator process. Only forward variables explicitly named
    // by the MCP config plus literal env overrides from that config.
    env_vars
        .iter()
        .filter(|var| !var.is_remote_source())
        .filter_map(|var| env::var_os(var.name()).map(|value| (OsString::from(var.name()), value)))
        .chain(extra_env.unwrap_or_default())
        .collect()
}

pub(crate) fn remote_mcp_env_var_names(env_vars: &[McpServerEnvVar]) -> Vec<String> {
    env_vars
        .iter()
        .filter(|var| var.is_remote_source())
        .map(|var| var.name().to_string())
        .collect()
}

fn local_stdio_env_var_names(env_vars: &[McpServerEnvVar]) -> Result<impl Iterator<Item = &str>> {
    if let Some(remote_var) = env_vars.iter().find(|var| var.is_remote_source()) {
        return Err(anyhow!(
            "env_vars entry `{}` uses source `remote`, which requires remote MCP stdio",
            remote_var.name()
        ));
    }
    Ok(env_vars.iter().map(McpServerEnvVar::name))
}

pub(crate) fn build_default_headers(
    http_headers: Option<HashMap<String, String>>,
    env_http_headers: Option<HashMap<String, String>>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(MCP_USER_AGENT));

    if let Some(static_headers) = http_headers {
        for (name, value) in static_headers {
            let header_name = match HeaderName::from_bytes(name.as_bytes()) {
                Ok(name) => name,
                Err(err) => {
                    tracing::warn!("invalid HTTP header name `{name}`: {err}");
                    continue;
                }
            };
            let header_value = match HeaderValue::from_str(value.as_str()) {
                Ok(value) => value,
                Err(err) => {
                    tracing::warn!("invalid HTTP header value for `{name}`: {err}");
                    continue;
                }
            };
            headers.insert(header_name, header_value);
        }
    }

    if let Some(env_headers) = env_http_headers {
        for (name, env_var) in env_headers {
            if let Ok(value) = env::var(&env_var) {
                if value.trim().is_empty() {
                    continue;
                }

                let header_name = match HeaderName::from_bytes(name.as_bytes()) {
                    Ok(name) => name,
                    Err(err) => {
                        tracing::warn!("invalid HTTP header name `{name}`: {err}");
                        continue;
                    }
                };

                let header_value = match HeaderValue::from_str(value.as_str()) {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::warn!(
                            "invalid HTTP header value read from {env_var} for `{name}`: {err}"
                        );
                        continue;
                    }
                };
                headers.insert(header_name, header_value);
            }
        }
    }

    Ok(headers)
}

pub(crate) fn apply_default_headers(
    builder: ClientBuilder,
    default_headers: &HeaderMap,
) -> ClientBuilder {
    if default_headers.is_empty() {
        builder
    } else {
        builder.default_headers(default_headers.clone())
    }
}

#[cfg(unix)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] = &[
    "HOME",
    "LOGNAME",
    "PATH",
    "SHELL",
    "USER",
    "__CF_USER_TEXT_ENCODING",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "TZ",
];

/// Termux-specific environment variables required by tools spawned under
/// `/data/data/com.termux/files`. Without these, `npx`, `npm`, `pip`, and
/// most native helpers either fail to find their interpreters/caches
/// (`PREFIX`, `NPM_CONFIG_PREFIX`), crash because the dynamic linker
/// shim is missing (`LD_PRELOAD`), or load the wrong system libraries
/// (`LD_LIBRARY_PATH`). Only chained into the allowlist when
/// `running_on_termux()` returns true.
///
/// Fix for GitHub `DioNanos/codex-termux` issue #10 — `MCP client for
/// context7 failed to start: MCP startup failed: handshaking with MCP
/// server failed: connection closed: initialize response` when the MCP
/// server is configured via `npx`.
#[cfg(unix)]
pub(crate) const TERMUX_ENV_VARS: &[&str] = &[
    "PREFIX",
    "TERMUX_VERSION",
    "TERMUX_APP_PID",
    "TERMUX_MAIN_PACKAGE_FORMAT",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "NPM_CONFIG_PREFIX",
    "ANDROID_DATA",
    "ANDROID_ROOT",
    "ANDROID_RUNTIME_ROOT",
    "BOOTCLASSPATH",
    "XDG_RUNTIME_DIR",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
];

/// Windows builds of codex-termux never run under Termux (Termux is
/// Android/Linux with Bionic) so the allowlist is empty. Declared explicitly so
/// that `create_env_for_mcp_server` resolves the name on every target —
/// without this, the non-Unix typecheck of the function body would fail
/// with `unresolved name TERMUX_ENV_VARS` (caught in the 0.134.1
/// pre-build review, 2026-05-27).
#[cfg(not(unix))]
pub(crate) const TERMUX_ENV_VARS: &[&str] = &[];

#[cfg(windows)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] =
    codex_protocol::shell_environment::WINDOWS_CORE_ENV_VARS;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    use serial_test::serial;
    use std::ffi::OsStr;

    struct EnvVarGuard {
        key: String,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: impl AsRef<OsStr>) -> Self {
            let original = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value.as_ref());
            }
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                unsafe {
                    std::env::set_var(&self.key, value);
                }
            } else {
                unsafe {
                    std::env::remove_var(&self.key);
                }
            }
        }
    }

    #[tokio::test]
    async fn create_env_honors_overrides() {
        let value = "custom".to_string();
        let expected = OsString::from(&value);
        let env = create_env_for_mcp_server(
            Some(HashMap::from([(OsString::from("TZ"), expected.clone())])),
            &[],
        )
        .expect("local MCP env should build");
        assert_eq!(env.get(OsStr::new("TZ")), Some(&expected));
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_includes_additional_whitelisted_variables() {
        let custom_var = "EXTRA_RMCP_ENV";
        let value = "from-env";
        let expected = OsString::from(value);
        let _guard = EnvVarGuard::set(custom_var, value);
        let env = create_env_for_mcp_server(/*extra_env*/ None, &[custom_var.into()])
            .expect("local MCP env should build");
        assert_eq!(env.get(OsStr::new(custom_var)), Some(&expected));
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_remote_env_overlay_only_forwards_explicit_variables() {
        let default_var = DEFAULT_ENV_VARS[0];
        let custom_var = "EXTRA_REMOTE_RMCP_ENV";
        let custom_value = OsString::from("from-env");
        let _default_guard = EnvVarGuard::set(default_var, "from-default");
        let _custom_guard = EnvVarGuard::set(custom_var, &custom_value);

        let env =
            create_env_overlay_for_remote_mcp_server(/*extra_env*/ None, &[custom_var.into()]);

        assert_eq!(
            env,
            HashMap::from([(OsString::from(custom_var), custom_value)])
        );
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_remote_env_overlay_does_not_copy_remote_source_variables() {
        let remote_var = "REMOTE_ONLY_RMCP_ENV";
        let local_var = "LOCAL_RMCP_ENV";
        let local_value = OsString::from("from-local-env");
        let _remote_guard = EnvVarGuard::set(remote_var, "should-not-be-copied");
        let _local_guard = EnvVarGuard::set(local_var, &local_value);

        let env = create_env_overlay_for_remote_mcp_server(
            /*extra_env*/ None,
            &[
                McpServerEnvVar::Config {
                    name: remote_var.to_string(),
                    source: Some("remote".to_string()),
                },
                McpServerEnvVar::Config {
                    name: local_var.to_string(),
                    source: Some("local".to_string()),
                },
            ],
        );

        assert_eq!(
            env,
            HashMap::from([(OsString::from(local_var), local_value)])
        );
    }

    #[test]
    fn remote_mcp_env_var_names_returns_remote_source_names() {
        let names = remote_mcp_env_var_names(&[
            "LEGACY".into(),
            McpServerEnvVar::Config {
                name: "LOCAL".to_string(),
                source: Some("local".to_string()),
            },
            McpServerEnvVar::Config {
                name: "REMOTE".to_string(),
                source: Some("remote".to_string()),
            },
        ]);

        assert_eq!(names, vec!["REMOTE".to_string()]);
    }

    #[test]
    fn create_local_env_rejects_remote_source_variables() {
        let err = create_env_for_mcp_server(
            /*extra_env*/ None,
            &[McpServerEnvVar::Config {
                name: "REMOTE".to_string(),
                source: Some("remote".to_string()),
            }],
        )
        .expect_err("remote source should require remote stdio");

        assert!(
            err.to_string().contains("requires remote MCP stdio"),
            "unexpected error: {err}"
        );
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_propagates_termux_vars_when_termux_version_is_set() {
        // GitHub DioNanos/codex-termux issue #10 regression guard.
        let _termux_guard = EnvVarGuard::set("TERMUX_VERSION", "0.118.3");
        let prefix_value = OsString::from("/data/data/com.termux/files/usr");
        let _prefix_guard = EnvVarGuard::set("PREFIX", &prefix_value);

        let env =
            create_env_for_mcp_server(/*extra_env*/ None, &[]).expect("local MCP env should build");

        assert_eq!(env.get(OsStr::new("PREFIX")), Some(&prefix_value));
        assert_eq!(
            env.get(OsStr::new("TERMUX_VERSION")),
            Some(&OsString::from("0.118.3"))
        );
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_does_not_leak_termux_vars_off_termux() {
        // Without TERMUX_VERSION set, PREFIX from the host shell must NOT
        // accidentally propagate to spawned MCP servers (Linux desktops
        // often use PREFIX for unrelated build-tool conventions).
        let original_termux = std::env::var_os("TERMUX_VERSION");
        unsafe {
            std::env::remove_var("TERMUX_VERSION");
        }
        let _prefix_guard = EnvVarGuard::set("PREFIX", "/opt/leaked");

        let env =
            create_env_for_mcp_server(/*extra_env*/ None, &[]).expect("local MCP env should build");

        assert_eq!(env.get(OsStr::new("PREFIX")), None);

        if let Some(value) = original_termux {
            unsafe {
                std::env::set_var("TERMUX_VERSION", value);
            }
        }
    }

    #[test]
    fn webpki_root_certificates_all_parse() {
        // GitHub DioNanos/codex-termux issue #11 regression guard: every
        // embedded Mozilla root must convert into a reqwest::Certificate,
        // otherwise the Termux TLS trust store silently shrinks.
        let certs = webpki_root_certificates();
        assert!(!certs.is_empty());
        assert_eq!(certs.len(), webpki_root_certs::TLS_SERVER_ROOT_CERTS.len());
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn apply_termux_tls_builds_client_on_termux() {
        // GitHub DioNanos/codex-termux issue #11 regression guard: with the
        // Termux gate active the client must build with explicit roots (the
        // rustls-platform-verifier path would panic at first use on android).
        let _guard = EnvVarGuard::set("TERMUX_VERSION", "0.118.3");
        apply_termux_tls(ClientBuilder::new())
            .build()
            .expect("client with embedded webpki roots should build");
    }

    #[cfg(unix)]
    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_preserves_path_when_it_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;

        let raw_path = std::ffi::OsStr::from_bytes(b"/tmp/codex-\xFF/bin");
        let expected = raw_path.to_os_string();
        let _guard = EnvVarGuard::set("PATH", raw_path);

        let env =
            create_env_for_mcp_server(/*extra_env*/ None, &[]).expect("local MCP env should build");

        assert_eq!(env.get(OsStr::new("PATH")), Some(&expected));
    }
}
