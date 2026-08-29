//! Persistence layer for the global, append-only *message history* file.
//!
//! The history is stored at `~/.codex/history.jsonl` with **one JSON object per
//! line** so that it can be efficiently appended to and parsed with standard
//! JSON-Lines tooling. Each record has the following schema:
//!
//! ````text
//! {"session_id":"<uuid>","ts":<unix_seconds>,"text":"<message>"}
//! ````
//!
//! To minimize the chance of interleaved writes when multiple processes are
//! appending concurrently, callers should *prepare the full line* (record +
//! trailing `\n`) and write it with a **single `write(2)` system call** while
//! the file descriptor is opened with the `O_APPEND` flag. On Linux/Android,
//! this keeps each append contiguous even when advisory locks are unavailable.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Result;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use memchr::memchr_iter;
use serde::Deserialize;
use serde::Serialize;

use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncReadExt;

use codex_config::types::History;
use codex_config::types::HistoryPersistence;

mod batch;
pub use batch::HistoryBatch;
pub use batch::HistoryBatchCursor;
pub use batch::HistoryBatchEntry;
pub use batch::lookup_batch;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Filename that stores the message history inside `~/.codex`.
const HISTORY_FILENAME: &str = "history.jsonl";
const HISTORY_READ_BUFFER_SIZE: usize = 8192;

/// When history exceeds the hard cap, trim it down to this fraction of `max_bytes`.
const HISTORY_SOFT_CAP_RATIO: f64 = 0.8;

const MAX_RETRIES: usize = 10;
const RETRY_SLEEP: Duration = Duration::from_millis(100);

/// Filesystems that do not support advisory file locking (observed on
/// Termux storage backends under `/data/data/com.termux/files`) surface
/// `ErrorKind::Unsupported` from `File::try_lock` / `File::try_lock_shared`.
/// Detect this on every target rather than gating on `cfg!(target_os = "android")`:
/// the filesystem behavior is a runtime property, and this also covers older
/// Termux package lines.
/// When this is true, callers should proceed without the advisory lock instead
/// of failing the operation. The single-write O_APPEND contract documented at
/// the top of this file keeps individual lines intact on Linux/Android even
/// without exclusion.
fn is_unsupported_file_lock_error(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::Unsupported
}

#[cfg(test)]
mod unsupported_lock_helper_tests {
    use super::is_unsupported_file_lock_error;
    use std::io::Error;
    use std::io::ErrorKind;

    #[test]
    fn unsupported_kind_is_classified_as_unsupported_lock_error() {
        assert!(is_unsupported_file_lock_error(&Error::from(
            ErrorKind::Unsupported
        )));
    }

    #[test]
    fn other_kinds_are_not_classified_as_unsupported_lock_error() {
        assert!(!is_unsupported_file_lock_error(&Error::from(
            ErrorKind::PermissionDenied
        )));
        assert!(!is_unsupported_file_lock_error(&Error::from(
            ErrorKind::NotFound
        )));
        assert!(!is_unsupported_file_lock_error(&Error::from(
            ErrorKind::WouldBlock
        )));
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub session_id: String,
    pub ts: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryConfig {
    pub codex_home: PathBuf,
    pub persistence: HistoryPersistence,
    pub max_bytes: Option<usize>,
}

impl HistoryConfig {
    pub fn new(codex_home: impl Into<PathBuf>, history: &History) -> Self {
        Self {
            codex_home: codex_home.into(),
            persistence: history.persistence,
            max_bytes: history.max_bytes,
        }
    }
}

fn history_filepath(config: &HistoryConfig) -> PathBuf {
    config.codex_home.join(HISTORY_FILENAME)
}

/// Append a `text` entry associated with `conversation_id` to the history file.
///
/// Uses advisory file locking (`File::try_lock`) with a retry loop to ensure
/// concurrent writes from multiple TUI processes do not interleave. The lock
/// acquisition and write are performed inside `spawn_blocking` so the caller's
/// async runtime is not blocked.
///
/// The entry is silently skipped when `config.history.persistence` is
/// [`HistoryPersistence::None`].
///
/// # Errors
///
/// Returns an I/O error if the history file cannot be opened/created, the
/// system clock is before the Unix epoch, or the exclusive lock cannot be
/// acquired after [`MAX_RETRIES`] attempts.
pub async fn append_entry(
    text: &str,
    conversation_id: impl std::fmt::Display,
    config: &HistoryConfig,
) -> Result<()> {
    match config.persistence {
        HistoryPersistence::SaveAll => {
            // Save everything: proceed.
        }
        HistoryPersistence::None => {
            // No history persistence requested.
            return Ok(());
        }
    }

    // TODO: check `text` for sensitive patterns

    // Resolve `~/.codex/history.jsonl` and ensure the parent directory exists.
    let path = history_filepath(config);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Compute timestamp (seconds since the Unix epoch).
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| std::io::Error::other(format!("system clock before Unix epoch: {e}")))?
        .as_secs();

    // Construct the JSON line first so we can write it in a single syscall.
    let entry = HistoryEntry {
        session_id: conversation_id.to_string(),
        ts,
        text: text.to_string(),
    };
    let mut line = serde_json::to_string(&entry)
        .map_err(|e| std::io::Error::other(format!("failed to serialise history entry: {e}")))?;
    line.push('\n');

    // Open the history file for read/write access (append-only on Unix).
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        options.append(true);
        options.mode(0o600);
    }

    let mut history_file = options.open(&path)?;

    // Ensure permissions.
    ensure_owner_only_permissions(&history_file).await?;

    let history_max_bytes = config.max_bytes;

    // Perform a blocking write under an advisory write lock using std::fs.
    tokio::task::spawn_blocking(move || -> Result<()> {
        // Retry a few times to avoid indefinite blocking when contended.
        for _ in 0..MAX_RETRIES {
            match history_file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(RETRY_SLEEP);
                    continue;
                }
                Err(std::fs::TryLockError::Error(ref e)) if is_unsupported_file_lock_error(e) => {
                    // Termux storage and other backends that reject advisory
                    // file locking. Proceed without the exclusive lock; the
                    // single-write O_APPEND contract documented at the top of
                    // this file keeps each line intact under concurrent writes
                    // on Linux/Android.
                }
                Err(e) => return Err(e.into()),
            }
            // While holding the exclusive lock (or after detecting the
            // filesystem cannot provide one), write the full line. We do not
            // open the file with `append(true)` on Windows, so ensure the
            // cursor is positioned at the end before writing.
            history_file.seek(SeekFrom::End(0))?;
            let written = history_file.write(line.as_bytes())?;
            if written != line.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    format!(
                        "partial history append: wrote {written} of {} bytes",
                        line.len()
                    ),
                ));
            }
            history_file.flush()?;
            enforce_history_limit(&mut history_file, history_max_bytes)?;
            return Ok(());
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "could not acquire exclusive lock on history file after multiple attempts",
        ))
    })
    .await??;

    Ok(())
}

/// Trim the history file to honor `max_bytes`, dropping the oldest lines while holding
/// the write lock so the newest entry is always retained. When the file exceeds the
/// hard cap, it rewrites the remaining tail to a soft cap to avoid trimming again
/// immediately on the next write.
fn enforce_history_limit(file: &mut File, max_bytes: Option<usize>) -> Result<()> {
    let Some(max_bytes) = max_bytes else {
        return Ok(());
    };

    if max_bytes == 0 {
        return Ok(());
    }

    let max_bytes = match u64::try_from(max_bytes) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };

    let mut current_len = file.metadata()?.len();

    if current_len <= max_bytes {
        return Ok(());
    }

    let mut reader_file = file.try_clone()?;
    reader_file.seek(SeekFrom::Start(0))?;

    let mut buf_reader = BufReader::new(reader_file);
    let mut line_lengths = Vec::new();
    let mut line_buf = String::new();

    loop {
        line_buf.clear();

        let bytes = buf_reader.read_line(&mut line_buf)?;

        if bytes == 0 {
            break;
        }

        line_lengths.push(bytes as u64);
    }

    if line_lengths.is_empty() {
        return Ok(());
    }

    let last_index = line_lengths.len() - 1;
    let trim_target = trim_target_bytes(max_bytes, line_lengths[last_index]);

    let mut drop_bytes = 0u64;
    let mut idx = 0usize;

    while current_len > trim_target && idx < last_index {
        current_len = current_len.saturating_sub(line_lengths[idx]);
        drop_bytes += line_lengths[idx];
        idx += 1;
    }

    if drop_bytes == 0 {
        return Ok(());
    }

    let mut reader = buf_reader.into_inner();
    reader.seek(SeekFrom::Start(drop_bytes))?;

    let capacity = usize::try_from(current_len).unwrap_or(0);
    let mut tail = Vec::with_capacity(capacity);

    reader.read_to_end(&mut tail)?;

    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&tail)?;
    file.flush()?;

    Ok(())
}

fn trim_target_bytes(max_bytes: u64, newest_entry_len: u64) -> u64 {
    let soft_cap_bytes = ((max_bytes as f64) * HISTORY_SOFT_CAP_RATIO)
        .floor()
        .clamp(1.0, max_bytes as f64) as u64;

    soft_cap_bytes.max(newest_entry_len)
}

/// Asynchronously fetch the history file's *identifier* and current entry count.
///
/// The identifier is the file's inode on Unix or creation time on Windows.
/// The entry count is derived by counting newline bytes in the file. Returns
/// `(0, 0)` when the file does not exist or its metadata cannot be read. If
/// metadata succeeds but the file cannot be opened or scanned, returns
/// `(log_id, 0)` so callers can still detect that a history file exists.
pub async fn history_metadata(config: &HistoryConfig) -> (u64, usize) {
    let path = history_filepath(config);
    history_metadata_for_file(&path).await
}

/// Look up a single history entry by file identity and zero-based offset.
///
/// Returns `Some(entry)` when the current history file's identifier (inode on
/// Unix, creation time on Windows) matches `log_id` **and** a valid JSON
/// record exists at `offset`. Returns `None` on any mismatch, I/O error, or
/// parse failure, all of which are logged at `warn` level.
///
/// This function is synchronous because it acquires a shared advisory file lock
/// via `File::try_lock_shared`. Callers on an async runtime should wrap it in
/// `spawn_blocking`.
pub fn lookup(log_id: u64, offset: usize, config: &HistoryConfig) -> Option<HistoryEntry> {
    let path = history_filepath(config);
    lookup_history_entry(&path, log_id, offset)
}

/// On Unix systems, ensure the file permissions are `0o600` (rw-------). If the
/// permissions cannot be changed the error is propagated to the caller.
#[cfg(unix)]
async fn ensure_owner_only_permissions(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    let current_mode = metadata.permissions().mode() & 0o777;
    if current_mode != 0o600 {
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        let perms_clone = perms.clone();
        let file_clone = file.try_clone()?;
        tokio::task::spawn_blocking(move || file_clone.set_permissions(perms_clone)).await??;
    }
    Ok(())
}

#[cfg(windows)]
// On Windows, simply succeed.
async fn ensure_owner_only_permissions(_file: &File) -> Result<()> {
    Ok(())
}

async fn history_metadata_for_file(path: &Path) -> (u64, usize) {
    let log_id = match fs::metadata(path).await {
        Ok(metadata) => log_identity(&metadata).unwrap_or(0),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (0, 0),
        Err(_) => return (0, 0),
    };

    // Open the file.
    let mut file = match fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return (log_id, 0),
    };

    // Count newline bytes.
    let mut buf = [0u8; HISTORY_READ_BUFFER_SIZE];
    let mut count = 0usize;
    loop {
        match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                count += memchr_iter(b'\n', &buf[..n]).count();
            }
            Err(_) => return (log_id, 0),
        }
    }

    (log_id, count)
}

fn lookup_history_entry(path: &Path, log_id: u64, offset: usize) -> Option<HistoryEntry> {
    use std::io::BufRead;
    use std::io::BufReader;

    let file: File = match OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "failed to open history file");
            return None;
        }
    };

    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "failed to stat history file");
            return None;
        }
    };

    let current_log_id = log_identity(&metadata)?;

    if log_id != 0 && current_log_id != log_id {
        return None;
    }

    // Open & lock file for reading using a shared lock.
    // Retry a few times to avoid indefinite blocking.
    for _ in 0..MAX_RETRIES {
        match file.try_lock_shared() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                std::thread::sleep(RETRY_SLEEP);
                continue;
            }
            Err(std::fs::TryLockError::Error(ref e)) if is_unsupported_file_lock_error(e) => {
                // Termux storage and other backends that reject advisory
                // file locking. Proceed without the shared lock; concurrent
                // appends may rarely produce a torn final line which the
                // JSON-Lines parser already tolerates by warning and
                // returning None.
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to acquire shared lock on history file");
                return None;
            }
        }

        let reader = BufReader::new(&file);
        for (idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to read line from history file");
                    return None;
                }
            };

            if idx == offset {
                match serde_json::from_str::<HistoryEntry>(&line) {
                    Ok(entry) => return Some(entry),
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to parse history entry");
                        return None;
                    }
                }
            }
        }
        // Not found at requested offset.
        return None;
    }

    None
}

#[cfg(unix)]
fn log_identity(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino())
}

#[cfg(windows)]
fn log_identity(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::windows::fs::MetadataExt;
    Some(metadata.creation_time())
}

#[cfg(not(any(unix, windows)))]
fn log_identity(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod batch_tests;
#[cfg(test)]
mod tests;
