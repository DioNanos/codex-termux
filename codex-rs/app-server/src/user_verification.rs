//! Dispatch boundary for experimental verification APIs.
//! Native operations are introduced separately from the public contract.

use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::UserVerificationErrorDetails;
use codex_app_server_protocol::UserVerificationUnavailableReason;

pub(crate) fn unavailable() -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32603,
        message: "User verification is not available in this build.".into(),
        data: serde_json::to_value(UserVerificationErrorDetails::Unavailable {
            reason: UserVerificationUnavailableReason::ProviderUnavailable,
        })
        .ok(),
    }
}
