use crate::models::Report;

/// Generate YAML schema with documentation comments.
pub fn generate_yaml_schema() -> String {
    let base_schema = Report::sample_yaml_schema();

    let mut annotated = String::new();
    for line in base_schema.lines() {
        if line.starts_with("status:") {
            annotated.push_str(line);
            annotated.push_str("  # MUST be: pending | in_progress | done | failed");
        } else if line.contains("severity:") {
            annotated.push_str(line);
            annotated.push_str("  # low | medium | high | critical");
        } else {
            annotated.push_str(line);
        }
        annotated.push('\n');
    }

    annotated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_schema_is_valid_yaml() {
        let schema = generate_yaml_schema();
        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&schema);
        assert!(parsed.is_ok(), "Generated schema should be valid YAML");
    }

    #[test]
    fn generated_schema_contains_status_comment() {
        let schema = generate_yaml_schema();
        assert!(schema.contains("# MUST be: pending | in_progress | done | failed"));
    }

    #[test]
    fn generated_schema_contains_severity_comment() {
        let schema = generate_yaml_schema();
        assert!(schema.contains("# low | medium | high | critical"));
    }
}
