use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::context::SessionExpertRoles;
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

/// Generate manifest JSON from config, session expert roles, and registry.
///
/// Iterates over all experts in the config. For each expert:
/// - Uses `session_roles` for the current role assignment (falls back to config default).
/// - Uses `registry` for the `worktree_path` field from `ExpertInfo`.
pub fn generate_expert_manifest(
    config: &Config,
    session_roles: &SessionExpertRoles,
    registry: &ExpertRegistry,
) -> Result<String> {
    let entries: Vec<ExpertManifestEntry> = (0..config.num_experts())
        .map(|id| {
            let name = config.get_expert_name(id);

            // Session roles take precedence over config defaults
            let role = session_roles
                .get_role(id)
                .map(|r| r.to_string())
                .unwrap_or_else(|| config.get_expert_role(id));

            let worktree_path = registry
                .get_expert(id)
                .and_then(|info| info.worktree_path.clone());

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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
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

        let json = generate_expert_manifest(&config, &roles, &registry).unwrap();
        let entries: Vec<ExpertManifestEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            entries[0].worktree_path, None,
            "generate_manifest: expert not in registry should have None worktree_path"
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
