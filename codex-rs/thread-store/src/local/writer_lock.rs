use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_protocol::ThreadId;
use tracing::warn;

use crate::ThreadStoreError;
use crate::ThreadStoreResult;

const WRITER_LOCK_DIR: &str = "thread-writer-locks";
const COORDINATION_LOCK_FILE: &str = ".coordination.lock";

/// Filesystems that do not support advisory file locking (observed on
/// Termux storage backends under `/data/data/com.termux/files`) surface
/// `ErrorKind::Unsupported` from `File::lock` / `File::try_lock`.
/// Detect this on every target rather than gating on `cfg!(target_os = "android")`:
/// the filesystem behavior is a runtime property, and this also covers older
/// Termux package lines.
///
/// Where it is true the writer lock cannot be taken at all, so single-writer
/// ownership stops being enforced and concurrent writers to one thread are no
/// longer detected. That is the deliberate trade: without this the store
/// cannot open a thread on those filesystems, which takes the whole CLI down
/// at startup rather than weakening a guarantee.
fn is_unsupported_file_lock_error(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::Unsupported
}

pub(super) struct WriterLockCoordinator {
    directory: PathBuf,
    cleanup_attempted: AtomicBool,
}

pub(super) struct WriterLockGuard {
    coordinator: Arc<WriterLockCoordinator>,
    path: PathBuf,
    file: Option<File>,
}

impl WriterLockCoordinator {
    pub(super) fn new(codex_home: &Path) -> Self {
        Self {
            directory: codex_home.join(WRITER_LOCK_DIR),
            cleanup_attempted: AtomicBool::new(false),
        }
    }

    pub(super) fn acquire(
        self: &Arc<Self>,
        thread_id: ThreadId,
    ) -> ThreadStoreResult<WriterLockGuard> {
        let coordination_lock = self.lock_coordination()?;
        if !self.cleanup_attempted.swap(true, Ordering::Relaxed)
            && let Err(err) = self.remove_stale_thread_locks()
        {
            warn!("failed to clean up stale thread writer locks: {err}");
        }

        let path = self.directory.join(format!("{thread_id}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| ThreadStoreError::Internal {
                message: format!(
                    "failed to open thread writer lock {}: {err}",
                    path.display()
                ),
            })?;

        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(ThreadStoreError::Conflict {
                    message: format!("thread {thread_id} already has an active writer"),
                });
            }
            Err(std::fs::TryLockError::Error(err)) if is_unsupported_file_lock_error(&err) => {
                // No advisory locking on this filesystem: proceed unlocked
                // rather than refuse to open the thread.
            }
            Err(std::fs::TryLockError::Error(err)) => {
                return Err(ThreadStoreError::Internal {
                    message: format!(
                        "failed to acquire thread writer lock {}: {err}",
                        path.display()
                    ),
                });
            }
        }

        drop(coordination_lock);
        Ok(WriterLockGuard {
            coordinator: Arc::clone(self),
            path,
            file: Some(file),
        })
    }

    fn lock_coordination(&self) -> ThreadStoreResult<File> {
        fs::create_dir_all(&self.directory).map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to create thread writer lock directory {}: {err}",
                self.directory.display()
            ),
        })?;
        let path = self.directory.join(COORDINATION_LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| ThreadStoreError::Internal {
                message: format!(
                    "failed to open thread writer coordination lock {}: {err}",
                    path.display()
                ),
            })?;
        if let Err(err) = file.lock()
            && !is_unsupported_file_lock_error(&err)
        {
            return Err(ThreadStoreError::Internal {
                message: format!(
                    "failed to acquire thread writer coordination lock {}: {err}",
                    path.display()
                ),
            });
        }
        Ok(file)
    }

    fn remove_stale_thread_locks(&self) -> io::Result<()> {
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(thread_id) = file_name.strip_suffix(".lock") else {
                continue;
            };
            if ThreadId::from_string(thread_id).is_err() {
                continue;
            }

            let path = entry.path();
            let file = match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    warn!(
                        "failed to inspect thread writer lock {}: {err}",
                        path.display()
                    );
                    continue;
                }
            };
            match file.try_lock() {
                Ok(()) => {
                    drop(file);
                    if let Err(err) = fs::remove_file(&path)
                        && err.kind() != io::ErrorKind::NotFound
                    {
                        warn!(
                            "failed to remove stale thread writer lock {}: {err}",
                            path.display()
                        );
                    }
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(err)) => {
                    warn!(
                        "failed to inspect thread writer lock {}: {err}",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }
}

impl Drop for WriterLockGuard {
    fn drop(&mut self) {
        let coordination_lock = match self.coordinator.lock_coordination() {
            Ok(lock) => lock,
            Err(err) => {
                warn!("failed to coordinate thread writer lock cleanup: {err}");
                return;
            }
        };

        // Close the writer lock before deleting it so cleanup works on Windows too.
        drop(self.file.take());
        if let Err(err) = fs::remove_file(&self.path)
            && err.kind() != io::ErrorKind::NotFound
        {
            warn!(
                "failed to remove thread writer lock {}: {err}",
                self.path.display()
            );
        }
        drop(coordination_lock);
    }
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
        // A real lock failure must stay fatal: degrading on these would hide
        // a broken store behind a silently unlocked writer.
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::NotFound,
            ErrorKind::WouldBlock,
            ErrorKind::Other,
        ] {
            assert!(
                !is_unsupported_file_lock_error(&Error::from(kind)),
                "{kind:?} must not be treated as an unsupported lock"
            );
        }
    }
}

#[cfg(test)]
#[path = "writer_lock_tests.rs"]
mod tests;
