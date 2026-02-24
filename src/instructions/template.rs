use anyhow::{Context, Result};
use minijinja::Environment;
use std::path::Path;

use super::defaults;
use super::schema::generate_yaml_schema;

/// Result of loading instructions, including fallback information.
#[derive(Debug, Clone)]
pub struct InstructionResult {
    pub content: String,
    pub requested_role: String,
    pub used_general_fallback: bool,
    pub agents_json: Option<String>,
}

/// Render a template file with the yaml_schema, expert_id, expert_name, and status_file_path variables.
pub fn render_template(
    template_content: &str,
    expert_id: u32,
    expert_name: &str,
    status_file_path: &str,
) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("core", template_content)
        .context("Failed to add template")?;

    let template = env.get_template("core").context("Failed to get template")?;

    let yaml_schema = generate_yaml_schema();
    let rendered = template
        .render(minijinja::context! {
            yaml_schema => yaml_schema,
            expert_id => expert_id,
            expert_name => expert_name,
            status_file_path => status_file_path,
        })
        .context("Failed to render template")?;

    Ok(rendered)
}

/// Load instruction with separate paths for core and role instructions.
///
/// - `core_path`: Project's instructions folder (for core.md and templates)
/// - `role_instructions_path`: User's config folder (~/.config/macot/instructions/)
/// - `role_name`: The role to load instructions for
///
/// Fallback chain for role instructions:
/// 1. User custom: role_instructions_path/{role}.md
/// 2. Embedded default for the requested role
/// 3. "general" instructions (with toast notification)
#[allow(clippy::too_many_arguments)]
pub fn load_instruction_with_template(
    core_path: &Path,
    role_instructions_path: &Path,
    role_name: &str,
    expert_id: u32,
    expert_name: &str,
    status_file_path: &str,
    worktree_path: Option<&str>,
    manifest_path: &str,
    status_dir: &str,
) -> Result<InstructionResult> {
    let mut content = String::new();

    // Load core instructions (from project - unchanged)
    let templates_dir = core_path.join("templates");
    let core_template_path = templates_dir.join("core.md.tmpl");
    let core_legacy_path = core_path.join("core.md");

    if core_template_path.exists() {
        let template_content =
            std::fs::read_to_string(&core_template_path).context("Failed to read core template")?;
        content.push_str(&render_template(
            &template_content,
            expert_id,
            expert_name,
            status_file_path,
        )?);
        content.push_str("\n\n");
    } else if core_legacy_path.exists() {
        content.push_str(&std::fs::read_to_string(&core_legacy_path)?);
        content.push_str("\n\n");
    }

    // Load role instructions with fallback chain
    let (role_content, used_general_fallback) =
        load_role_instruction(role_instructions_path, role_name);

    content.push_str(&role_content);

    // Render agent templates (for --agents CLI flag)
    let agents_json = super::agents::render_agents_json(
        core_path,
        expert_id,
        expert_name,
        worktree_path,
        manifest_path,
        status_dir,
    )?;

    Ok(InstructionResult {
        content,
        requested_role: role_name.to_string(),
        used_general_fallback,
        agents_json,
    })
}

/// Load role instruction with fallback chain.
/// Returns (content, used_general_fallback)
fn load_role_instruction(role_instructions_path: &Path, role_name: &str) -> (String, bool) {
    // 1. Try user custom instruction
    let user_path = role_instructions_path.join(format!("{role_name}.md"));
    if user_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&user_path) {
            return (content, false);
        }
    }

    // 2. Try embedded default for requested role
    if let Some(default_content) = defaults::get_default(role_name) {
        return (default_content.to_string(), false);
    }

    // 3. Fallback to "general" - first try user's general.md
    let general_user_path = role_instructions_path.join("general.md");
    if general_user_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&general_user_path) {
            return (content, true);
        }
    }

    // 4. Embedded general as last resort
    let general_default = defaults::get_default("general").unwrap_or("");
    (general_default.to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn render_template_replaces_yaml_schema() {
        let template = "## Report Format\n\n```yaml\n{{ yaml_schema }}```\n";
        let rendered = render_template(template, 0, "test", "/tmp/status/expert0").unwrap();

        assert!(rendered.contains("task_id:"));
        assert!(rendered.contains("expert_id:"));
        assert!(rendered.contains("status:"));
        assert!(!rendered.contains("{{ yaml_schema }}"));
    }

    #[test]
    fn render_template_preserves_surrounding_text() {
        let template = "# Header\n\nSome text before.\n\n{{ yaml_schema }}\n\nSome text after.";
        let rendered = render_template(template, 0, "test", "/tmp/status/expert0").unwrap();

        assert!(rendered.contains("# Header"));
        assert!(rendered.contains("Some text before."));
        assert!(rendered.contains("Some text after."));
    }

    #[test]
    fn render_core_produces_valid_markdown() {
        let template = r#"# Multi-Agent Control Tower - Core Instructions

## Report Format

**IMPORTANT**: Your report MUST follow this exact YAML schema.

```yaml
{{ yaml_schema }}```

**Critical Notes**:
- `status` must be exactly `done`
"#;
        let rendered = render_template(template, 0, "test", "/tmp/status/expert0").unwrap();

        assert!(rendered.contains("# Multi-Agent Control Tower"));
        assert!(rendered.contains("task_id:"));
        assert!(rendered.contains("**Critical Notes**"));
    }

    #[test]
    fn load_instruction_uses_embedded_default() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        assert!(!result.content.is_empty());
        assert_eq!(result.requested_role, "architect");
        assert!(!result.used_general_fallback);
    }

    #[test]
    fn load_instruction_uses_user_custom() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        std::fs::write(
            role_dir.path().join("architect.md"),
            "# Custom Architect\n\nCustom content",
        )
        .unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        assert!(result.content.contains("Custom Architect"));
        assert_eq!(result.requested_role, "architect");
        assert!(!result.used_general_fallback);
    }

    #[test]
    fn load_instruction_falls_back_to_general() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "unknown-role",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        assert!(!result.content.is_empty());
        assert_eq!(result.requested_role, "unknown-role");
        assert!(result.used_general_fallback);
    }

    #[test]
    fn load_instruction_includes_core() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        std::fs::write(core_dir.path().join("core.md"), "# Core Instructions").unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        assert!(result.content.contains("Core Instructions"));
    }

    #[test]
    fn render_template_replaces_expert_identity() {
        let template = "You are **{{ expert_name }}** (Expert ID: {{ expert_id }}).";
        let rendered = render_template(template, 3, "Alyosha", "/tmp/status/expert3").unwrap();

        assert!(rendered.contains("You are **Alyosha** (Expert ID: 3)."));
        assert!(!rendered.contains("{{ expert_name }}"));
        assert!(!rendered.contains("{{ expert_id }}"));
    }

    #[test]
    fn render_template_replaces_status_file_path() {
        let template = "Write status to: {{ status_file_path }}";
        let rendered =
            render_template(template, 0, "test", "/tmp/project/.macot/status/expert0").unwrap();

        assert!(rendered.contains("/tmp/project/.macot/status/expert0"));
        assert!(!rendered.contains("{{ status_file_path }}"));
    }

    #[test]
    fn instruction_result_includes_agents_json() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let agents_dir = core_dir.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("messaging.md.tmpl"),
            "from_expert_id: {{ expert_id }}",
        )
        .unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            3,
            "TestExpert",
            "/tmp/status/expert3",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        assert!(
            result.agents_json.is_some(),
            "instruction_result: agents_json should be Some when agent templates exist"
        );
        let json: serde_json::Value =
            serde_json::from_str(result.agents_json.as_ref().unwrap()).unwrap();
        assert!(
            json["messaging"]["prompt"]
                .as_str()
                .unwrap()
                .contains("from_expert_id: 3"),
            "instruction_result: agents_json should contain rendered expert_id"
        );
    }

    #[test]
    fn instruction_result_agents_json_none_without_template() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        assert!(
            result.agents_json.is_none(),
            "instruction_result: agents_json should be None when no agent templates exist"
        );
    }

    #[test]
    fn load_instruction_passes_manifest_path_to_agents() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let agents_dir = core_dir.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "manifest={{ manifest_path }}",
        )
        .unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/custom/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        let json: serde_json::Value =
            serde_json::from_str(result.agents_json.as_ref().unwrap()).unwrap();
        let prompt = json["expert-discovery"]["prompt"].as_str().unwrap();
        assert!(
            prompt.contains("/custom/manifest.json"),
            "load_instruction: should pass manifest_path to agents renderer, got: {}",
            prompt
        );
    }

    #[test]
    fn load_instruction_passes_status_dir_to_agents() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let agents_dir = core_dir.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "status={{ status_dir }}",
        )
        .unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            None,
            "/tmp/manifest.json",
            "/custom/status/dir",
        )
        .unwrap();

        let json: serde_json::Value =
            serde_json::from_str(result.agents_json.as_ref().unwrap()).unwrap();
        let prompt = json["expert-discovery"]["prompt"].as_str().unwrap();
        assert!(
            prompt.contains("/custom/status/dir"),
            "load_instruction: should pass status_dir to agents renderer, got: {}",
            prompt
        );
    }

    #[test]
    fn load_instruction_passes_worktree_path_to_agents() {
        let core_dir = TempDir::new().unwrap();
        let role_dir = TempDir::new().unwrap();

        let agents_dir = core_dir.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "wt={{ worktree_path }}",
        )
        .unwrap();

        let result = load_instruction_with_template(
            core_dir.path(),
            role_dir.path(),
            "architect",
            0,
            "test",
            "/tmp/status/expert0",
            Some("/wt/my-feature"),
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();

        let json: serde_json::Value =
            serde_json::from_str(result.agents_json.as_ref().unwrap()).unwrap();
        let prompt = json["expert-discovery"]["prompt"].as_str().unwrap();
        assert!(
            prompt.contains("/wt/my-feature"),
            "load_instruction: should pass worktree_path to agents renderer, got: {}",
            prompt
        );
    }
}
