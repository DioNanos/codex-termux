//! Exercise usage capability opt-in and lightweight polling through public JSON-RPC requests.

use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::ChatGptIdTokenClaims;
use app_test_support::TestAppServer;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::RateLimitResetCreditsSummary;
use codex_config::types::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use test_case::test_case;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[derive(Clone, Copy, PartialEq)]
enum AuthKind {
    Chatgpt,
    Fedramp,
    Pat,
}

#[test_case(AuthKind::Chatgpt, /*params*/ None, /*expect_header*/ false; "omitted_params")]
#[test_case(AuthKind::Chatgpt, Some(Value::Null), /*expect_header*/ false; "null_params")]
#[test_case(AuthKind::Chatgpt, Some(json!({})), /*expect_header*/ false; "default_capability")]
#[test_case(AuthKind::Chatgpt, Some(json!({"supportsLunaReserve": false})), /*expect_header*/ false; "disabled_capability")]
#[test_case(AuthKind::Chatgpt, Some(json!({"supportsLunaReserve": true})), /*expect_header*/ true; "eligible_client")]
#[test_case(AuthKind::Fedramp, Some(json!({"supportsLunaReserve": true})), /*expect_header*/ false; "restricted_account")]
#[test_case(AuthKind::Pat, Some(json!({"supportsLunaReserve": true})), /*expect_header*/ false; "personal_access_token")]
#[test_case(AuthKind::Chatgpt, Some(json!({"supportsLunaReserve": true, "excludeResetCreditDetails": true})), /*expect_header*/ true; "lightweight_poll")]
#[tokio::test]
async fn luna_reserve_usage_capability(
    auth_kind: AuthKind,
    params: Option<Value>,
    expect_header: bool,
) -> Result<()> {
    let home = TempDir::new()?;
    let backend = MockServer::start().await;
    let backend_url = backend.uri();
    std::fs::write(
        home.path().join("config.toml"),
        format!("chatgpt_base_url = \"{backend_url}\"\n"),
    )?;
    if auth_kind != AuthKind::Pat {
        let mut claims = ChatGptIdTokenClaims::new().chatgpt_user_id("user-a");
        claims.chatgpt_account_is_fedramp = auth_kind == AuthKind::Fedramp;
        write_chatgpt_auth(
            home.path(),
            ChatGptAuthFixture::new("test-token")
                .account_id("account-a")
                .claims(claims),
            AuthCredentialsStoreMode::File,
        )?;
    }
    Mock::given(method("GET"))
        .and(path("/v1/user-auth-credential/whoami"))
        .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(json!({
            "email": null, "chatgpt_user_id": "user-a",
            "chatgpt_account_id": "account-a", "chatgpt_plan_type": "pro",
            "chatgpt_account_is_fedramp": false
        })))
        .mount(&backend)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(json!({
            "account_id": "account-a", "user_id": "user-a", "plan_type": "pro",
            "rate_limit": {"allowed": false, "limit_reached": true},
            "rate_limit_reset_credits": {"available_count": 2}
        })))
        .expect(/*r*/ 1)
        .mount(&backend)
        .await;
    let lightweight = params
        .as_ref()
        .is_some_and(|params| params["excludeResetCreditDetails"] == true);
    Mock::given(method("GET"))
        .and(path("/api/codex/rate-limit-reset-credits"))
        .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(json!({
            "available_count": 2, "credits": []
        })))
        .expect(if lightweight { 0 } else { 1 })
        .mount(&backend)
        .await;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_auto_env()
        .with_env_overrides(&[
            ("OPENAI_API_KEY", None),
            (
                "CODEX_ACCESS_TOKEN",
                (auth_kind == AuthKind::Pat).then_some("at-test-token"),
            ),
            ("CODEX_AUTHAPI_BASE_URL", Some(backend_url.as_str())),
        ])
        .build_initialized()
        .await?;
    let request = app.send_request("account/rateLimits/read", params).await?;
    let response: GetAccountRateLimitsResponse = app.read_response(request).await?;
    assert_eq!(
        response.rate_limit_reset_credits,
        Some(RateLimitResetCreditsSummary {
            available_count: 2,
            credits: (!lightweight).then(Vec::new),
        })
    );
    let requests = backend
        .received_requests()
        .await
        .expect("recorded backend requests");
    let usage = requests
        .iter()
        .find(|request| request.url.path() == "/api/codex/usage")
        .expect("usage request");
    assert_eq!(
        usage.headers.contains_key("x-openai-codex-luna-reserve"),
        expect_header
    );
    backend.verify().await;
    Ok(())
}
