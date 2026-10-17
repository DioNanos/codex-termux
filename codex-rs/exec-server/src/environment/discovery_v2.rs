//! Acquires executor inventories without activation and replays safe reads after recovery.

use super::Environment;
use crate::DiscoverV2CapabilitiesRequest;
use crate::DiscoverV2CapabilitiesResponse;
use crate::ExecServerError;

impl Environment {
    /// Returns metadata for the caller's cwd from the remote session's V2 lifecycle.
    /// Local environments and older executors retain the V1 selected-root API.
    pub async fn discover_capabilities_v2(
        &self,
        params: DiscoverV2CapabilitiesRequest,
    ) -> Result<DiscoverV2CapabilitiesResponse, ExecServerError> {
        let Some(client) = &self.remote_client else {
            return Err(ExecServerError::Protocol(
                "capabilities/discoverV2 requires a remote environment".to_string(),
            ));
        };
        let connection_state = client.subscribe_connection_state();
        let client = client.get().await?;
        let discover = || async {
            if !client
                .environment_info()
                .await?
                .capabilities
                .capability_discovery_v2
            {
                return Err(ExecServerError::Protocol(
                    "capabilities/discoverV2 is unavailable in this environment session"
                        .to_string(),
                ));
            }
            client.discover_capabilities_v2(params.clone()).await
        };
        self.discover_with_recovery(connection_state, discover)
            .await
    }
}

#[cfg(test)]
#[path = "discovery_v2_tests.rs"]
mod tests;
