use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::context::SessionExpertRoles;
use crate::experts::persist::ExpertEntry;
use crate::experts::ExpertRegistry;

/// Entry in the expert manifest file.
///
/// Describes a single expert for discovery by other agents.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ExpertManifestEntry {
    pub expert_id: u32,
    pub name: String,
    pub role: String,
    pub worktree_path: Option<String>,
}

/// Generate manifest JSON from config, session expert roles, registry, and the
/// entries currently on disk.
///
/// The roster is the union of three sources, because dynamically added experts
/// (F2 modal / `macot expert add`) exist in the registry and on disk but never
/// in the startup `config` snapshot:
/// - `config` — the startup snapshot (ids `0..num_experts`).
/// - `registry` — the runtime source of truth, includes dynamic experts.
/// - `existing` — whatever is on disk, so an entry this process has not
///   reloaded yet is preserved instead of clobbered.
///
/// Per field: `session_roles` overrides win, then config defaults (config ids
/// only), then registry, then disk. `worktree_path` comes from the registry
/// whenever it knows the expert (so clearing a worktree sticks), otherwise from
/// disk.
pub fn generate_expert_manifest(
    config: &Config,
    session_roles: &SessionExpertRoles,
    registry: &ExpertRegistry,
    existing: &[ExpertEntry],
) -> Result<String> {
    let mut ids: Vec<u32> = (0..config.num_experts())
        .chain(registry.get_all_experts().iter().map(|info| info.id))
        .chain(existing.iter().map(|e| e.expert_id))
        .collect();
    ids.sort_unstable();
    ids.dedup();

    let entries: Vec<ExpertManifestEntry> = ids
        .into_iter()
        .map(|id| {
            let in_config = config.get_expert(id).is_some();
            let from_registry = registry.get_expert(id);
            let from_disk = existing.iter().find(|e| e.expert_id == id);

            let name = if in_config {
                config.get_expert_name(id)
            } else {
                from_registry
                    .map(|info| info.name.clone())
                    .or_else(|| from_disk.map(|e| e.name.clone()))
                    .unwrap_or_else(|| format!("expert{id}"))
            };

            let role = session_roles
                .get_role(id)
                .map(|r| r.to_string())
                .or_else(|| in_config.then(|| config.get_expert_role(id)))
                .or_else(|| from_registry.map(|info| info.role.as_str().to_string()))
                .or_else(|| from_disk.map(|e| e.role.clone()))
                .unwrap_or_else(|| "general".to_string());

            let worktree_path = match from_registry {
                Some(info) => info.worktree_path.clone(),
                None => from_disk.and_then(|e| e.worktree_path.clone()),
            };

            ExpertManifestEntry {
                expert_id: id,
                name,
                role,
                worktree_path,
            }
        })
        .collect();

    let json = serde_json::to_string_pretty(&entries)?;
    Ok(json)
}

/// Write manifest to `.macot/experts_manifest.json`.
///
/// Overwrites existing file if present.
pub fn write_expert_manifest(queue_path: &Path, content: &str) -> Result<PathBuf> {
    let manifest_path = queue_path.join("experts_manifest.json");
    std::fs::write(&manifest_path, content)?;
    Ok(manifest_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ExpertInfo, Role};
    use tempfile::TempDir;

    fn make_config(experts: Vec<(&str, &str)>) -> Config {
        use crate::config::ExpertConfig;
        let mut config = Config::default();
        config.experts = experts
            .into_iter()
            .map(|(name, role)| ExpertConfig {
                name: name.to_string(),
                role: role.to_string(),
            })
            .collect();
        config
    }

    fn make_session_roles() -> SessionExpertRoles {
        SessionExpertRoles::new("test-hash".to_string())
    }

    // --- Task 1.1: Tests for generate_expert_manifest ---

    #[test]
    fn generate_manifest_empty_config() {
        let config = make_config(vec![]);
        let roles = make_session_roles();
        let registry = ExpertRegistry::new();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert!(
            entries.is_empty(),
            "generate_manifest_empty_config: empty config should produce empty manifest"
        );
        assert_eq!(
            json.trim(),
            "[]",
            "generate_manifest_empty_config: should produce valid empty JSON array"
        );
    }

    #[test]
    fn generate_manifest_includes_all_experts() {
        let config = make_config(vec![
            ("Alyosha", "architect"),
            ("Dmitri", "developer"),
            ("Katya", "debugger"),
        ]);
        let roles = make_session_roles();
        let registry = ExpertRegistry::new();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries.len(),
            3,
            "generate_manifest_includes_all_experts: should include all 3 experts"
        );
        assert_eq!(entries[0].name, "Alyosha");
        assert_eq!(entries[1].name, "Dmitri");
        assert_eq!(entries[2].name, "Katya");
        assert_eq!(entries[0].expert_id, 0);
        assert_eq!(entries[1].expert_id, 1);
        assert_eq!(entries[2].expert_id, 2);
    }

    #[test]
    fn generate_manifest_uses_config_roles_by_default() {
        let config = make_config(vec![("Alyosha", "architect"), ("Dmitri", "developer")]);
        let roles = make_session_roles();
        let registry = ExpertRegistry::new();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[0].role, "architect",
            "generate_manifest: should use config role when no session override"
        );
        assert_eq!(entries[1].role, "developer");
    }

    #[test]
    fn generate_manifest_uses_session_roles() {
        let config = make_config(vec![("Alyosha", "architect"), ("Dmitri", "developer")]);
        let mut roles = make_session_roles();
        roles.set_role(0, "frontend".to_string());

        let registry = ExpertRegistry::new();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[0].role, "frontend",
            "generate_manifest_uses_session_roles: session role should override config"
        );
        assert_eq!(
            entries[1].role, "developer",
            "generate_manifest_uses_session_roles: non-overridden role should use config default"
        );
    }

    #[test]
    fn generate_manifest_includes_worktree_paths() {
        let config = make_config(vec![("Alyosha", "architect"), ("Dmitri", "developer")]);
        let roles = make_session_roles();

        let mut registry = ExpertRegistry::new();
        let expert0 = ExpertInfo::new(
            0,
            "Alyosha".to_string(),
            Role::specialist("architect"),
            "session".to_string(),
            "0".to_string(),
        );
        let expert1 = ExpertInfo::new(
            1,
            "Dmitri".to_string(),
            Role::Developer,
            "session".to_string(),
            "1".to_string(),
        );
        registry.register_expert(expert0).unwrap();
        registry.register_expert(expert1).unwrap();
        registry
            .update_expert_worktree(0, Some("/wt/feature-auth".to_string()))
            .unwrap();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[0].worktree_path,
            Some("/wt/feature-auth".to_string()),
            "generate_manifest_includes_worktree_paths: should include worktree_path from registry"
        );
        assert_eq!(
            entries[1].worktree_path, None,
            "generate_manifest_includes_worktree_paths: None worktree should stay None"
        );
    }

    #[test]
    fn generate_manifest_is_valid_json() {
        let config = make_config(vec![("Alyosha", "architect")]);
        let roles = make_session_roles();
        let registry = ExpertRegistry::new();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("generate_manifest_is_valid_json: output should be valid JSON");

        assert!(
            parsed.is_array(),
            "generate_manifest_is_valid_json: root should be a JSON array"
        );
    }

    #[test]
    fn generate_manifest_worktree_none_when_expert_not_in_registry() {
        let config = make_config(vec![("Alyosha", "architect")]);
        let roles = make_session_roles();
        let registry = ExpertRegistry::new(); // Empty registry

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[0].worktree_path, None,
            "generate_manifest: expert not in registry should have None worktree_path"
        );
    }

    #[test]
    fn generate_manifest_preserves_dynamically_added_expert() {
        // Expert 1 was added at runtime (F2 modal / `macot expert add`), so it
        // lives in the registry but not in the startup config snapshot.
        let config = make_config(vec![("Alyosha", "architect")]);
        let roles = make_session_roles();
        let mut registry = ExpertRegistry::new();
        registry
            .register_expert(ExpertInfo::new(
                0,
                "Alyosha".to_string(),
                Role::specialist("architect"),
                "session".to_string(),
                "0".to_string(),
            ))
            .unwrap();
        registry
            .register_expert(ExpertInfo::new(
                1,
                "Nova".to_string(),
                Role::specialist("planner"),
                "session".to_string(),
                "1".to_string(),
            ))
            .unwrap();

        let json = generate_expert_manifest(&config, &roles, &registry, &[]).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries.len(),
            2,
            "generate_manifest_preserves_dynamically_added_expert: registry-only expert must survive regeneration"
        );
        assert_eq!(entries[1].expert_id, 1);
        assert_eq!(entries[1].name, "Nova");
        assert_eq!(entries[1].role, "planner");
    }

    #[test]
    fn generate_manifest_preserves_manifest_only_expert() {
        // Expert 1 was committed to disk by another process; this process has
        // not reloaded it into its registry yet.
        let config = make_config(vec![("Alyosha", "architect")]);
        let roles = make_session_roles();
        let existing = vec![ExpertEntry {
            expert_id: 1,
            name: "Nova".to_string(),
            role: "planner".to_string(),
            worktree_path: Some("/wt/nova".to_string()),
        }];

        let json =
            generate_expert_manifest(&config, &roles, &ExpertRegistry::new(), &existing).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries.len(),
            2,
            "generate_manifest_preserves_manifest_only_expert: on-disk entry must not be dropped"
        );
        assert_eq!(entries[1].name, "Nova");
        assert_eq!(entries[1].role, "planner");
        assert_eq!(entries[1].worktree_path, Some("/wt/nova".to_string()));
    }

    #[test]
    fn generate_manifest_dynamic_expert_honours_session_role() {
        let config = make_config(vec![("Alyosha", "architect")]);
        let mut roles = make_session_roles();
        roles.set_role(1, "debugger".to_string());
        let existing = vec![ExpertEntry {
            expert_id: 1,
            name: "Nova".to_string(),
            role: "planner".to_string(),
            worktree_path: None,
        }];

        let json =
            generate_expert_manifest(&config, &roles, &ExpertRegistry::new(), &existing).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[1].role, "debugger",
            "generate_manifest_dynamic_expert_honours_session_role: session override wins for dynamic experts"
        );
    }

    #[test]
    fn generate_manifest_registry_clears_worktree_of_known_expert() {
        // Registry knows expert 0 and says "no worktree" — that must win over
        // a stale on-disk path (worktree return / Ctrl+W toggle off).
        let config = make_config(vec![("Alyosha", "architect")]);
        let roles = make_session_roles();
        let mut registry = ExpertRegistry::new();
        registry
            .register_expert(ExpertInfo::new(
                0,
                "Alyosha".to_string(),
                Role::specialist("architect"),
                "session".to_string(),
                "0".to_string(),
            ))
            .unwrap();
        let existing = vec![ExpertEntry {
            expert_id: 0,
            name: "Alyosha".to_string(),
            role: "architect".to_string(),
            worktree_path: Some("/wt/stale".to_string()),
        }];

        let json = generate_expert_manifest(&config, &roles, &registry, &existing).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[0].worktree_path, None,
            "generate_manifest_registry_clears_worktree_of_known_expert: registry is authoritative for registered experts"
        );
    }

    #[test]
    fn generate_manifest_sorts_and_dedupes_ids() {
        let config = make_config(vec![("Alyosha", "architect")]);
        let roles = make_session_roles();
        let mut registry = ExpertRegistry::new();
        registry
            .register_expert(ExpertInfo::new(
                7,
                "Nova".to_string(),
                Role::specialist("planner"),
                "session".to_string(),
                "7".to_string(),
            ))
            .unwrap();
        let existing = vec![
            ExpertEntry {
                expert_id: 7,
                name: "Nova".to_string(),
                role: "planner".to_string(),
                worktree_path: None,
            },
            ExpertEntry {
                expert_id: 3,
                name: "Mira".to_string(),
                role: "general".to_string(),
                worktree_path: None,
            },
        ];

        let json = generate_expert_manifest(&config, &roles, &registry, &existing).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        let ids: Vec<u32> = entries.iter().map(|e| e.expert_id).collect();
        assert_eq!(
            ids,
            vec![0, 3, 7],
            "generate_manifest_sorts_and_dedupes_ids: ids should be unique and ascending"
        );
    }

    // --- Task 2.1: Tests for write_expert_manifest ---

    #[test]
    fn write_manifest_creates_file() {
        let tmp = TempDir::new().unwrap();
        let content =
            r#"[{"expert_id":0,"name":"Alyosha","role":"architect","worktree_path":null}]"#;

        let path = write_expert_manifest(tmp.path(), content).unwrap();

        assert!(
            path.exists(),
            "write_manifest_creates_file: file should exist at expected path"
        );
        assert_eq!(
            path,
            tmp.path().join("experts_manifest.json"),
            "write_manifest_creates_file: path should be queue_path/experts_manifest.json"
        );

        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, content,
            "write_manifest_creates_file: content should match what was written"
        );
    }

    #[test]
    fn write_manifest_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let old_content = r#"[{"expert_id":0,"name":"Old","role":"old","worktree_path":null}]"#;
        let new_content = r#"[{"expert_id":0,"name":"New","role":"new","worktree_path":null}]"#;

        write_expert_manifest(tmp.path(), old_content).unwrap();
        let path = write_expert_manifest(tmp.path(), new_content).unwrap();

        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, new_content,
            "write_manifest_overwrites_existing: should overwrite with new content"
        );
    }

    // --- Task 4.1: Content tests for expert-discovery template ---

    fn read_discovery_template() -> String {
        let template_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("instructions")
            .join("templates")
            .join("agents")
            .join("expert-discovery.md.tmpl");
        std::fs::read_to_string(&template_path).expect("expert-discovery.md.tmpl should exist")
    }

    #[test]
    fn discovery_template_contains_manifest_path_variable() {
        let content = read_discovery_template();
        assert!(
            content.contains("{{ manifest_path }}"),
            "discovery_template: should contain manifest_path template variable"
        );
    }

    #[test]
    fn discovery_template_contains_status_dir_variable() {
        let content = read_discovery_template();
        assert!(
            content.contains("{{ status_dir }}"),
            "discovery_template: should contain status_dir template variable"
        );
    }

    #[test]
    fn discovery_template_contains_worktree_path_variable() {
        let content = read_discovery_template();
        assert!(
            content.contains("{{ worktree_path }}"),
            "discovery_template: should contain worktree_path template variable"
        );
    }

    #[test]
    fn discovery_template_contains_worktree_filter_instruction() {
        let content = read_discovery_template();
        assert!(
            content.contains("worktree_path"),
            "discovery_template: should contain worktree filtering instructions"
        );
        assert!(
            content.contains("same") || content.contains("share") || content.contains("match"),
            "discovery_template: should instruct filtering by matching worktree"
        );
    }

    #[test]
    fn discovery_template_is_read_only() {
        let content = read_discovery_template();
        assert!(
            content.contains("read-only") || content.contains("read only"),
            "discovery_template: should instruct read-only behavior"
        );
        assert!(
            content.contains("never write")
                || content.contains("must not write")
                || content.contains("never modify"),
            "discovery_template: should explicitly prohibit writes"
        );
    }

    #[test]
    fn discovery_template_contains_expert_id_variable() {
        let content = read_discovery_template();
        assert!(
            content.contains("{{ expert_id }}"),
            "discovery_template: should contain expert_id template variable"
        );
    }

    #[test]
    fn discovery_template_contains_expert_name_variable() {
        let content = read_discovery_template();
        assert!(
            content.contains("{{ expert_name }}"),
            "discovery_template: should contain expert_name template variable"
        );
    }
}
