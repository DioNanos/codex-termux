//! Keeps one OAuth refresh and persistence owner for the lifetime of a connection.
//!
//! Both modes use cached expiry to prepare credentials before the MCP request budget.
//! Coordinated preparation owns a task so caller cancellation cannot interrupt persistence.
//! It locks the manager before the credential store, matching RMCP's lock order.
//! RMCP commits through the pinned store while Legacy retains Codex's persistor.

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use rmcp::transport::auth::AuthorizationManager;
use tokio::sync::Mutex;
use tracing::warn;

use super::OAuthPersistor;
use super::credential_store::OAuthCredentialStore;
use super::token_needs_refresh;

#[derive(Clone)]
pub(crate) enum OAuthRuntime {
    Legacy(OAuthPersistor),
    Coordinated {
        auth_manager: Arc<Mutex<AuthorizationManager>>,
        store: OAuthCredentialStore,
    },
}

impl OAuthRuntime {
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "AuthorizationManager access follows RMCP's manager-before-credential lock order"
    )]
    pub(crate) async fn refresh_if_needed(&self) -> Result<()> {
        match self {
            Self::Legacy(persistor) => persistor.refresh_if_needed().await,
            Self::Coordinated {
                auth_manager,
                store,
            } => {
                let expires_at = store
                    .stored_credentials()
                    .await
                    .and_then(|tokens| tokens.expires_at);
                if !token_needs_refresh(expires_at) {
                    return Ok(());
                }
                let auth_manager = Arc::clone(auth_manager);
                let store = store.clone();
                tokio::spawn(async move {
                    let mut manager = auth_manager.lock().await;
                    let result = store.refresh_if_needed(&mut manager).await;
                    if result.is_err() {
                        // Keep the summary in the owned task without logging credential/provider data.
                        warn!("MCP OAuth preparation failed");
                    }
                    result
                })
                .await
                .context("OAuth refresh task failed")?
            }
        }
    }
}
