//! End-to-end tests for the dynamic expert add flow.
//!
//! These tests exercise [`macot::expert::add::ExpertAddService`] with a
//! real [`macot::session::TmuxManager`] against an actual `tmux` server,
//! covering the cross-component integration that the unit tests in
//! `src/expert/add.rs` cannot verify (Property 4 — the Process layer).
//!
//! Each test is `#[ignore]`d so the default `cargo test` (and `make ci`)
//! run is hermetic. They opt in via:
//!
//! ```bash
//! make test-e2e   # cargo test --test e2e_dynamic_expert_add -- --ignored
//! ```
//!
//! Requirements: `tmux` on PATH. A real `claude` is not required because
//! the assertions only inspect the *tmux window* the spawn creates;
//! whatever the shell does with the `claude` invocation that gets sent
//! via `send-keys` is irrelevant.
//!
//! Maps to dynamic-expert-add-tasks.md §13.

#![cfg(unix)]

use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use macot::expert::add::{ExpertAddRequest, ExpertAddService};
use macot::expert::role::{BuiltinRole, RoleSpec};
use macot::experts::persist::ManifestPersistor;
use macot::session::TmuxManager;
use regex::Regex;
use tempfile::TempDir;

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn tmux_available() -> bool {
    StdCommand::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// RAII guard that kills the tmux session on drop so a panicking
/// assertion never leaks a session into the host's tmux server.
struct SessionGuard {
    name: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        let _ = StdCommand::new("tmux")
            .args(["kill-session", "-t", &self.name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn create_detached_session(name: &str, cwd: &Path) {
    let status = StdCommand::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            name,
            "-c",
            cwd.to_str().expect("cwd UTF-8"),
            "-x",
            "200",
            "-y",
            "50",
        ])
        .status()
        .expect("failed to spawn tmux");
    assert!(
        status.success(),
        "tmux new-session -d -s {name} failed (is another session with that name running?)"
    );
}

fn list_window_names(session: &str) -> Vec<String> {
    let output = StdCommand::new("tmux")
        .args(["list-windows", "-t", session, "-F", "#{window_name}"])
        .output()
        .expect("tmux list-windows");
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn make_service(
    project_root: &Path,
    session: &str,
    session_hash: &str,
) -> ExpertAddService<TmuxManager> {
    ExpertAddService::new(
        project_root.to_path_buf(),
        session.to_string(),
        session_hash.to_string(),
        TmuxManager::new(session.to_string()),
    )
}

// ---------------------------------------------------------------------
// Task 13.1 — dynamic add against real tmux
// ---------------------------------------------------------------------

/// **Property 4: Tmux Eventual Consistency** — `add_expert` must produce
/// a tmux window named `expert{N}` that is observable via
/// `tmux list-windows`.
///
/// Validates: Property 1, Property 2, Property 4, Requirement 3.5.
#[tokio::test]
#[ignore]
async fn dynamic_add_creates_real_tmux_window() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let session = format!("macot-e2e-{}-add", std::process::id());
    create_detached_session(&session, tmp.path());
    let _guard = SessionGuard {
        name: session.clone(),
    };

    let svc = make_service(tmp.path(), &session, "e2e-add");
    let added = svc
        .add_expert(ExpertAddRequest {
            role: RoleSpec::Builtin(BuiltinRole::General),
            name: Some("Smerdyakov".to_string()),
        })
        .await
        .expect("add_expert succeeds against real tmux");

    assert_eq!(added.expert_id, 0);
    assert_eq!(added.name, "Smerdyakov");
    assert_eq!(added.role, "general");

    let names = list_window_names(&session);
    assert!(
        names.iter().any(|n| n == "expert0"),
        "Property 4: tmux should expose window 'expert0', got {names:?}"
    );

    assert!(svc.prompt_path(0).exists(), "prompt file");
    assert!(svc.settings_path(0).exists(), "settings file");
    assert!(svc.status_path(0).exists(), "status file");
    assert!(svc.context_path(0).exists(), "context.yaml");

    let entries = ManifestPersistor::new(tmp.path().to_path_buf())
        .load_entries()
        .expect("manifest readable");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].expert_id, 0);
    assert_eq!(entries[0].name, "Smerdyakov");
}

/// Adding several experts back-to-back — each one becomes its own tmux
/// window with a monotonically increasing `expert{N}` name.
///
/// Validates: Property 2, Property 4, Requirement 3.5.
#[tokio::test]
#[ignore]
async fn dynamic_add_creates_distinct_windows_for_sequential_adds() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let session = format!("macot-e2e-{}-seq", std::process::id());
    create_detached_session(&session, tmp.path());
    let _guard = SessionGuard {
        name: session.clone(),
    };

    let svc = make_service(tmp.path(), &session, "e2e-seq");

    for _ in 0..3 {
        svc.add_expert(ExpertAddRequest {
            role: RoleSpec::Builtin(BuiltinRole::General),
            name: None,
        })
        .await
        .expect("sequential add");
    }

    let names = list_window_names(&session);
    for n in 0..3u32 {
        let target = format!("expert{n}");
        assert!(
            names.contains(&target),
            "tmux should expose '{target}', got {names:?}"
        );
    }
}

// ---------------------------------------------------------------------
// Task 13.2 — name-pool exhaustion fallback
// ---------------------------------------------------------------------

/// Add experts repeatedly without `--name` until the literary pool
/// drains; assert that at least one returned name matches the
/// `Expert\d{2}` fallback shape.
///
/// Validates: Requirement 3.4.
#[tokio::test]
#[ignore]
async fn name_pool_drains_to_fallback_format() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let session = format!("macot-e2e-{}-pool", std::process::id());
    create_detached_session(&session, tmp.path());
    let _guard = SessionGuard {
        name: session.clone(),
    };

    let svc = make_service(tmp.path(), &session, "e2e-pool");

    // The literary pool ships with 12 names. Add 13 to guarantee at
    // least one fallback emission, with a small safety margin.
    let total: u32 = 13;
    let mut names: Vec<String> = Vec::with_capacity(total as usize);
    for _ in 0..total {
        let added = svc
            .add_expert(ExpertAddRequest {
                role: RoleSpec::Builtin(BuiltinRole::General),
                name: None,
            })
            .await
            .expect("add_expert with no name");
        names.push(added.name);
    }

    let fallback_re = Regex::new(r"^Expert\d{2}$").unwrap();
    let fallback_count = names.iter().filter(|n| fallback_re.is_match(n)).count();
    assert!(
        fallback_count >= 1,
        "after {total} adds at least one name should match `Expert\\d{{2}}`, got {names:?}"
    );

    // Names must remain unique even when the literary pool empties.
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "auto-pick must not produce duplicates: {names:?}"
    );
}

// ---------------------------------------------------------------------
// Task 13.3 — `down --cleanup` analogue
// ---------------------------------------------------------------------

/// `macot down --cleanup` is composed of two observable effects:
///
/// 1. The tmux session is killed.
/// 2. `.macot/` is removed (or its dynamic state is cleared).
///
/// We exercise the underlying flow — add 1 expert, kill the tmux session
/// (mirroring `down`), then nuke `.macot/` (mirroring `--cleanup`) — and
/// assert that everything dynamic-add wrote is gone.
///
/// Validates: Requirement 4.1, Requirement 4.4.
#[tokio::test]
#[ignore]
async fn down_cleanup_removes_dynamic_state() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let session = format!("macot-e2e-{}-cleanup", std::process::id());
    create_detached_session(&session, tmp.path());
    let guard = SessionGuard {
        name: session.clone(),
    };

    let svc = make_service(tmp.path(), &session, "e2e-cleanup");
    svc.add_expert(ExpertAddRequest {
        role: RoleSpec::Builtin(BuiltinRole::General),
        name: Some("Cleanup".to_string()),
    })
    .await
    .expect("add_expert");

    // Sanity: dynamic state exists before cleanup.
    assert!(svc.manifest_path().exists());
    assert!(svc.prompt_path(0).exists());
    assert!(svc.context_path(0).exists());

    // 1. Down: tmux kill-session (handled by guard going out of scope).
    drop(guard);
    // Give tmux a beat to process the SIGTERM.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let alive = StdCommand::new("tmux")
        .args(["has-session", "-t", &session])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(!alive, "tmux session should be gone after kill-session");

    // 2. --cleanup: remove .macot/ and verify nothing dynamic remains.
    std::fs::remove_dir_all(tmp.path().join(".macot")).expect("rm -rf .macot");
    assert!(!svc.manifest_path().exists());
    assert!(!svc.prompt_path(0).exists());
    assert!(!svc.context_path(0).exists());
    assert!(!svc.expert_roles_path().exists());
}
