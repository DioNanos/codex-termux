use anyhow::Result;
use app_test_support::DEFAULT_CLIENT_NAME;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn user_verification_methods_require_experimental_opt_in() -> Result<()> {
    let mut app_server = TestAppServer::builder().build().await?;
    app_server
        .initialize_with_capabilities(
            ClientInfo {
                name: DEFAULT_CLIENT_NAME.into(),
                title: None,
                version: "0.1.0".into(),
            },
            Some(InitializeCapabilities {
                experimental_api: false,
                ..Default::default()
            }),
        )
        .await?;

    for (method, params) in [
        ("userVerification/status", json!({})),
        ("userVerification/enroll", json!({})),
        ("userVerification/delete", json!({})),
        (
            "userVerification/verify",
            json!({"challenge": "AQ", "title": "Approve", "description": ""}),
        ),
    ] {
        let id = app_server.send_raw_request(method, Some(params)).await?;
        let response = app_server
            .read_stream_until_error_message(RequestId::Integer(id))
            .await?;
        assert_eq!(
            response.error,
            JSONRPCErrorError {
                code: -32600,
                message: format!("{method} requires experimentalApi capability"),
                data: None,
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn user_verification_without_provider_returns_typed_unavailability() -> Result<()> {
    let mut app_server = TestAppServer::builder().build_initialized().await?;
    let id = app_server
        .send_raw_request(
            "userVerification/verify",
            Some(json!({
                "challenge": "AQ", "title": "Approve", "description": ""
            })),
        )
        .await?;
    let response = app_server
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(
        response.error,
        JSONRPCErrorError {
            code: -32603,
            message: "User verification is not available in this build.".into(),
            data: Some(json!({"type": "unavailable", "reason": "providerUnavailable"})),
        }
    );
    Ok(())
}
