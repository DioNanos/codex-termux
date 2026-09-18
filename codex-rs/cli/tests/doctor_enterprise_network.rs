#[cfg(target_os = "macos")]
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Context as _;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use codex_config::types::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_reports_cloud_filesystem_policy_and_rejects_invalid_requirements() -> Result<()> {
    for valid_requirements in [true, false] {
        let server = MockServer::start().await;
        let codex_home = TempDir::new()?;
        let workspace = TempDir::new()?;
        let workspace_key = serde_json::to_string(workspace.path())?;
        let private_path = workspace.path().join("private-doctor-control");
        let glob = format!("{}/**/*.doctor-secret", workspace.path().display());
        let requirements = if valid_requirements {
            format!("[permissions.filesystem]\ndeny_read = [{private_path:?}, {glob:?}]\n")
        } else {
            "[permissions.filesystem]\ndeny_read = false\n".to_string()
        };
        std::fs::write(
            codex_home.path().join("config.toml"),
            format!(
                r#"
cli_auth_credentials_store = "ephemeral"
chatgpt_base_url = "{}/backend-api"
model_provider = "local"
[model_providers.local]
name = "local"
base_url = "{}/v1"
wire_api = "responses"
[windows]
sandbox = "elevated"
[projects.{workspace_key}]
trust_level = "trusted"
"#,
                server.uri(),
                server.uri(),
            ),
        )?;
        // Cloud authentication must use the project selected by --cd.
        std::fs::create_dir(workspace.path().join(".codex"))?;
        std::fs::write(
            workspace.path().join(".codex/config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        )?;
        write_chatgpt_auth(
            codex_home.path(),
            ChatGptAuthFixture::new("doctor-test-token")
                .account_id("doctor-workspace")
                .chatgpt_account_id("doctor-workspace")
                .chatgpt_user_id("doctor-user")
                .plan_type("enterprise"),
            AuthCredentialsStoreMode::File,
        )?;
        Mock::given(method("GET"))
            .and(path("/backend-api/wham/config/bundle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "requirements_toml": {
                    "enterprise_managed": [{
                        "id": "doctor-policy",
                        "name": "Doctor policy fixture",
                        "contents": requirements,
                    }],
                },
            })))
            .expect(1)
            .mount(&server)
            .await;

        let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
            .current_dir(codex_home.path())
            .env("CODEX_HOME", codex_home.path())
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env_remove("CODEX_ACCESS_TOKEN")
            .env_remove("CODEX_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .arg("--cd")
            .arg(workspace.path())
            .args(["doctor", "--json"])
            .stdin(Stdio::null())
            .output()?;
        let report: Value = serde_json::from_slice(&output.stdout)?;
        if valid_requirements {
            let config = &report["checks"]["config.load"];
            let sandbox = &report["checks"]["sandbox.helpers"]["details"];
            assert_eq!(config["status"], "ok", "{config:#}");
            assert_eq!(
                config["details"]["cwd"],
                workspace.path().display().to_string()
            );
            insta::assert_snapshot!(
                serde_json::to_string_pretty(&json!({
                    "scope": config["details"]["configuration scope"],
                    "activeThreadOverrides": config["details"]["active thread overrides"],
                    "denyRules": sandbox["denied-read rules"],
                    "denyGlobs": sandbox["denied-read glob rules"],
                    "scanDepth": sandbox["glob scan max depth"],
                    "managedFilesystemSource": sandbox["managed filesystem source"],
                }))?,
                @r#"
                {
                  "scope": "invocation config, including cloud-managed policy",
                  "activeThreadOverrides": "not inspected",
                  "denyRules": "2",
                  "denyGlobs": "1",
                  "scanDepth": "unbounded",
                  "managedFilesystemSource": "cloud"
                }
                "#
            );
            let stdout = String::from_utf8(output.stdout)?;
            assert!(!stdout.contains("doctor-secret"));
            assert!(!stdout.contains("private-doctor-control"));
        } else {
            assert_eq!(report["checks"]["config.load"]["status"], "fail");
            assert!(report["checks"].get("sandbox.helpers").is_none());
        }
        server.verify().await;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_custom_ca_falls_back_to_system_roots() -> Result<()> {
    let server = MockServer::start().await;
    for (request_method, request_path) in [("HEAD", "/v1/responses"), ("GET", "/v1/models")] {
        Mock::given(method(request_method))
            .and(path(request_path))
            .respond_with(ResponseTemplate::new(200))
            .expect(2)
            .mount(&server)
            .await;
    }

    let codex_home = TempDir::new()?;
    let certificate = codex_home.path().join("invalid-ca.pem");
    std::fs::write(&certificate, "not a certificate")?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        format!(
            "model_provider = \"local\"\n[model_providers.local]\nname = \"local\"\nbase_url = \"{}/v1\"\nwire_api = \"responses\"\n",
            server.uri()
        ),
    )?;
    for sandbox in [None, Some("seatbelt")] {
        let mut command = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
        command
            .args(["doctor", "--json"])
            .env("CODEX_HOME", codex_home.path())
            .env("CODEX_CA_CERTIFICATE", &certificate)
            .stdin(Stdio::null());
        if let Some(sandbox) = sandbox {
            command
                .env("CODEX_SANDBOX", sandbox)
                .env("HTTP_PROXY", "http://127.0.0.1:1")
                .env("http_proxy", "http://127.0.0.1:1")
                .env("HTTPS_PROXY", "http://127.0.0.1:1")
                .env("https_proxy", "http://127.0.0.1:1")
                .env("NO_PROXY", "")
                .env("no_proxy", "");
        } else {
            command
                .env("NO_PROXY", "127.0.0.1,localhost")
                .env("no_proxy", "127.0.0.1,localhost");
        }
        let output = command
            .output()
            .context("failed to run the doctor with an invalid custom CA")?;
        let report: Value = serde_json::from_slice(&output.stdout)?;

        assert!(
            report["checks"]["network.provider_reachability"]["details"]["local API inference URL"]
                .as_str()
                .is_some_and(|detail| detail.ends_with("reachable (HTTP 200)"))
        );
    }
    server.verify().await;

    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn doctor_reports_macos_system_proxy_configuration_and_policy() -> Result<()> {
    let codex_home = TempDir::new()?;
    let report = doctor_report(codex_home.path())?;
    let details = &report["checks"]["network.env"]["details"];

    assert_eq!(details["respect system proxy"], json!("disabled"));
    assert!(matches!(
        details["system proxy"].as_str(),
        Some("automatic (PAC)" | "manual" | "direct" | "unavailable")
    ));

    std::fs::write(
        codex_home.path().join("config.toml"),
        "[features]\nrespect_system_proxy = true\n",
    )?;
    let report = doctor_report(codex_home.path())?;
    assert_eq!(
        report["checks"]["network.env"]["details"]["respect system proxy"],
        json!("enabled")
    );

    Ok(())
}

#[cfg(target_os = "macos")]
fn doctor_report(codex_home: &Path) -> Result<Value> {
    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .args(["doctor", "--json"])
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::null())
        .output()
        .context("failed to run the doctor")?;

    serde_json::from_slice(&output.stdout).context("doctor did not emit a valid json report")
}
