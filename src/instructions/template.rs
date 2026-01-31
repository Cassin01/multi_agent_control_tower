use anyhow::{Context, Result};
use minijinja::Environment;
use std::path::Path;

use super::schema::generate_yaml_schema;

/// Render a template file with the yaml_schema variable.
pub fn render_template(template_content: &str) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("core", template_content)
        .context("Failed to add template")?;

    let template = env.get_template("core").context("Failed to get template")?;

    let yaml_schema = generate_yaml_schema();
    let rendered = template
        .render(minijinja::context! {
            yaml_schema => yaml_schema
        })
        .context("Failed to render template")?;

    Ok(rendered)
}

/// Load instruction from templates directory if available, otherwise use legacy format.
pub fn load_instruction_with_template(
    instructions_path: &Path,
    expert_name: &str,
) -> Result<String> {
    let templates_dir = instructions_path.join("templates");
    let core_template_path = templates_dir.join("core.md.tmpl");
    let core_legacy_path = instructions_path.join("core.md");
    let expert_path = instructions_path.join(format!("{}.md", expert_name));

    let mut instruction = String::new();

    if core_template_path.exists() {
        let template_content =
            std::fs::read_to_string(&core_template_path).context("Failed to read core template")?;
        instruction.push_str(&render_template(&template_content)?);
        instruction.push_str("\n\n");
    } else if core_legacy_path.exists() {
        instruction.push_str(&std::fs::read_to_string(&core_legacy_path)?);
        instruction.push_str("\n\n");
    }

    if expert_path.exists() {
        instruction.push_str(&std::fs::read_to_string(&expert_path)?);
    }

    Ok(instruction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_replaces_yaml_schema() {
        let template = "## Report Format\n\n```yaml\n{{ yaml_schema }}```\n";
        let rendered = render_template(template).unwrap();

        assert!(rendered.contains("task_id:"));
        assert!(rendered.contains("expert_id:"));
        assert!(rendered.contains("status:"));
        assert!(!rendered.contains("{{ yaml_schema }}"));
    }

    #[test]
    fn render_template_preserves_surrounding_text() {
        let template = "# Header\n\nSome text before.\n\n{{ yaml_schema }}\n\nSome text after.";
        let rendered = render_template(template).unwrap();

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
        let rendered = render_template(template).unwrap();

        assert!(rendered.contains("# Multi-Agent Control Tower"));
        assert!(rendered.contains("task_id:"));
        assert!(rendered.contains("**Critical Notes**"));
    }
}
