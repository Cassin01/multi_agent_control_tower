use anyhow::{Context, Result};
use minijinja::Environment;
use std::path::Path;

/// Render agent template files into a JSON string for the `--agents` CLI flag.
///
/// Looks for `templates/agents/messaging.md.tmpl` under `core_path`.
/// Returns `Ok(None)` if no agent templates exist.
pub fn render_agents_json(
    core_path: &Path,
    expert_id: u32,
    expert_name: &str,
) -> Result<Option<String>> {
    let template_path = core_path
        .join("templates")
        .join("agents")
        .join("messaging.md.tmpl");

    if !template_path.exists() {
        return Ok(None);
    }

    let template_content =
        std::fs::read_to_string(&template_path).context("Failed to read messaging agent template")?;

    let rendered = render_messaging_template(&template_content, expert_id, expert_name)?;

    let description = "Send messages to other experts through the MACOT messaging system. \
                        Use this agent when you need to coordinate, ask questions, \
                        or delegate tasks to other experts.";

    let json = serde_json::json!({
        "messaging": {
            "description": description,
            "prompt": rendered
        }
    });

    Ok(Some(serde_json::to_string(&json).context("Failed to serialize agents JSON")?))
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
        let result = render_agents_json(tmp.path(), 0, "test").unwrap();
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

        let result = render_agents_json(tmp.path(), 2, "Alyosha").unwrap();
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

        let result = render_agents_json(tmp.path(), 5, "TestExpert").unwrap().unwrap();
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
}
