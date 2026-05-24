//! Manifest change watcher for the tower TUI.
//!
//! Wraps `notify::RecommendedWatcher` over
//! `<project_root>/.macot/experts_manifest.json` so the tower can refresh
//! its `Vec<ExpertCell>` layout within ~100ms of a `macot expert add`
//! commit (Property 8 in dynamic-expert-add-design.md §6).
//!
//! Usage pattern from `TowerApp`:
//!
//! ```text
//! let watcher = ManifestWatcher::start(project_root)?;
//! // each tick:
//! if watcher.poll_change() {
//!     self.reload_expert_panel();
//! }
//! ```
//!
//! The watcher runs `notify` on a background OS thread and forwards a
//! single `()` token per qualifying event into a `crossbeam`-style mpsc
//! channel. The tower polls that channel non-blockingly from its main
//! event loop. We emit `Some(())` only on rename / create / modify
//! events — `notify` 6.x bundles polling fallbacks internally so we
//! don't add our own.
//!
//! Per Property 8 the only documented bound is the OS-native delivery
//! latency (inotify/fsevents/RDCW). We do not add a polling fallback;
//! Ctrl+L is the manual refresh escape hatch.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::{Context, Result};
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Channel-backed handle. Drop to stop watching.
pub struct ManifestWatcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<()>,
    /// Kept so external callers can introspect the file path under
    /// observation; useful in diagnostics and tests.
    #[allow(dead_code)]
    target: PathBuf,
}

impl ManifestWatcher {
    /// Begin watching `<project_root>/.macot/experts_manifest.json` (and
    /// its parent directory, since the file may be replaced via rename
    /// from a sibling temp file by [`ManifestPersistor::append_atomic`]).
    pub fn start(project_root: &Path) -> Result<Self> {
        let target = manifest_path(project_root);
        let (tx, rx) = mpsc::channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                if event_is_relevant(&event.kind) {
                    let _ = tx.send(());
                }
            }
        })
        .context("failed to construct notify watcher")?;

        // Watch the parent dir non-recursively so we capture the
        // "rename(tmp -> manifest)" event used by atomic writes.
        let parent = target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest has no parent: {}", target.display()))?;
        if !parent.exists() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create watch parent dir: {}", parent.display())
            })?;
        }
        watcher
            .watch(parent, RecursiveMode::NonRecursive)
            .with_context(|| format!("failed to watch {}", parent.display()))?;

        Ok(Self {
            _watcher: watcher,
            rx,
            target,
        })
    }

    /// The manifest path under observation.
    #[allow(dead_code)]
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Drain and return whether at least one relevant change was queued.
    /// Non-blocking: never sleeps.
    pub fn poll_change(&self) -> bool {
        let mut seen = false;
        while let Ok(()) = self.rx.try_recv() {
            seen = true;
        }
        seen
    }

    /// Block up to `dur` for a change. Used by tests that need to wait
    /// for the OS notify pipeline to flush. Production callers should
    /// use `poll_change` from the main loop.
    #[cfg(test)]
    pub fn wait_change(&self, dur: std::time::Duration) -> bool {
        match self.rx.recv_timeout(dur) {
            Ok(()) => {
                // Drain any other queued events.
                while self.rx.try_recv().is_ok() {}
                true
            }
            Err(_) => false,
        }
    }
}

fn manifest_path(project_root: &Path) -> PathBuf {
    project_root.join(".macot").join("experts_manifest.json")
}

/// Returns whether a notify event kind should trigger a manifest reload.
///
/// Pulled out as a free function so it's directly unit-testable (Task
/// 11.4 — Property 8 reload-on-rename assertion).
pub fn event_is_relevant(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Name(RenameMode::To))
            | EventKind::Modify(ModifyKind::Name(RenameMode::Both))
            | EventKind::Modify(ModifyKind::Name(RenameMode::Any))
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Any)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn event_is_relevant_for_rename_to() {
        // Property 8: a manifest commit (rename of .tmp -> manifest)
        // must fire a reload.
        assert!(event_is_relevant(&EventKind::Modify(ModifyKind::Name(
            RenameMode::To
        ))));
    }

    #[test]
    fn event_is_relevant_for_data_modify() {
        assert!(event_is_relevant(&EventKind::Modify(ModifyKind::Data(
            DataChange::Content
        ))));
    }

    #[test]
    fn event_is_relevant_for_create() {
        assert!(event_is_relevant(&EventKind::Create(CreateKind::File)));
    }

    #[test]
    fn event_is_irrelevant_for_access() {
        // Read-only access (atime updates etc.) must not trigger a
        // reload — that would create busy loops on systems where
        // notify forwards access events.
        assert!(!event_is_relevant(&EventKind::Access(
            notify::event::AccessKind::Read
        )));
    }

    #[test]
    fn watcher_emits_on_atomic_rename() {
        // Property 8 (Tower Liveness Under Add): writing
        // experts_manifest.json via tmp + rename produces a notify
        // event the tower picks up within the platform-typical window.
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path();

        let watcher = ManifestWatcher::start(project_root).expect("watcher start");

        let manifest = manifest_path(project_root);
        let staging = manifest.with_extension("json.tmp.test");
        std::fs::write(&staging, b"[]").expect("write tmp");
        std::fs::rename(&staging, &manifest).expect("rename");

        // 1s mirrors the macOS fsevents coalescing worst case in the
        // design (Property 8 bound).
        let observed = watcher.wait_change(Duration::from_secs(1));
        assert!(observed, "watcher should observe the rename within 1s");
    }

    #[test]
    fn watcher_target_path_matches_macot_layout() {
        let tmp = TempDir::new().unwrap();
        let watcher = ManifestWatcher::start(tmp.path()).unwrap();
        assert_eq!(
            watcher.target(),
            tmp.path().join(".macot").join("experts_manifest.json")
        );
    }
}
