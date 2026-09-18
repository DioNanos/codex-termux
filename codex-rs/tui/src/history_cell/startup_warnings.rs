//! Compact startup diagnostics with full details retained in the transcript.
//! Affected sources are unique; sign-in servers are a subset of MCP servers.

use super::*;
use codex_app_server_protocol::McpServerStartupFailureReason;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub(crate) struct StartupWarningsCell {
    pub(crate) messages: Vec<String>,
    pub(crate) other_sources: BTreeSet<String>,
    pub(crate) mcp_servers: BTreeSet<String>,
    pub(crate) sign_in_servers: BTreeSet<String>,
    pub(crate) pending_header: bool,
    pub(crate) transcript_hint: Option<String>,
}

impl StartupWarningsCell {
    pub(crate) fn new(messages: Vec<String>) -> Self {
        Self {
            other_sources: messages.iter().cloned().collect(),
            messages,
            ..Self::default()
        }
    }

    pub(crate) fn mcp(
        messages: Vec<String>,
        servers: impl IntoIterator<Item = String>,
        failure_reason: Option<McpServerStartupFailureReason>,
    ) -> Self {
        let mcp_servers: BTreeSet<_> = servers.into_iter().collect();
        let sign_in_servers = match failure_reason {
            Some(McpServerStartupFailureReason::ReauthenticationRequired) => mcp_servers.clone(),
            None => BTreeSet::new(),
        };
        Self {
            messages,
            mcp_servers,
            sign_in_servers,
            ..Self::default()
        }
    }
}

impl HistoryCell for StartupWarningsCell {
    #[allow(clippy::disallowed_methods)]
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.pending_header || self.messages.is_empty() || width == 0 {
            return Vec::new();
        }
        let mcp_count = self.mcp_servers.len();
        let count = mcp_count + self.other_sources.len();
        let sign_in_count = self.sign_in_servers.len();
        let plural = if count == 1 { "" } else { "s" };
        let source = if mcp_count == count { "MCP " } else { "" };
        let mut summary = format!("⚠ {count} {source}startup issue{plural}");
        let mut breakdown = Vec::new();
        if mcp_count > 0 && mcp_count < count {
            breakdown.push(format!("{mcp_count} MCP"));
        }
        if sign_in_count > 0 {
            let verb = if sign_in_count == 1 { "needs" } else { "need" };
            breakdown.push(format!("{sign_in_count} {verb} sign-in"));
        }
        if !breakdown.is_empty() {
            summary.push_str(&format!(" ({})", breakdown.join("; ")));
        }
        if let Some(hint) = &self.transcript_hint {
            summary.push_str(&format!(" · {hint} for details"));
        }
        vec![
            crate::line_truncation::truncate_line_with_ellipsis_if_overflow(
                Line::from(summary.yellow().dim()),
                width as usize,
            ),
        ]
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.messages
            .iter()
            .flat_map(|message| new_warning_event(message.clone()).transcript_lines(width))
            .collect()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.messages
            .iter()
            .flat_map(|message| raw_lines_from_source(message))
            .collect()
    }
}
