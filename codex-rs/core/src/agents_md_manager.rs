use crate::agents_md::LoadedAgentsMd;
use crate::agents_md::load_project_instructions;
use crate::config::Config;
use crate::environment_selection::TurnEnvironmentSnapshot;
use codex_extension_api::Instructions;
use codex_protocol::config_types::TrustLevel;
use codex_protocol::protocol::TurnEnvironmentSelection;
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Owns the inputs and cached result of AGENTS.md discovery for a session.
pub(crate) struct AgentsMdManager {
    user_instructions: Option<Instructions>,
    cache: Mutex<AgentsMdCache>,
}

#[derive(Default)]
struct AgentsMdCache {
    selections: Option<Vec<TurnEnvironmentSelection>>,
    active_project_trust_level: Option<TrustLevel>,
    loaded: Option<Arc<LoadedAgentsMd>>,
}

impl AgentsMdManager {
    pub(crate) fn new(user_instructions: Option<Instructions>) -> Self {
        Self {
            user_instructions: user_instructions
                .filter(|instructions| !instructions.text.trim().is_empty()),
            cache: Mutex::new(AgentsMdCache::default()),
        }
    }

    #[tracing::instrument(name = "agents_md.refresh", skip_all)]
    pub(crate) async fn refresh(
        &self,
        config: &Config,
        environments: &TurnEnvironmentSnapshot,
    ) -> io::Result<()> {
        let selections = environments
            .turn_environments()
            .map(|environment| environment.selection.clone())
            .collect::<Vec<_>>();
        let active_project_trust_level = config.active_project.trust_level;
        {
            let mut cache = self.cache.lock().await;
            // The environment selection is not the only input to discovery: so
            // are the files on disk. A session that starts without an AGENTS.md
            // and then has one created — which is what `/init` does — kept
            // reporting that none existed, because the selection had not
            // changed. Skip the work only when instructions have actually been
            // found; re-running discovery while nothing exists is a bounded set
            // of path probes and stops costing anything as soon as they do.
            if cache.selections.as_ref() == Some(&selections)
                && cache.active_project_trust_level == active_project_trust_level
                && cache.loaded.is_some()
            {
                return Ok(());
            }
            cache.selections = None;
            cache.active_project_trust_level = None;
            cache.loaded = None;
        }

        let loaded =
            load_project_instructions(config, self.user_instructions.clone(), environments)
                .await?
                .map(Arc::new);
        let mut cache = self.cache.lock().await;
        cache.selections = Some(selections);
        cache.active_project_trust_level = active_project_trust_level;
        cache.loaded = loaded;
        Ok(())
    }

    pub(crate) async fn get_loaded(&self) -> Option<Arc<LoadedAgentsMd>> {
        self.cache.lock().await.loaded.clone()
    }

    pub(crate) fn user_instructions(&self) -> Option<Instructions> {
        self.user_instructions.clone()
    }
}
