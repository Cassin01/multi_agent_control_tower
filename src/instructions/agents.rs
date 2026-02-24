use anyhow::{Context, Result};
use minijinja::Environment;
use std::path::Path;

/// Render agent template files into a JSON string for the `--agents` CLI flag.
///
/// Looks for `templates/agents/messaging.md.tmpl` and `templates/agents/expert-discovery.md.tmpl`
/// under `core_path`. Returns `Ok(None)` if no agent templates exist.
pub fn render_agents_json(
    core_path: &Path,
    expert_id: u32,
    expert_name: &str,
    worktree_path: Option<&str>,
    manifest_path: &str,
    status_dir: &str,
) -> Result<Option<String>> {
    let agents_dir = core_path.join("templates").join("agents");

    let messaging_path = agents_dir.join("messaging.md.tmpl");
    let discovery_path = agents_dir.join("expert-discovery.md.tmpl");

    if !messaging_path.exists() && !discovery_path.exists() {
        return Ok(None);
    }

    let mut json = serde_json::Map::new();

    if messaging_path.exists() {
        let template_content = std::fs::read_to_string(&messaging_path)
            .context("Failed to read messaging agent template")?;
        let rendered = render_messaging_template(&template_content, expert_id, expert_name)?;

        let description = "Send messages to other experts through the MACOT messaging system. \
                            Use this agent when you need to coordinate, ask questions, \
                            or delegate tasks to other experts.";

        json.insert(
            "messaging".to_string(),
            serde_json::json!({
                "description": description,
                "prompt": rendered
            }),
        );
    }

    if discovery_path.exists() {
        let template_content = std::fs::read_to_string(&discovery_path)
            .context("Failed to read expert-discovery agent template")?;
        let rendered = render_discovery_template(
            &template_content,
            expert_id,
            expert_name,
            worktree_path,
            manifest_path,
            status_dir,
        )?;

        let description = "Query information about other experts in your worktree: \
                            their IDs, names, roles, and current status (idle/busy).";

        json.insert(
            "expert-discovery".to_string(),
            serde_json::json!({
                "description": description,
                "prompt": rendered
            }),
        );
    }

    if json.is_empty() {
        return Ok(None);
    }

    Ok(Some(
        serde_json::to_string(&serde_json::Value::Object(json))
            .context("Failed to serialize agents JSON")?,
    ))
}

fn render_discovery_template(
    template_content: &str,
    expert_id: u32,
    expert_name: &str,
    worktree_path: Option<&str>,
    manifest_path: &str,
    status_dir: &str,
) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("discovery", template_content)
        .context("Failed to add expert-discovery template")?;

    let template = env
        .get_template("discovery")
        .context("Failed to get expert-discovery template")?;

    let wt_display = worktree_path.unwrap_or("null");

    let rendered = template
        .render(minijinja::context! {
            expert_id => expert_id,
            expert_name => expert_name,
            worktree_path => wt_display,
            manifest_path => manifest_path,
            status_dir => status_dir,
        })
        .context("Failed to render expert-discovery template")?;

    Ok(rendered)
}

fn render_messaging_template(
    template_content: &str,
    expert_id: u32,
    expert_name: &str,
) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("messaging", template_content)
        .context("Failed to add messaging template")?;

    let template = env
        .get_template("messaging")
        .context("Failed to get messaging template")?;

    let rendered = template
        .render(minijinja::context! {
            expert_id => expert_id,
            expert_name => expert_name,
        })
        .context("Failed to render messaging template")?;

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn render_agents_json_returns_none_when_no_template() {
        let tmp = TempDir::new().unwrap();
        let result = render_agents_json(
            tmp.path(),
            0,
            "test",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();
        assert!(
            result.is_none(),
            "render_agents_json: should return None when no agent templates exist"
        );
    }

    #[test]
    fn render_agents_json_returns_valid_json_with_template() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("messaging.md.tmpl"),
            "Send a message from expert {{ expert_id }}.",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            2,
            "Alyosha",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap();
        assert!(
            result.is_some(),
            "render_agents_json: should return Some when template exists"
        );

        let json: serde_json::Value = serde_json::from_str(result.as_ref().unwrap()).unwrap();
        assert!(
            json.get("messaging").is_some(),
            "render_agents_json: JSON should have 'messaging' key"
        );
        assert!(
            json["messaging"]["description"].is_string(),
            "render_agents_json: messaging should have 'description' string"
        );
        assert!(
            json["messaging"]["prompt"].is_string(),
            "render_agents_json: messaging should have 'prompt' string"
        );
    }

    #[test]
    fn render_agents_json_renders_expert_id() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("messaging.md.tmpl"),
            "from_expert_id: {{ expert_id }}",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            5,
            "TestExpert",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap()
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let prompt = json["messaging"]["prompt"].as_str().unwrap();

        assert!(
            prompt.contains("from_expert_id: 5"),
            "render_agents_json: should render expert_id in template, got: {}",
            prompt
        );
        assert!(
            !prompt.contains("{{ expert_id }}"),
            "render_agents_json: should not contain unrendered template variable"
        );
    }

    #[test]
    fn render_agents_json_includes_discovery_agent() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "Manifest: {{ manifest_path }}, Status: {{ status_dir }}",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            0,
            "Alyosha",
            None,
            "/tmp/.macot/experts_manifest.json",
            "/tmp/.macot/status",
        )
        .unwrap();
        assert!(
            result.is_some(),
            "render_agents_json: should return Some when discovery template exists"
        );

        let json: serde_json::Value = serde_json::from_str(result.as_ref().unwrap()).unwrap();
        assert!(
            json.get("expert-discovery").is_some(),
            "render_agents_json: JSON should have 'expert-discovery' key"
        );
        assert!(
            json["expert-discovery"]["description"].is_string(),
            "render_agents_json: expert-discovery should have 'description' string"
        );
        assert!(
            json["expert-discovery"]["prompt"].is_string(),
            "render_agents_json: expert-discovery should have 'prompt' string"
        );
    }

    #[test]
    fn render_agents_json_discovery_absent_without_template() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("messaging.md.tmpl"),
            "msg from {{ expert_id }}",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            0,
            "test",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap()
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert!(
            json.get("messaging").is_some(),
            "render_agents_json: should have messaging when only messaging template exists"
        );
        assert!(
            json.get("expert-discovery").is_none(),
            "render_agents_json: should not have expert-discovery when no discovery template"
        );
    }

    #[test]
    fn render_agents_json_discovery_renders_manifest_path() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "path={{ manifest_path }}",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            0,
            "test",
            None,
            "/custom/path/manifest.json",
            "/tmp/status",
        )
        .unwrap()
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let prompt = json["expert-discovery"]["prompt"].as_str().unwrap();

        assert!(
            prompt.contains("/custom/path/manifest.json"),
            "render_agents_json: discovery prompt should contain rendered manifest_path, got: {}",
            prompt
        );
    }

    #[test]
    fn render_agents_json_discovery_renders_status_dir() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "dir={{ status_dir }}",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            0,
            "test",
            None,
            "/tmp/manifest.json",
            "/custom/status/dir",
        )
        .unwrap()
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let prompt = json["expert-discovery"]["prompt"].as_str().unwrap();

        assert!(
            prompt.contains("/custom/status/dir"),
            "render_agents_json: discovery prompt should contain rendered status_dir, got: {}",
            prompt
        );
    }

    #[test]
    fn render_agents_json_discovery_renders_worktree_path() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("templates").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("expert-discovery.md.tmpl"),
            "wt={{ worktree_path }}",
        )
        .unwrap();

        let result = render_agents_json(
            tmp.path(),
            0,
            "test",
            Some("/wt/feature-auth"),
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap()
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let prompt = json["expert-discovery"]["prompt"].as_str().unwrap();

        assert!(
            prompt.contains("/wt/feature-auth"),
            "render_agents_json: discovery prompt should contain worktree_path, got: {}",
            prompt
        );

        // Test null worktree_path
        let result_null = render_agents_json(
            tmp.path(),
            0,
            "test",
            None,
            "/tmp/manifest.json",
            "/tmp/status",
        )
        .unwrap()
        .unwrap();
        let json_null: serde_json::Value = serde_json::from_str(&result_null).unwrap();
        let prompt_null = json_null["expert-discovery"]["prompt"].as_str().unwrap();

        assert!(
            prompt_null.contains("null"),
            "render_agents_json: discovery prompt should render 'null' for None worktree_path, got: {}",
            prompt_null
        );
    }
}
