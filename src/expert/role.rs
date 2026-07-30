//! Role resolver for dynamic expert add.
//!
//! Translates a [`RoleSpec`] into a [`ResolvedRole`] containing the
//! canonical role name plus the system-prompt body and settings template
//! that the spawn flow will materialise into the per-expert files.
//!
//! Builtin roles embed their prompt body via the existing
//! [`crate::instructions::defaults`] table. Custom roles search the
//! project-local template directory first, then the user-global
//! configuration directory, mirroring the convention used elsewhere by
//! macot's instructions loader.
//!
//! See design doc §3.3 for the contract.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use thiserror::Error;

use crate::instructions::defaults;

/// User-supplied role description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleSpec {
    /// One of the builtin role categories with embedded prompt content.
    Builtin(BuiltinRole),
    /// External template referenced by name.
    ///
    /// Resolution order:
    /// 1. `<project_root>/.macot/templates/roles/{name}.md`
    /// 2. `~/.config/macot/roles/{name}.md`
    Custom { name: String },
}

/// Builtin role categories enumerated for the dynamic-add CLI surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinRole {
    Architect,
    Planner,
    General,
}

impl BuiltinRole {
    /// The canonical (lowercase) string written into the manifest.
    pub fn canonical_name(self) -> &'static str {
        match self {
            BuiltinRole::Architect => "architect",
            BuiltinRole::Planner => "planner",
            BuiltinRole::General => "general",
        }
    }

    /// Embedded prompt body for the builtin role.
    pub fn prompt_md(self) -> &'static str {
        match self {
            BuiltinRole::Architect => defaults::DEFAULT_ARCHITECT,
            BuiltinRole::Planner => defaults::DEFAULT_PLANNER,
            BuiltinRole::General => defaults::DEFAULT_GENERAL,
        }
    }
}

impl FromStr for BuiltinRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "architect" => Ok(BuiltinRole::Architect),
            "planner" => Ok(BuiltinRole::Planner),
            "general" => Ok(BuiltinRole::General),
            _ => Err(()),
        }
    }
}

/// Settings template carried alongside a resolved role.
///
/// The current implementation has a single shared settings family for every
/// role (matching the existing `generate_expert_settings` output). The
/// type is kept as a value object so that future role-specific settings
/// variations remain a non-breaking change for callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTemplate {
    family: SettingsFamily,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsFamily {
    Default,
}

impl SettingsTemplate {
    /// The default hook template shared by every builtin and custom role.
    pub const fn default_family() -> Self {
        Self {
            family: SettingsFamily::Default,
        }
    }

    /// Render the template into the JSON string written to
    /// `system_prompt/expert{N}_settings.json`.
    pub fn render(&self, status_file_path: &str) -> String {
        match self.family {
            SettingsFamily::Default => {
                crate::instructions::generate_expert_settings(status_file_path)
            }
        }
    }
}

/// The resolved role data ready to be written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRole {
    pub canonical_name: String,
    pub prompt_md: String,
    pub settings_template: SettingsTemplate,
}

/// Errors surfaced from [`resolve`].
#[derive(Debug, Error)]
pub enum RoleError {
    #[error("custom role template not found: {0}")]
    NotFound(String),

    #[error("custom role template is not valid UTF-8: {0}")]
    InvalidUtf8(PathBuf),

    #[error("failed to read custom role template at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Resolve a role specification into the prompt + settings to materialise.
pub fn resolve(spec: &RoleSpec, project_root: &Path) -> Result<ResolvedRole, RoleError> {
    match spec {
        RoleSpec::Builtin(role) => Ok(ResolvedRole {
            canonical_name: role.canonical_name().to_string(),
            prompt_md: role.prompt_md().to_string(),
            settings_template: SettingsTemplate::default_family(),
        }),
        RoleSpec::Custom { name } => {
            let prompt_md = read_custom_template(project_root, name)?;
            Ok(ResolvedRole {
                canonical_name: name.clone(),
                prompt_md,
                settings_template: SettingsTemplate::default_family(),
            })
        }
    }
}

fn read_custom_template(project_root: &Path, name: &str) -> Result<String, RoleError> {
    let project_local = project_root
        .join(".macot")
        .join("templates")
        .join("roles")
        .join(format!("{name}.md"));
    if let Some(content) = read_if_exists(&project_local)? {
        return Ok(content);
    }

    if let Some(user_global) = user_global_template(name) {
        if let Some(content) = read_if_exists(&user_global)? {
            return Ok(content);
        }
    }

    Err(RoleError::NotFound(name.to_string()))
}

fn read_if_exists(path: &Path) -> Result<Option<String>, RoleError> {
    if !path.exists() {
        return Ok(None);
    }
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(content) => Ok(Some(content)),
            Err(_) => Err(RoleError::InvalidUtf8(path.to_path_buf())),
        },
        Err(source) => Err(RoleError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn user_global_template(name: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|cfg| cfg.join("macot").join("roles").join(format!("{name}.md")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use tempfile::TempDir;

    fn write_template(root: &Path, name: &str, body: &str) {
        let dir = root.join(".macot").join("templates").join("roles");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
    }

    #[test]
    fn resolve_builtin_architect_returns_embedded_prompt() {
        let tmp = TempDir::new().unwrap();
        let resolved = resolve(&RoleSpec::Builtin(BuiltinRole::Architect), tmp.path()).unwrap();

        assert_eq!(
            resolved.canonical_name, "architect",
            "resolve(builtin): canonical_name should be 'architect'"
        );
        assert_eq!(
            resolved.prompt_md,
            defaults::DEFAULT_ARCHITECT,
            "resolve(builtin): prompt should match embedded default"
        );
        assert_eq!(
            resolved.settings_template,
            SettingsTemplate::default_family(),
            "resolve(builtin): settings template should be default family"
        );
    }

    #[test]
    fn resolve_builtin_planner_and_general() {
        let tmp = TempDir::new().unwrap();

        let planner = resolve(&RoleSpec::Builtin(BuiltinRole::Planner), tmp.path()).unwrap();
        assert_eq!(planner.canonical_name, "planner");
        assert_eq!(planner.prompt_md, defaults::DEFAULT_PLANNER);

        let general = resolve(&RoleSpec::Builtin(BuiltinRole::General), tmp.path()).unwrap();
        assert_eq!(general.canonical_name, "general");
        assert_eq!(general.prompt_md, defaults::DEFAULT_GENERAL);
    }

    #[test]
    fn resolve_custom_reads_project_local_template() {
        let tmp = TempDir::new().unwrap();
        write_template(tmp.path(), "qa-bot", "# QA Bot\n\nbody");

        let resolved = resolve(
            &RoleSpec::Custom {
                name: "qa-bot".to_string(),
            },
            tmp.path(),
        )
        .unwrap();

        assert_eq!(resolved.canonical_name, "qa-bot");
        assert_eq!(resolved.prompt_md, "# QA Bot\n\nbody");
    }

    #[test]
    fn resolve_custom_missing_template_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        // Override XDG_CONFIG_HOME to an empty dir so the user-global
        // search is exhausted deterministically.
        let cfg_home = TempDir::new().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", cfg_home.path());
        std::env::set_var("HOME", cfg_home.path());

        let result = resolve(
            &RoleSpec::Custom {
                name: "missing".to_string(),
            },
            tmp.path(),
        );

        assert!(
            matches!(result, Err(RoleError::NotFound(ref n)) if n == "missing"),
            "resolve(custom missing): expected NotFound, got {result:?}"
        );
    }

    #[test]
    fn resolve_custom_rejects_invalid_utf8() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".macot").join("templates").join("roles");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("badbytes.md");
        // Invalid UTF-8 byte sequence.
        std::fs::write(&path, [0xFFu8, 0xFE, 0xFD]).unwrap();

        let result = resolve(
            &RoleSpec::Custom {
                name: "badbytes".to_string(),
            },
            tmp.path(),
        );

        match result {
            Err(RoleError::InvalidUtf8(p)) => assert_eq!(p, path),
            other => panic!("resolve(custom invalid utf8): expected InvalidUtf8, got {other:?}"),
        }
    }

    #[test]
    fn settings_template_renders_status_path() {
        let tpl = SettingsTemplate::default_family();
        let rendered = tpl.render("/tmp/status/expert3");
        assert!(
            rendered.contains("/tmp/status/expert3"),
            "render: should embed the status file path"
        );
        let _: serde_json::Value =
            serde_json::from_str(&rendered).expect("render: result should be valid JSON");
    }

    #[test]
    fn builtin_from_str_round_trip() {
        for r in [
            BuiltinRole::Architect,
            BuiltinRole::Planner,
            BuiltinRole::General,
        ] {
            let parsed: BuiltinRole = r.canonical_name().parse().unwrap();
            assert_eq!(parsed, r);
        }
        assert!(BuiltinRole::from_str("nope").is_err());
    }

    proptest! {
        // Property 5: Role Resolution Determinism.
        // **Validates: Requirements 3.3, Property 5**
        #[test]
        fn resolve_is_deterministic_for_fixed_fs(
            kind in 0u8..6,
            custom_body in "[a-zA-Z0-9 \n#]{0,128}",
        ) {
            let tmp = TempDir::new().unwrap();
            write_template(tmp.path(), "fixture-role", &custom_body);

            let spec = match kind {
                0 => RoleSpec::Builtin(BuiltinRole::Architect),
                1 => RoleSpec::Builtin(BuiltinRole::Planner),
                2 => RoleSpec::Builtin(BuiltinRole::General),
                3 => RoleSpec::Custom { name: "fixture-role".to_string() },
                4 => RoleSpec::Builtin(BuiltinRole::Architect),
                _ => RoleSpec::Custom { name: "fixture-role".to_string() },
            };

            let first = resolve(&spec, tmp.path()).unwrap();
            let second = resolve(&spec, tmp.path()).unwrap();

            prop_assert_eq!(&first.canonical_name, &second.canonical_name);
            prop_assert_eq!(&first.prompt_md, &second.prompt_md);
            prop_assert_eq!(&first.settings_template, &second.settings_template);
        }
    }
}
