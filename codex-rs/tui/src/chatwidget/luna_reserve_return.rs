//! Persist the account-bound return model for a task before automatically entering Reserve.
//! Task IDs scope the cache across reconnects/resumes without changing global model defaults.

use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ReserveReturnModel {
    pub account_id: String,
    pub model: String,
    pub effort: Option<ReasoningEffort>,
}

impl ReserveReturnModel {
    fn path(codex_home: &Path, thread_id: ThreadId) -> PathBuf {
        codex_home
            .join("tui-luna-reserve")
            .join(format!("{thread_id}.json"))
    }

    pub(super) fn load(codex_home: &Path, thread_id: ThreadId) -> Option<Self> {
        let file = std::fs::File::open(Self::path(codex_home, thread_id)).ok()?;
        // A corrupt or unrelated cache file must not cause an unbounded read.
        serde_json::from_reader(file.take(/*limit*/ 4096)).ok()
    }

    pub(super) fn save(&self, codex_home: &Path, thread_id: ThreadId) -> io::Result<()> {
        let path = Self::path(codex_home, thread_id);
        let directory = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
        std::fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut file, self)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    pub(super) fn clear(codex_home: &Path, thread_id: ThreadId) {
        // A missing cache needs no cleanup; the live model is authoritative after leaving Reserve.
        let _ = std::fs::remove_file(Self::path(codex_home, thread_id));
    }
}
