//! Validates framed provisioning requests and normalizes proxy settings.

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_windows_sandbox::PROVISIONING_PROTOCOL_VERSION;
use codex_windows_sandbox::ProvisioningMessage;
use codex_windows_sandbox::WindowsSandboxProvisioningSettings;
use codex_windows_sandbox::WindowsSandboxProxyListeners;
use codex_windows_sandbox::read_provisioning_frame;
use std::path::PathBuf;

use super::MAX_REQUEST_BYTES;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ProvisioningRequest {
    pub(super) codex_home: PathBuf,
    pub(super) listeners: WindowsSandboxProxyListeners,
    pub(super) settings: WindowsSandboxProvisioningSettings,
}

pub(super) fn validate_request(request: &[u8]) -> Result<ProvisioningRequest> {
    if request.len() > MAX_REQUEST_BYTES {
        bail!("provisioning request exceeds size limit");
    }
    let mut reader = request;
    let frame = read_provisioning_frame(&mut reader)
        .context("invalid framed provisioning request")?
        .context("provisioning client sent an empty request")?;
    if !reader.is_empty() {
        bail!("provisioning requests must contain exactly one IPC frame");
    }
    if frame.version != PROVISIONING_PROTOCOL_VERSION {
        bail!(
            "unsupported provisioning request version: {}",
            frame.version
        );
    }
    let ProvisioningMessage::ProvisionSandboxRequest { payload: request } = frame.message else {
        bail!("expected a sandbox provisioning request");
    };
    if request.codex_home.is_empty() || request.codex_home.contains(['\0', '\r', '\n']) {
        bail!("Codex home is empty or contains an invalid control character");
    }
    let mut settings = request.settings;
    let mut listeners = request.listeners;
    for ports in [
        &mut settings.proxy_ports,
        &mut listeners.http_ports,
        &mut listeners.socks_ports,
    ] {
        if ports.contains(&0) {
            bail!("provisioning request includes an invalid proxy port");
        }
        ports.sort_unstable();
        ports.dedup();
    }
    if listeners
        .http_ports
        .iter()
        .chain(&listeners.socks_ports)
        .any(|port| !settings.proxy_ports.contains(port))
    {
        bail!("provisioning listener is absent from the proxy settings");
    }
    Ok(ProvisioningRequest {
        codex_home: PathBuf::from(request.codex_home),
        listeners,
        settings,
    })
}
