//! Advisory file lock for `.macot/` mutations.
//!
//! All concurrent producers (CLI `expert add`, tower TUI add modal, future
//! `expert remove`) must acquire [`MacotLock`] before mutating manifest
//! state. The lock is released on drop (RAII).
//!
//! See dynamic-expert-add-design.md §2.4 / Property 3 / Property 10.

// `MacotLock` is consumed by `ExpertAddService` (task 8) and CLI/TUI
// surfaces (tasks 10–11). Until those land we suppress the dead-code
// lint at the bin target so `make lint` stays green.
#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use thiserror::Error;

/// How frequently to retry `try_lock_exclusive` while spinning on
/// acquisition. Picked to keep contention overhead low while still
/// completing well within the 5s budget for the primary CLI flow.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Default acquisition deadline matching the design's 5-second budget.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors surfaced from [`MacotLock`] acquisition.
#[derive(Debug, Error)]
pub enum LockError {
    /// Could not acquire the lock within the deadline. Maps to
    /// `ExpertAddError::LockBusy` at the service boundary.
    #[error("lock acquisition timed out after {0:?}")]
    Timeout(Duration),

    /// I/O failure preparing the lock file or its parent directory.
    #[error("lock file I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// RAII guard for `<project_root>/.macot/.lock`.
///
/// Drop releases the advisory `flock(LOCK_EX)` and closes the file
/// descriptor.
#[derive(Debug)]
pub struct MacotLock {
    file: File,
    path: PathBuf,
}

impl MacotLock {
    /// Acquire the lock, spinning with a 50ms poll until either the lock
    /// is obtained or `timeout` elapses.
    ///
    /// `project_root` is the directory that contains `.macot/`. The
    /// lock file (`.macot/.lock`) is created on demand.
    pub fn acquire(project_root: &Path, timeout: Duration) -> Result<Self, LockError> {
        let (file, path) = open_lock_file(project_root)?;
        let start = Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file, path }),
                Err(err) if !is_would_block(&err) => {
                    return Err(LockError::Io { path, source: err });
                }
                Err(_) => {
                    if start.elapsed() >= timeout {
                        return Err(LockError::Timeout(timeout));
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            }
        }
    }

    /// Best-effort, non-blocking acquisition. Returns `Ok(None)` when the
    /// lock is held elsewhere; reserves I/O errors for genuinely failed
    /// filesystem operations.
    pub fn try_acquire(project_root: &Path) -> Result<Option<Self>, LockError> {
        let (file, path) = open_lock_file(project_root)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file, path })),
            Err(err) if is_would_block(&err) => Ok(None),
            Err(err) => Err(LockError::Io { path, source: err }),
        }
    }

    /// Path to the lock file (for diagnostics / logging only).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MacotLock {
    fn drop(&mut self) {
        // Best-effort: nothing meaningful we can do if unlock fails.
        let _ = FileExt::unlock(&self.file);
    }
}

fn open_lock_file(project_root: &Path) -> Result<(File, PathBuf), LockError> {
    let dir = project_root.join(".macot");
    std::fs::create_dir_all(&dir).map_err(|source| LockError::Io {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| LockError::Io {
            path: path.clone(),
            source,
        })?;
    Ok((file, path))
}

fn is_would_block(err: &io::Error) -> bool {
    // fs2 surfaces lock contention as `ErrorKind::WouldBlock` on Unix.
    // Some platforms map `EAGAIN` directly; treat both as contention.
    matches!(err.kind(), io::ErrorKind::WouldBlock)
        || err
            .raw_os_error()
            .map(|c| c == libc_eagain())
            .unwrap_or(false)
}

const fn libc_eagain() -> i32 {
    // EAGAIN value on Linux/macOS — both use 11/35 respectively but
    // ErrorKind::WouldBlock already covers them; keeping a fallback
    // shields tests on platforms where the mapping is incomplete.
    11
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn acquire_creates_lock_file_in_macot_dir() {
        let tmp = TempDir::new().unwrap();
        let lock = MacotLock::acquire(tmp.path(), DEFAULT_TIMEOUT).expect("acquire");
        let expected = tmp.path().join(".macot").join(".lock");
        assert_eq!(
            lock.path(),
            expected.as_path(),
            "acquire: lock path should be <root>/.macot/.lock"
        );
        assert!(expected.exists(), "acquire: lock file should be created");
    }

    #[test]
    fn second_try_acquire_returns_none_while_first_held() {
        let tmp = TempDir::new().unwrap();
        let _held = MacotLock::acquire(tmp.path(), DEFAULT_TIMEOUT).unwrap();

        // try_acquire on the same fd is reentrant on some platforms, so
        // race against an OS-level second open on a separate descriptor.
        let path = tmp.path().to_path_buf();
        let result = thread::spawn(move || MacotLock::try_acquire(&path).unwrap())
            .join()
            .unwrap();
        assert!(
            result.is_none(),
            "try_acquire: second holder must observe contention"
        );
    }

    #[test]
    fn acquire_succeeds_after_first_holder_drops() {
        let tmp = TempDir::new().unwrap();
        let held = MacotLock::acquire(tmp.path(), DEFAULT_TIMEOUT).unwrap();
        let path = tmp.path().to_path_buf();

        // Spawn a thread that releases the lock after a short delay.
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            drop(held);
        });

        // The waiter should pick up the lock once the holder drops.
        let waiter = MacotLock::acquire(&path, Duration::from_secs(2));
        handle.join().unwrap();
        assert!(
            waiter.is_ok(),
            "acquire: waiter must succeed once holder releases"
        );
    }

    #[test]
    fn acquire_times_out_when_lock_remains_held() {
        let tmp = TempDir::new().unwrap();
        let _held = MacotLock::acquire(tmp.path(), DEFAULT_TIMEOUT).unwrap();
        let path = tmp.path().to_path_buf();

        let waiter = thread::spawn(move || MacotLock::acquire(&path, Duration::from_millis(200)))
            .join()
            .unwrap();
        match waiter {
            Err(LockError::Timeout(d)) => {
                assert_eq!(
                    d,
                    Duration::from_millis(200),
                    "timeout: should report configured deadline"
                );
            }
            other => panic!("acquire: expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn drop_releases_lock_for_subsequent_acquire() {
        let tmp = TempDir::new().unwrap();
        {
            let _l = MacotLock::acquire(tmp.path(), DEFAULT_TIMEOUT).unwrap();
        }
        // Immediately re-acquire — must succeed without waiting.
        let again = MacotLock::acquire(tmp.path(), Duration::from_millis(100));
        assert!(
            again.is_ok(),
            "drop: should release the lock for subsequent acquisition"
        );
    }

    /// Property 3: Lock-Serialized Critical Section.
    /// Two parallel acquirers serialise — the second one observes the
    /// counter incremented by the first.
    #[test]
    fn parallel_acquirers_observe_serialised_state() {
        let tmp = Arc::new(TempDir::new().unwrap());
        let counter_path = tmp.path().join(".macot").join("counter");
        std::fs::create_dir_all(tmp.path().join(".macot")).unwrap();
        std::fs::write(&counter_path, "0").unwrap();

        let mut handles = Vec::new();
        for _ in 0..4 {
            let root = tmp.path().to_path_buf();
            let counter = counter_path.clone();
            handles.push(thread::spawn(move || {
                let _lock = MacotLock::acquire(&root, Duration::from_secs(2)).unwrap();
                let current: u32 = std::fs::read_to_string(&counter).unwrap().parse().unwrap();
                // Sleep a touch inside the critical section to amplify
                // any interleavings if the lock were broken.
                thread::sleep(Duration::from_millis(20));
                std::fs::write(&counter, (current + 1).to_string()).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let final_value: u32 = std::fs::read_to_string(&counter_path)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            final_value, 4,
            "parallel: serialised increments must reach 4, got {final_value}"
        );
    }
}
