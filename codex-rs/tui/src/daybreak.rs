//! Read-only Daybreak eligibility used to choose cyber refusal copy.
//! Account-scoped discovery runs in the background; pending/failed reads use neutral copy.
//! Astra takes precedence; the TUI does not configure the app’s Daybreak access program.

use crate::app_server_session::AppServerSession;
use crate::legacy_core::config::Config;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::AuthMode;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::GetAuthStatusParams;
use codex_app_server_protocol::GetAuthStatusResponse;
use codex_app_server_protocol::RequestId;
use codex_http_client::ClientRouteClass;
use codex_http_client::RouteAwareClientPool;
use codex_login::CodexAuth;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

pub(crate) type NoticeCache = Arc<OnceCell<Notice>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Notice {
    Apply,
    Astra,
    #[default]
    Limited,
}

pub(crate) fn prefetch_notice(config: &Config, server: &AppServerSession, cache: NoticeCache) {
    if config.model_provider_id != "openai"
        || server.uses_remote_workspace()
        || cache.get().is_some()
    {
        return;
    }
    let config = config.clone();
    let request_handle = server.request_handle();
    tokio::spawn(async move {
        cache
            .get_or_init(|| read_notice(&config, &request_handle))
            .await;
    });
}

async fn read_notice(config: &Config, request_handle: &AppServerRequestHandle) -> Notice {
    tokio::time::timeout(Duration::from_secs(3), async {
        // Allow the server to refresh its token before reading local credentials.
        let status: GetAuthStatusResponse = request_handle
            .request_typed(ClientRequest::GetAuthStatus {
                request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
                params: GetAuthStatusParams {
                    include_token: Some(true),
                    refresh_token: Some(false),
                },
            })
            .await
            .ok()?;
        let auth = config
            .auth_config()
            .load_auth(/*enable_codex_api_key_env*/ false)
            .await
            .ok()
            .flatten()?;
        let CodexAuth::Chatgpt(_) = &auth else {
            return None;
        };
        if status.auth_method != Some(AuthMode::Chatgpt)
            || status.auth_token.as_deref() != Some(auth.get_token().ok()?.as_str())
        {
            return None;
        }
        let client = RouteAwareClientPool::new_without_redirects(
            config.http_client_factory(),
            ClientRouteClass::Api,
        );
        let url = format!(
            "{}/accounts/verified_access",
            config.chatgpt_base_url.trim_end_matches('/')
        );
        let response = client
            .get(url)
            .headers(codex_model_provider::auth_provider_from_auth(&auth).to_auth_headers())
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let access = response.json::<VerifiedAccess>().await.ok()?;
        let current_auth = config
            .auth_config()
            .load_auth(/*enable_codex_api_key_env*/ false)
            .await
            .ok()
            .flatten()?;
        if current_auth.get_token().ok()? != auth.get_token().ok()?
            || current_auth.get_account_id() != auth.get_account_id()
        {
            return None;
        }
        Some(access.notice())
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default()
}

#[derive(Deserialize)]
struct VerifiedAccess {
    programs: Vec<Program>,
}
#[derive(Deserialize)]
#[serde(tag = "program", rename_all = "snake_case")]
enum Program {
    Cyber {
        state: String,
        grants: Vec<serde::de::IgnoredAny>,
    },
    #[serde(other)]
    Other,
}
impl VerifiedAccess {
    fn notice(&self) -> Notice {
        let absent = self.programs.iter().all(|program| match program {
            Program::Cyber { state, grants } => state == "inactive" && grants.is_empty(),
            Program::Other => true,
        });
        if absent {
            Notice::Apply
        } else {
            Notice::Limited
        }
    }
}

impl Notice {
    pub(crate) fn for_model(self, model: &str) -> Self {
        match model {
            // Same identifiers as the app's isDaybreakUnavailableModel.
            "gpt-6-astra" | "gpt-6-astra-wm" => Self::Astra,
            "gpt-5.6-sol" => self,
            // Other model/access-program mappings are not established for the TUI.
            _ => Self::Limited,
        }
    }
}

#[cfg(test)]
#[path = "daybreak_tests.rs"]
mod tests;
