//! Dynamic expert add orchestration.
//!
//! `ExpertAddService` is the single entry-point for adding a new expert
//! to a running session. It is invoked by both the `macot expert add`
//! CLI and the tower TUI add modal (tasks 10–11).
//!
//! The service takes care of:
//! - Validating the user-provided role / name.
//! - Allocating a fresh `expert_id` (single-monotonic per the design).
//! - Writing the per-expert state files (`system_prompt/expert{N}.md`,
//!   `_settings.json`, `status/expert{N}`, per-session
//!   `context.yaml`, `expert_roles.yaml`).
//! - Committing the manifest atomically (Property 1 / Property 7).
//! - Spawning the tmux window via [`TmuxWindowSpawner`] outside the
//!   lock (Property 10).
//! - Rolling back state on tmux failure (Property 1 / Property 7).
//!
//! See dynamic-expert-add-design.md §3.1 / §5 / §6.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Error as AnyError;
use chrono::Utc;
use regex::Regex;
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

use crate::context::{ExpertContext, RoleAssignment, SessionExpertRoles};
use crate::expert::role::{resolve as resolve_role, RoleError, RoleSpec};
use crate::experts::names::NamePool;
use crate::experts::persist::{ExpertEntry, ManifestPersistor, PersistError};
use crate::experts::registry::{ExpertRegistry, AUTO_ASSIGN_ID};
use crate::instructions::generate_hooks_settings;
use crate::models::{ExpertId, ExpertInfo, Role};
use crate::session::TmuxWindowSpawner;
use crate::state::lock::{LockError, MacotLock, DEFAULT_TIMEOUT};

/// Request sent to [`ExpertAddService::add_expert`].
#[derive(Debug, Clone)]
pub struct ExpertAddRequest {
    pub role: RoleSpec,
    /// User-supplied display name. `None` triggers automatic selection
    /// from [`NamePool`].
    pub name: Option<String>,
}

/// Outcome of a successful add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpertAdded {
    pub session: String,
    pub expert_id: ExpertId,
    pub name: String,
    pub role: String,
    pub tmux_window_index: u32,
}

/// Public errors emitted by the add flow.
#[derive(Debug, Error)]
pub enum ExpertAddError {
    #[error("invalid role spec: {0}")]
    InvalidRole(String),

    #[error("invalid name '{name}': {reason}")]
    InvalidName { name: String, reason: String },

    #[error("name '{0}' already used in this session")]
    DuplicateName(String),

    #[error("another macot operation in progress (lock busy)")]
    LockBusy,

    #[error("manifest write failed: {0}")]
    StateWrite(#[source] std::io::Error),

    #[error("manifest persist failed: {0}")]
    Manifest(#[source] PersistError),

    #[error("tmux launch failed: {0}")]
    TmuxLaunch(#[source] AnyError),

    #[error("rollback failed after {original}: {rollback}")]
    RollbackFailure {
        original: Box<ExpertAddError>,
        rollback: String,
    },
}

impl From<LockError> for ExpertAddError {
    fn from(value: LockError) -> Self {
        match value {
            LockError::Timeout(_) => ExpertAddError::LockBusy,
            LockError::Io { source, .. } => ExpertAddError::StateWrite(source),
        }
    }
}

impl From<RoleError> for ExpertAddError {
    fn from(value: RoleError) -> Self {
        ExpertAddError::InvalidRole(value.to_string())
    }
}

impl From<PersistError> for ExpertAddError {
    fn from(value: PersistError) -> Self {
        ExpertAddError::Manifest(value)
    }
}

const NAME_MIN_LEN: usize = 1;
const NAME_MAX_LEN: usize = 32;

fn name_regex() -> &'static Regex {
    use std::sync::OnceLock;
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_-]*$").expect("valid name regex"))
}

fn validate_user_name(name: &str) -> Result<(), ExpertAddError> {
    let len = name.chars().count();
    if !(NAME_MIN_LEN..=NAME_MAX_LEN).contains(&len) {
        return Err(ExpertAddError::InvalidName {
            name: name.to_string(),
            reason: format!("length must be {NAME_MIN_LEN}..={NAME_MAX_LEN} chars (got {len})"),
        });
    }
    if !name_regex().is_match(name) {
        return Err(ExpertAddError::InvalidName {
            name: name.to_string(),
            reason: "must match ^[A-Za-z][A-Za-z0-9_-]*$".to_string(),
        });
    }
    Ok(())
}

/// Orchestrates the dynamic add flow.
///
/// Generic over `S: TmuxWindowSpawner` so unit tests can inject the
/// in-memory mock from `crate::session::tmux::test_support`.
pub struct ExpertAddService<S>
where
    S: TmuxWindowSpawner,
{
    project_root: PathBuf,
    session_name: String,
    session_hash: String,
    spawner: S,
    lock_timeout: Duration,
    /// Process-local mutex serialising `add_expert` calls within the
    /// same `ExpertAddService` instance. The advisory file lock
    /// (`MacotLock`) handles cross-process serialisation; this mutex
    /// also defends in-memory `ExpertRegistry` reconciliation against
    /// async runtime interleavings within one process.
    inflight: Arc<AsyncMutex<()>>,
}

impl<S: TmuxWindowSpawner> ExpertAddService<S> {
    pub fn new(
        project_root: PathBuf,
        session_name: String,
        session_hash: String,
        spawner: S,
    ) -> Self {
        Self {
            project_root,
            session_name,
            session_hash,
            spawner,
            lock_timeout: DEFAULT_TIMEOUT,
            inflight: Arc::new(AsyncMutex::new(())),
        }
    }

    /// Override the file-lock acquisition timeout (defaults to
    /// [`DEFAULT_TIMEOUT`]). Mainly used in tests.
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// Synchronous handle to the lock for tests that want to assert
    /// lock-release-before-tmux behaviour.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub async fn add_expert(&self, req: ExpertAddRequest) -> Result<ExpertAdded, ExpertAddError> {
        let _serialise = self.inflight.lock().await;

        // 1. Resolve role (FS read; no lock needed).
        let resolved = resolve_role(&req.role, &self.project_root).map_err(ExpertAddError::from)?;

        // 2. Validate user-supplied name (regex/length only — duplicate
        //    check happens after we've loaded the manifest under lock).
        if let Some(ref n) = req.name {
            validate_user_name(n)?;
        }

        // 3. Acquire the advisory lock for the state-write critical
        //    section.
        let lock = MacotLock::acquire(&self.project_root, self.lock_timeout)?;

        // 4. Reload manifest -> registry; reconcile next_id with disk.
        let persistor = ManifestPersistor::new(self.project_root.clone());
        let mut registry = persistor.load_into_registry()?;
        let on_disk_max = persistor.load_entries()?.iter().map(|e| e.expert_id).max();
        registry.reconcile_next_id_with_disk(on_disk_max);

        // 5. Determine the name (auto-pick or validated user value).
        let chosen_name = self.choose_name(&registry, req.name.as_deref())?;

        // 6. Allocate ID via registry (uses AUTO_ASSIGN_ID sentinel).
        let provisional_id = registry.next_id();
        let info = ExpertInfo::new(
            AUTO_ASSIGN_ID,
            chosen_name.clone(),
            Role::Specialist(resolved.canonical_name.clone()),
            self.session_name.clone(),
            String::new(),
        );
        registry.register_expert(info).map_err(|e| match e {
            crate::experts::RegistryError::DuplicateName(n) => ExpertAddError::DuplicateName(n),
            other => ExpertAddError::InvalidName {
                name: chosen_name.clone(),
                reason: other.to_string(),
            },
        })?;
        let expert_id = provisional_id;

        // 7. Write per-expert state files. On any failure delete what
        //    we've already created so the manifest never sees a partial
        //    expert.
        let written: Vec<PathBuf> = match self
            .write_state_files(expert_id, &chosen_name, &resolved)
            .await
        {
            Ok(paths) => paths,
            Err(err) => {
                // Even though we never reached the manifest commit,
                // some files may exist on disk; clean them up so future
                // add attempts don't trip duplicate checks.
                self.cleanup_state_files(expert_id);
                registry.decrement_next_id_after_failed_commit();
                return Err(err);
            }
        };

        // 8. Append to expert_roles.yaml under the lock.
        if let Err(err) = self
            .append_role_assignment(expert_id, &resolved.canonical_name)
            .await
        {
            self.cleanup_state_files(expert_id);
            registry.decrement_next_id_after_failed_commit();
            return Err(err);
        }

        // 9. Commit the manifest (last write — Property 1 commit point).
        let entry = ExpertEntry {
            expert_id,
            name: chosen_name.clone(),
            role: resolved.canonical_name.clone(),
            worktree_path: None,
        };
        if let Err(err) = persistor.append_atomic(&entry) {
            self.cleanup_state_files(expert_id);
            let _ = self.remove_role_assignment(expert_id).await;
            registry.decrement_next_id_after_failed_commit();
            return Err(ExpertAddError::Manifest(err));
        }

        // 10. Release the file lock before tmux I/O (Property 10).
        drop(lock);
        let _ = written; // keep the variable alive for clarity

        // 11. Spawn the tmux window. On failure roll the manifest +
        //     state files back; the caller-visible state is identical
        //     to the pre-call state.
        let prompt_path = self.prompt_path(expert_id);
        let settings_path = self.settings_path(expert_id);
        let cwd = self.project_root.clone();
        let window_index = match self
            .spawner
            .spawn_expert_window(expert_id, &cwd, &prompt_path, &settings_path)
            .await
        {
            Ok(idx) => idx,
            Err(spawn_err) => {
                let rollback_outcome = self.rollback_after_tmux_failure(expert_id).await;
                return Err(match rollback_outcome {
                    Ok(()) => ExpertAddError::TmuxLaunch(spawn_err),
                    Err(rb) => ExpertAddError::RollbackFailure {
                        original: Box::new(ExpertAddError::TmuxLaunch(spawn_err)),
                        rollback: rb,
                    },
                });
            }
        };

        Ok(ExpertAdded {
            session: self.session_name.clone(),
            expert_id,
            name: chosen_name,
            role: resolved.canonical_name,
            tmux_window_index: window_index,
        })
    }

    fn choose_name(
        &self,
        registry: &ExpertRegistry,
        user_supplied: Option<&str>,
    ) -> Result<String, ExpertAddError> {
        let mut used: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for info in registry.get_all_experts() {
            used.insert(info.name.as_str());
        }
        match user_supplied {
            Some(name) => {
                if used.contains(name) {
                    return Err(ExpertAddError::DuplicateName(name.to_string()));
                }
                Ok(name.to_string())
            }
            None => {
                let pool = NamePool::new();
                if let Some(picked) = pool.pick_unused(&used) {
                    Ok(picked.to_string())
                } else {
                    let mut id = registry.next_id();
                    loop {
                        let candidate = pool.fallback(id);
                        if !used.contains(candidate.as_str()) {
                            return Ok(candidate);
                        }
                        id = id.saturating_add(1);
                    }
                }
            }
        }
    }

    async fn write_state_files(
        &self,
        expert_id: ExpertId,
        name: &str,
        resolved: &crate::expert::role::ResolvedRole,
    ) -> Result<Vec<PathBuf>, ExpertAddError> {
        let mut paths = Vec::new();

        let prompt_dir = self.system_prompt_dir();
        tokio::fs::create_dir_all(&prompt_dir)
            .await
            .map_err(ExpertAddError::StateWrite)?;

        let prompt = self.prompt_path(expert_id);
        tokio::fs::write(&prompt, resolved.prompt_md.as_bytes())
            .await
            .map_err(ExpertAddError::StateWrite)?;
        paths.push(prompt);

        let status_path = self.status_path(expert_id);
        let status_dir = status_path.parent().unwrap();
        tokio::fs::create_dir_all(&status_dir)
            .await
            .map_err(ExpertAddError::StateWrite)?;
        tokio::fs::write(&status_path, b"pending")
            .await
            .map_err(ExpertAddError::StateWrite)?;
        paths.push(status_path.clone());

        // Render settings template now that the status path exists, so
        // the embedded hook references the actual on-disk file.
        let settings = self.settings_path(expert_id);
        let rendered = resolved
            .settings_template
            .render(status_path.to_str().unwrap_or(""));
        tokio::fs::write(&settings, rendered.as_bytes())
            .await
            .map_err(ExpertAddError::StateWrite)?;
        paths.push(settings);

        let ctx_path = self.context_path(expert_id);
        let ctx_dir = ctx_path.parent().unwrap();
        tokio::fs::create_dir_all(&ctx_dir)
            .await
            .map_err(ExpertAddError::StateWrite)?;
        let ctx = ExpertContext::new(expert_id, name.to_string(), self.session_hash.clone());
        let yaml = serde_yaml::to_string(&ctx)
            .map_err(|e| ExpertAddError::StateWrite(std::io::Error::other(e)))?;
        tokio::fs::write(&ctx_path, yaml.as_bytes())
            .await
            .map_err(ExpertAddError::StateWrite)?;
        paths.push(ctx_path);

        Ok(paths)
    }

    fn cleanup_state_files(&self, expert_id: ExpertId) {
        let _ = std::fs::remove_file(self.prompt_path(expert_id));
        let _ = std::fs::remove_file(self.settings_path(expert_id));
        let _ = std::fs::remove_file(self.status_path(expert_id));
        let ctx_path = self.context_path(expert_id);
        let _ = std::fs::remove_file(&ctx_path);
        if let Some(parent) = ctx_path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }

    async fn append_role_assignment(
        &self,
        expert_id: ExpertId,
        canonical_role: &str,
    ) -> Result<(), ExpertAddError> {
        let path = self.expert_roles_path();
        let mut roles = if path.exists() {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(ExpertAddError::StateWrite)?;
            serde_yaml::from_str::<SessionExpertRoles>(&content)
                .map_err(|e| ExpertAddError::StateWrite(std::io::Error::other(e)))?
        } else {
            SessionExpertRoles::new(self.session_hash.clone())
        };
        let now = Utc::now();
        roles.assignments.push(RoleAssignment {
            expert_id,
            role: canonical_role.to_string(),
            assigned_at: now,
        });
        roles.updated_at = now;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(ExpertAddError::StateWrite)?;
        }
        let yaml = serde_yaml::to_string(&roles)
            .map_err(|e| ExpertAddError::StateWrite(std::io::Error::other(e)))?;
        tokio::fs::write(&path, yaml.as_bytes())
            .await
            .map_err(ExpertAddError::StateWrite)?;
        Ok(())
    }

    async fn remove_role_assignment(&self, expert_id: ExpertId) -> Result<(), ExpertAddError> {
        let path = self.expert_roles_path();
        if !path.exists() {
            return Ok(());
        }
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(ExpertAddError::StateWrite)?;
        let mut roles: SessionExpertRoles = serde_yaml::from_str(&content)
            .map_err(|e| ExpertAddError::StateWrite(std::io::Error::other(e)))?;
        roles.assignments.retain(|a| a.expert_id != expert_id);
        roles.updated_at = Utc::now();
        let yaml = serde_yaml::to_string(&roles)
            .map_err(|e| ExpertAddError::StateWrite(std::io::Error::other(e)))?;
        tokio::fs::write(&path, yaml.as_bytes())
            .await
            .map_err(ExpertAddError::StateWrite)?;
        Ok(())
    }

    async fn rollback_after_tmux_failure(&self, expert_id: ExpertId) -> Result<(), String> {
        // Re-acquire the advisory lock before tearing down committed
        // manifest state — another caller might be racing us.
        let _lock = MacotLock::acquire(&self.project_root, self.lock_timeout)
            .map_err(|e| format!("re-acquire lock: {e}"))?;
        let persistor = ManifestPersistor::new(self.project_root.clone());
        if let Err(e) = persistor.remove_by_id_atomic(expert_id) {
            return Err(format!("manifest unwind: {e}"));
        }
        if let Err(e) = self.remove_role_assignment(expert_id).await {
            return Err(format!("expert_roles unwind: {e}"));
        }
        self.cleanup_state_files(expert_id);
        // Best-effort: ensure no straggler tmux window remains.
        let _ = self.spawner.kill_expert_window(expert_id).await;
        Ok(())
    }

    // ---- path helpers -----------------------------------------------

    pub fn macot_dir(&self) -> PathBuf {
        self.project_root.join(".macot")
    }

    pub fn system_prompt_dir(&self) -> PathBuf {
        self.macot_dir().join("system_prompt")
    }

    pub fn prompt_path(&self, expert_id: ExpertId) -> PathBuf {
        self.system_prompt_dir()
            .join(format!("expert{expert_id}.md"))
    }

    pub fn settings_path(&self, expert_id: ExpertId) -> PathBuf {
        self.system_prompt_dir()
            .join(format!("expert{expert_id}_settings.json"))
    }

    pub fn status_path(&self, expert_id: ExpertId) -> PathBuf {
        self.macot_dir()
            .join("status")
            .join(format!("expert{expert_id}"))
    }

    pub fn context_path(&self, expert_id: ExpertId) -> PathBuf {
        self.macot_dir()
            .join("sessions")
            .join(&self.session_hash)
            .join("experts")
            .join(format!("expert{expert_id}"))
            .join("context.yaml")
    }

    pub fn expert_roles_path(&self) -> PathBuf {
        self.macot_dir()
            .join("sessions")
            .join(&self.session_hash)
            .join("expert_roles.yaml")
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.macot_dir().join("experts_manifest.json")
    }
}

// `generate_hooks_settings` is reachable through `resolved.settings_template.render`
// (see role.rs) — keep an explicit reference here so removing it from
// `instructions::*` would break this file too, surfacing the coupling.
#[allow(dead_code)]
fn _hooks_anchor(p: &str) -> String {
    generate_hooks_settings(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expert::role::{BuiltinRole, RoleSpec};
    use crate::experts::persist::ManifestPersistor;
    use crate::session::tmux::test_support::MockSpawner;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;

    fn make_service(tmp: &TempDir, spawner: MockSpawner) -> ExpertAddService<MockSpawner> {
        ExpertAddService::new(
            tmp.path().to_path_buf(),
            "macot-test".to_string(),
            "test-hash".to_string(),
            spawner,
        )
        .with_lock_timeout(Duration::from_secs(1))
    }

    fn manifest_bytes(svc: &ExpertAddService<MockSpawner>) -> Vec<u8> {
        std::fs::read(svc.manifest_path()).unwrap_or_default()
    }

    fn roles_bytes(svc: &ExpertAddService<MockSpawner>) -> Vec<u8> {
        std::fs::read(svc.expert_roles_path()).unwrap_or_default()
    }

    // --- Task 8.4: happy path ----------------------------------------

    #[tokio::test]
    async fn add_expert_writes_all_state_files_and_returns_added() {
        let tmp = TempDir::new().unwrap();
        let spawner = MockSpawner::new();
        let svc = make_service(&tmp, spawner.clone());

        let added = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: Some("Smerdyakov".to_string()),
            })
            .await
            .expect("add_expert should succeed");

        assert_eq!(added.expert_id, 0);
        assert_eq!(added.name, "Smerdyakov");
        assert_eq!(added.role, "general");

        // All four paired files exist (Property 1).
        assert!(svc.prompt_path(0).exists(), "prompt file missing");
        assert!(svc.settings_path(0).exists(), "settings file missing");
        assert!(svc.status_path(0).exists(), "status file missing");
        assert!(svc.context_path(0).exists(), "context file missing");

        // Manifest contains exactly one entry with id=0.
        let entries = ManifestPersistor::new(tmp.path().to_path_buf())
            .load_entries()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].expert_id, 0);
        assert_eq!(entries[0].name, "Smerdyakov");

        // Mock recorded one spawn call.
        let calls = spawner.spawn_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].expert_id, 0);
    }

    #[tokio::test]
    async fn add_expert_assigns_monotonic_ids_when_called_serially() {
        let tmp = TempDir::new().unwrap();
        let svc = make_service(&tmp, MockSpawner::new());

        let a = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: None,
            })
            .await
            .unwrap();
        let b = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: None,
            })
            .await
            .unwrap();
        assert_eq!(a.expert_id, 0);
        assert_eq!(b.expert_id, 1);
        assert_ne!(a.name, b.name, "auto-pick should yield distinct names");
    }

    // --- Task 8.5: tmux failure rollback -----------------------------

    #[tokio::test]
    async fn add_expert_rolls_back_when_tmux_spawn_fails() {
        let tmp = TempDir::new().unwrap();
        let spawner = MockSpawner::new();
        spawner.fail_next_spawn();
        let svc = make_service(&tmp, spawner.clone());

        let result = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: Some("Doomed".to_string()),
            })
            .await;
        assert!(matches!(result, Err(ExpertAddError::TmuxLaunch(_))));

        // Property 1 + 7: state is logically empty after rollback.
        let entries = ManifestPersistor::new(tmp.path().to_path_buf())
            .load_entries()
            .unwrap();
        assert!(
            entries.is_empty(),
            "manifest should hold no entries after tmux failure, got {entries:?}"
        );

        // expert_roles.yaml may have been created and rewritten with an
        // empty assignments list; either non-existent or `assignments`
        // empty is acceptable.
        if svc.expert_roles_path().exists() {
            let raw = std::fs::read_to_string(svc.expert_roles_path()).unwrap();
            let roles: SessionExpertRoles = serde_yaml::from_str(&raw).unwrap();
            assert!(
                roles.assignments.is_empty(),
                "expert_roles assignments should be empty after rollback, got {:?}",
                roles.assignments
            );
        }

        // No paired files left behind.
        assert!(!svc.prompt_path(0).exists(), "prompt file leaked");
        assert!(!svc.settings_path(0).exists(), "settings file leaked");
        assert!(!svc.status_path(0).exists(), "status file leaked");
        assert!(!svc.context_path(0).exists(), "context file leaked");

        // Mock kill should have been invoked (idempotent rollback).
        let kills = spawner.kill_calls();
        assert!(
            kills.contains(&0),
            "rollback should invoke kill_expert_window for the doomed id"
        );
    }

    // --- Task 8.6: state-write failure (single representative case) --

    #[tokio::test]
    async fn add_expert_state_write_failure_leaves_no_partial_files() {
        let tmp = TempDir::new().unwrap();
        // Pre-create `system_prompt` as a regular file so subsequent
        // `create_dir_all` fails — exercises the prompt-write failure
        // sub-case of Property 7.
        std::fs::create_dir_all(tmp.path().join(".macot")).unwrap();
        std::fs::write(tmp.path().join(".macot").join("system_prompt"), b"junk").unwrap();

        let svc = make_service(&tmp, MockSpawner::new());
        let result = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: Some("Failing".to_string()),
            })
            .await;
        assert!(
            matches!(result, Err(ExpertAddError::StateWrite(_))),
            "expected StateWrite error, got {result:?}"
        );

        // Manifest must still be empty / non-existent.
        let manifest = ManifestPersistor::new(tmp.path().to_path_buf())
            .load_entries()
            .unwrap();
        assert!(manifest.is_empty(), "manifest should remain empty");
    }

    // --- Task 8.7: concurrent adds -----------------------------------

    #[tokio::test]
    async fn add_expert_concurrent_adds_get_distinct_ids() {
        let tmp = Arc::new(TempDir::new().unwrap());
        let spawner = MockSpawner::new();
        let svc = Arc::new(ExpertAddService::new(
            tmp.path().to_path_buf(),
            "macot-test".to_string(),
            "test-hash".to_string(),
            spawner,
        ));

        let svc_a = Arc::clone(&svc);
        let svc_b = Arc::clone(&svc);
        let (ra, rb) = tokio::join!(
            tokio::spawn(async move {
                svc_a
                    .add_expert(ExpertAddRequest {
                        role: RoleSpec::Builtin(BuiltinRole::General),
                        name: None,
                    })
                    .await
            }),
            tokio::spawn(async move {
                svc_b
                    .add_expert(ExpertAddRequest {
                        role: RoleSpec::Builtin(BuiltinRole::General),
                        name: None,
                    })
                    .await
            }),
        );
        let a = ra.unwrap().unwrap();
        let b = rb.unwrap().unwrap();
        assert_ne!(
            a.expert_id, b.expert_id,
            "concurrent adds must produce distinct ids"
        );
        assert_ne!(
            a.name, b.name,
            "concurrent adds must produce distinct names"
        );

        let entries = ManifestPersistor::new(tmp.path().to_path_buf())
            .load_entries()
            .unwrap();
        assert_eq!(entries.len(), 2);
    }

    // --- Task 8.8: lock release before tmux --------------------------

    #[tokio::test]
    async fn lock_is_released_before_tmux_spawn() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let observed = Arc::new(std::sync::Mutex::new(false));

        let spawner = MockSpawner::new();
        {
            let observed = observed.clone();
            let project_root = project_root.clone();
            spawner.on_spawn(move || {
                // Property 10: while the spawner runs, the file lock
                // must be free. `try_acquire` should succeed.
                let acquired = MacotLock::try_acquire(&project_root)
                    .expect("try_acquire I/O failed")
                    .is_some();
                *observed.lock().unwrap() = acquired;
            });
        }

        let svc = make_service(&tmp, spawner);
        svc.add_expert(ExpertAddRequest {
            role: RoleSpec::Builtin(BuiltinRole::General),
            name: Some("Smerdyakov".to_string()),
        })
        .await
        .unwrap();

        assert!(
            *observed.lock().unwrap(),
            "Property 10: tmux phase must observe the file lock as free"
        );
    }

    #[tokio::test]
    async fn invalid_user_name_is_rejected_before_state_writes() {
        let tmp = TempDir::new().unwrap();
        let svc = make_service(&tmp, MockSpawner::new());

        let bad = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: Some("9bad".to_string()), // starts with digit
            })
            .await;
        assert!(matches!(bad, Err(ExpertAddError::InvalidName { .. })));
        // No state should be touched.
        assert!(!svc.prompt_path(0).exists());
    }

    /// **Property 6: Name Uniqueness Within Session** — `add_expert`
    /// rejects a name that is already present in the manifest with
    /// `DuplicateName` and never advances the manifest.
    #[tokio::test]
    async fn duplicate_user_name_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let svc = make_service(&tmp, MockSpawner::new());
        svc.add_expert(ExpertAddRequest {
            role: RoleSpec::Builtin(BuiltinRole::General),
            name: Some("Alyosha".to_string()),
        })
        .await
        .unwrap();

        let dup = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: Some("Alyosha".to_string()),
            })
            .await;
        assert!(matches!(dup, Err(ExpertAddError::DuplicateName(_))));

        // Property 6 corollary: the manifest must still hold exactly one
        // entry — a rejected duplicate must not advance state.
        let entries = ManifestPersistor::new(tmp.path().to_path_buf())
            .load_entries()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Alyosha");
    }

    /// **Property 9: Reset Compatibility** — a dynamically-added expert
    /// produces the exact filesystem layout (`system_prompt/expert{N}.md`,
    /// `system_prompt/expert{N}_settings.json`, `status/expert{N}`,
    /// `sessions/{h}/experts/expert{N}/context.yaml`) that
    /// `macot reset expert {N}` operates on. Asserting the layout
    /// directly is sufficient — the reset command's own tests cover the
    /// reset semantics on top of that layout.
    #[tokio::test]
    async fn dynamically_added_expert_uses_reset_compatible_layout() {
        let tmp = TempDir::new().unwrap();
        let svc = make_service(&tmp, MockSpawner::new());
        let added = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: Some("Resettable".to_string()),
            })
            .await
            .expect("add_expert");

        let n = added.expert_id;
        for path in [
            svc.prompt_path(n),
            svc.settings_path(n),
            svc.status_path(n),
            svc.context_path(n),
        ] {
            assert!(
                path.exists(),
                "Property 9: reset target path missing: {}",
                path.display()
            );
        }
        // The status file must hold the exact `pending` marker that the
        // reset path expects to find before it transitions state.
        assert_eq!(
            std::fs::read_to_string(svc.status_path(n)).unwrap(),
            "pending"
        );
    }
}
