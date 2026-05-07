// Items in this module are consumed by `ExpertAddService` (Phase 8) and
// the CLI/TUI surfaces (Phases 10–11). Until those land, suppress
// dead-code lints from the bin target to keep `make lint` green.
#![allow(dead_code)]

//! Manifest persistor for dynamic expert add.
//!
//! Reads and writes `<project_root>/.macot/experts_manifest.json` as the
//! authoritative source of truth for the expert roster. Writes are
//! performed via temp-file + `rename(2)` so partial writes are never
//! observable by other processes (Property 1 / Property 7 in the design
//! doc).
//!
//! Callers are expected to hold the `.macot/.lock` advisory file lock
//! during any mutation; this module does not acquire the lock itself.
//!
//! See design doc §3.2 / §4.2 / §4.4.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::experts::registry::{ExpertRegistry, AUTO_ASSIGN_ID};
use crate::models::{ExpertId, ExpertInfo, Role};

/// Schema for a single entry in `experts_manifest.json`.
///
/// Mirrors the existing on-disk shape used by the static start flow so
/// that dynamic-add reads and writes remain compatible with the rest of
/// the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpertEntry {
    pub expert_id: u32,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub worktree_path: Option<String>,
}

/// Errors surfaced from manifest I/O.
#[derive(Debug, Error)]
pub enum PersistError {
    #[error("manifest read failed at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("manifest write failed at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("manifest rename failed: {from} -> {to}: {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("manifest parse failed at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("registry rebuild failed: duplicate expert_id {0} in manifest")]
    DuplicateId(ExpertId),

    #[error("registry rebuild failed: duplicate name {0} in manifest")]
    DuplicateName(String),
}

/// Reads and writes the manifest atomically.
#[derive(Debug, Clone)]
pub struct ManifestPersistor {
    /// Project root — i.e. the directory that contains `.macot/`.
    project_root: PathBuf,
}

impl ManifestPersistor {
    /// Construct a persistor rooted at `project_root` (the directory that
    /// contains `.macot/`).
    pub fn new<P: Into<PathBuf>>(project_root: P) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }

    /// Path to `<project_root>/.macot/experts_manifest.json`.
    pub fn manifest_path(&self) -> PathBuf {
        self.project_root
            .join(".macot")
            .join("experts_manifest.json")
    }

    /// Read all entries from the manifest. Missing file is treated as an
    /// empty roster (Ok(vec![])).
    pub fn load_entries(&self) -> Result<Vec<ExpertEntry>, PersistError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(&path).map_err(|source| PersistError::Read {
            path: path.clone(),
            source,
        })?;
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_slice::<Vec<ExpertEntry>>(&bytes).map_err(|source| PersistError::Parse {
            path: path.clone(),
            source,
        })
    }

    /// Read the manifest and return a freshly populated [`ExpertRegistry`].
    ///
    /// `next_id` is restored as `max(entry.expert_id) + 1`, or `0` for an
    /// empty manifest (per design §4.2.1).
    pub fn load_into_registry(&self) -> Result<ExpertRegistry, PersistError> {
        let entries = self.load_entries()?;
        let mut registry = ExpertRegistry::new();
        for entry in entries {
            let info = ExpertInfo::new(
                entry.expert_id,
                entry.name.clone(),
                role_from_string(&entry.role),
                String::new(),
                String::new(),
            );
            registry.register_expert(info).map_err(|err| match err {
                crate::experts::RegistryError::DuplicateName(name) => {
                    PersistError::DuplicateName(name)
                }
                other => PersistError::DuplicateName(format!("{other}")),
            })?;
            if let Some(path) = entry.worktree_path {
                let _ = registry.update_expert_worktree(entry.expert_id, Some(path));
            }
        }
        Ok(registry)
    }

    /// Append `entry` to the manifest and atomically replace the file.
    ///
    /// Caller MUST hold `.macot/.lock` for the duration of this call.
    /// The function:
    /// 1. Reads the current array (or empty if missing).
    /// 2. Appends `entry`.
    /// 3. Writes the updated array to a sibling temp file.
    /// 4. `rename(2)` the temp file over the manifest path.
    pub fn append_atomic(&self, entry: &ExpertEntry) -> Result<(), PersistError> {
        let mut entries = self.load_entries()?;
        entries.push(entry.clone());
        self.write_atomic(&entries)
    }

    /// Remove the entry whose `expert_id` matches and atomically replace
    /// the file. No-op when the id is absent (idempotent — used for
    /// rollback in the add flow).
    ///
    /// Caller MUST hold `.macot/.lock` for the duration of this call.
    pub fn remove_by_id_atomic(&self, expert_id: ExpertId) -> Result<(), PersistError> {
        let mut entries = self.load_entries()?;
        let before = entries.len();
        entries.retain(|e| e.expert_id != expert_id);
        if entries.len() == before {
            // Nothing to do — keep on-disk state byte-identical.
            return Ok(());
        }
        self.write_atomic(&entries)
    }

    fn write_atomic(&self, entries: &[ExpertEntry]) -> Result<(), PersistError> {
        let manifest_path = self.manifest_path();
        let parent = manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&parent).map_err(|source| PersistError::Write {
            path: parent.clone(),
            source,
        })?;

        let tmp_name = format!("experts_manifest.json.tmp.{}", std::process::id());
        let tmp_path = parent.join(tmp_name);

        let json = serde_json::to_string_pretty(entries).map_err(|source| PersistError::Parse {
            path: manifest_path.clone(),
            source,
        })?;

        std::fs::write(&tmp_path, json).map_err(|source| PersistError::Write {
            path: tmp_path.clone(),
            source,
        })?;

        std::fs::rename(&tmp_path, &manifest_path).map_err(|source| PersistError::Rename {
            from: tmp_path,
            to: manifest_path,
            source,
        })
    }
}

/// Map a manifest role string to the registry's [`Role`] enum.
///
/// Builtin variants of the enum are matched case-insensitively on their
/// canonical name; everything else falls into `Role::Specialist(name)`.
fn role_from_string(s: &str) -> Role {
    match s.to_ascii_lowercase().as_str() {
        "analyst" => Role::Analyst,
        "developer" => Role::Developer,
        "reviewer" => Role::Reviewer,
        "coordinator" => Role::Coordinator,
        _ => Role::Specialist(s.to_string()),
    }
}

// `AUTO_ASSIGN_ID` is unused here directly but re-exported for callers that
// build entries before knowing the id; suppress the lint when not used.
#[allow(dead_code)]
const _: ExpertId = AUTO_ASSIGN_ID;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_entry(id: u32, name: &str, role: &str) -> ExpertEntry {
        ExpertEntry {
            expert_id: id,
            name: name.to_string(),
            role: role.to_string(),
            worktree_path: None,
        }
    }

    fn read_manifest(p: &ManifestPersistor) -> String {
        std::fs::read_to_string(p.manifest_path()).unwrap()
    }

    #[test]
    fn load_entries_empty_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        let entries = p.load_entries().unwrap();
        assert!(
            entries.is_empty(),
            "load_entries: missing file should yield empty vec"
        );
    }

    #[test]
    fn append_atomic_creates_file_and_round_trips() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());

        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();
        p.append_atomic(&make_entry(1, "Ilyusha", "planner"))
            .unwrap();
        p.append_atomic(&make_entry(2, "Grigory", "general"))
            .unwrap();

        let entries = p.load_entries().unwrap();
        assert_eq!(entries.len(), 3, "append_atomic: should have 3 entries");
        assert_eq!(
            entries.iter().map(|e| e.expert_id).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "append_atomic: insertion order preserved & matches IDs"
        );
        assert_eq!(entries[0].name, "Alyosha");
        assert_eq!(entries[1].role, "planner");
        assert!(entries[2].worktree_path.is_none());
    }

    #[test]
    fn append_atomic_uses_temp_file_and_rename() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();

        // After successful rename, no temp file should remain in the dir.
        let dir = tmp.path().join(".macot");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("experts_manifest.json.tmp.")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "append_atomic: should clean up temp file after rename, found: {leftovers:?}"
        );
    }

    #[test]
    fn write_produces_pretty_json_array() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();
        let raw = read_manifest(&p);
        // Pretty JSON has a leading "[" and a trailing "]" on their own
        // logical positions; just sanity check valid JSON shape.
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(parsed.is_array(), "write_atomic: top level should be array");
    }

    #[test]
    fn remove_by_id_is_idempotent_when_id_absent() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();
        let before = read_manifest(&p);

        // Removing a non-existent id must not change the file at all.
        p.remove_by_id_atomic(99).unwrap();
        let after = read_manifest(&p);

        assert_eq!(
            before, after,
            "remove_by_id_atomic: no-op should be byte-stable"
        );
    }

    #[test]
    fn remove_by_id_drops_only_matching_entry() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();
        p.append_atomic(&make_entry(1, "Ilyusha", "planner"))
            .unwrap();
        p.append_atomic(&make_entry(2, "Grigory", "general"))
            .unwrap();

        p.remove_by_id_atomic(1).unwrap();

        let entries = p.load_entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries.iter().map(|e| e.expert_id).collect::<Vec<_>>(),
            vec![0, 2],
            "remove_by_id_atomic: should keep order, drop only id=1"
        );
    }

    #[test]
    fn load_into_registry_restores_next_id_and_entries() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();
        p.append_atomic(&make_entry(1, "Ilyusha", "planner"))
            .unwrap();
        p.append_atomic(&make_entry(2, "Grigory", "general"))
            .unwrap();

        let mut registry = p.load_into_registry().unwrap();
        // Entries are observable.
        assert_eq!(
            registry.len(),
            3,
            "load_into_registry: should rehydrate count"
        );
        assert_eq!(registry.find_by_name("Alyosha"), Some(0));
        assert_eq!(registry.find_by_name("Grigory"), Some(2));

        // next_id is max+1: registering AUTO_ASSIGN_ID should yield 3.
        let info = ExpertInfo::new(
            AUTO_ASSIGN_ID,
            "NewOne".to_string(),
            Role::Specialist("general".to_string()),
            String::new(),
            String::new(),
        );
        let assigned = registry.register_expert(info).unwrap();
        assert_eq!(
            assigned, 3,
            "load_into_registry: next_id should be max(expert_id)+1 = 3"
        );
    }

    #[test]
    fn load_into_registry_empty_manifest_starts_at_zero() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        // No file written -> empty.

        let mut registry = p.load_into_registry().unwrap();
        assert!(registry.is_empty());
        let info = ExpertInfo::new(
            AUTO_ASSIGN_ID,
            "First".to_string(),
            Role::Developer,
            String::new(),
            String::new(),
        );
        let id = registry.register_expert(info).unwrap();
        assert_eq!(
            id, 0,
            "load_into_registry: empty manifest should start next_id at 0"
        );
    }

    #[test]
    fn load_into_registry_preserves_worktree_path() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        let mut entry = make_entry(0, "Alyosha", "architect");
        entry.worktree_path = Some("/wt/feature-auth".to_string());
        p.append_atomic(&entry).unwrap();

        let registry = p.load_into_registry().unwrap();
        assert_eq!(
            registry.get_expert(0).unwrap().worktree_path,
            Some("/wt/feature-auth".to_string()),
            "load_into_registry: worktree_path should round-trip"
        );
    }

    /// Crash-simulation: a stale `.tmp.{pid}` sitting next to the manifest
    /// must NOT alter the observable manifest. A subsequent `load_entries`
    /// returns the original content.
    #[test]
    fn dangling_tmp_file_does_not_affect_load() {
        let tmp = TempDir::new().unwrap();
        let p = ManifestPersistor::new(tmp.path());
        p.append_atomic(&make_entry(0, "Alyosha", "architect"))
            .unwrap();
        p.append_atomic(&make_entry(1, "Ilyusha", "planner"))
            .unwrap();
        let original = read_manifest(&p);

        // Drop a fake half-written tmp next to the manifest as if a prior
        // process had crashed mid-write.
        let dir = tmp.path().join(".macot");
        let stale_tmp = dir.join("experts_manifest.json.tmp.99999");
        std::fs::write(&stale_tmp, b"{ this is not even valid json").unwrap();

        let entries = p.load_entries().unwrap();
        assert_eq!(entries.len(), 2, "stale tmp must not be read as manifest");
        let after = read_manifest(&p);
        assert_eq!(
            original, after,
            "stale tmp must not alter the canonical manifest"
        );
    }

    #[test]
    fn role_from_string_maps_known_and_unknown() {
        assert_eq!(role_from_string("developer"), Role::Developer);
        assert_eq!(role_from_string("Analyst"), Role::Analyst);
        assert_eq!(role_from_string("REVIEWER"), Role::Reviewer);
        assert_eq!(role_from_string("coordinator"), Role::Coordinator);
        // Unknown roles (including the dynamic-add builtin canonical names
        // architect / planner / general) become specialists.
        assert_eq!(
            role_from_string("architect"),
            Role::Specialist("architect".to_string())
        );
        assert_eq!(
            role_from_string("Custom-Role"),
            Role::Specialist("Custom-Role".to_string())
        );
    }
}
