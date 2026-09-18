//! Managed requirements for application traffic, separate from agent permissions.

use super::NetworkDomainPermission;
use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ApplicationRequirements {
    pub network: Option<ApplicationNetworkRequirements>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ApplicationNetworkRequirements {
    /// When enabled, only explicitly allowed exact domains may be contacted.
    pub enabled: bool,
    pub domains: BTreeMap<String, NetworkDomainPermission>,
}
