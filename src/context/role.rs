use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::instructions::defaults;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub expert_id: u32,
    pub role: String,
    pub assigned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExpertRoles {
    pub session_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub assignments: Vec<RoleAssignment>,
}

impl SessionExpertRoles {
    pub fn new(session_hash: String) -> Self {
        Self {
            session_hash,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            assignments: Vec::new(),
        }
    }

    pub fn get_role(&self, expert_id: u32) -> Option<&str> {
        self.assignments
            .iter()
            .find(|a| a.expert_id == expert_id)
            .map(|a| a.role.as_str())
    }

    pub fn set_role(&mut self, expert_id: u32, role: String) {
        self.updated_at = Utc::now();

        if let Some(assignment) = self.assignments.iter_mut().find(|a| a.expert_id == expert_id) {
            assignment.role = role;
            assignment.assigned_at = Utc::now();
        } else {
            self.assignments.push(RoleAssignment {
                expert_id,
                role,
                assigned_at: Utc::now(),
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoleInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct AvailableRoles {
    pub roles: Vec<RoleInfo>,
}

impl AvailableRoles {
    /// Load available roles from user's config folder and merge with embedded defaults.
    /// User custom roles in the folder take precedence over embedded defaults.
    pub fn from_instructions_path(path: &Path) -> Result<Self> {
        let mut roles = Vec::new();

        // Scan user's config folder for custom roles
        if path.exists() {
            let entries = std::fs::read_dir(path)?;
            for entry in entries.flatten() {
                let file_path = entry.path();

                if file_path.extension().map(|e| e == "md").unwrap_or(false) {
                    let file_name = file_path.file_stem().and_then(|s| s.to_str());

                    if let Some(name) = file_name {
                        if name == "core" {
                            continue;
                        }

                        let content = std::fs::read_to_string(&file_path).unwrap_or_default();
                        let description = content
                            .lines()
                            .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                            .unwrap_or("")
                            .to_string();

                        let display_name = Self::capitalize_name(name);

                        roles.push(RoleInfo {
                            name: name.to_string(),
                            display_name,
                            description,
                        });
                    }
                }
            }
        }

        // Merge with embedded defaults (always available)
        for name in defaults::default_role_names() {
            if !roles.iter().any(|r| r.name == *name) {
                let default_content = defaults::get_default(name).unwrap_or("");
                let description = default_content
                    .lines()
                    .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                    .unwrap_or("")
                    .to_string();

                roles.push(RoleInfo {
                    name: name.to_string(),
                    display_name: Self::capitalize_name(name),
                    description,
                });
            }
        }

        roles.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Self { roles })
    }

    fn capitalize_name(name: &str) -> String {
        name.split(['-', '_'])
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(if name.contains('-') { "-" } else { " " })
    }

    #[allow(dead_code)]
    pub fn find_by_name(&self, name: &str) -> Option<&RoleInfo> {
        self.roles.iter().find(|r| r.name == name)
    }

    #[allow(dead_code)]
    pub fn names(&self) -> Vec<&str> {
        self.roles.iter().map(|r| r.name.as_str()).collect()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn session_expert_roles_new_creates_empty_assignments() {
        let roles = SessionExpertRoles::new("test-hash".to_string());
        assert_eq!(roles.session_hash, "test-hash");
        assert!(roles.assignments.is_empty());
    }

    #[test]
    fn session_expert_roles_set_and_get_role() {
        let mut roles = SessionExpertRoles::new("test-hash".to_string());

        roles.set_role(0, "architect".to_string());
        assert_eq!(roles.get_role(0), Some("architect"));

        roles.set_role(1, "frontend".to_string());
        assert_eq!(roles.get_role(1), Some("frontend"));

        assert_eq!(roles.get_role(99), None);
    }

    #[test]
    fn session_expert_roles_update_existing_role() {
        let mut roles = SessionExpertRoles::new("test-hash".to_string());

        roles.set_role(0, "architect".to_string());
        roles.set_role(0, "backend".to_string());

        assert_eq!(roles.get_role(0), Some("backend"));
        assert_eq!(roles.assignments.len(), 1);
    }

    #[test]
    fn available_roles_from_empty_path_includes_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        // Even with empty path, should include embedded defaults
        assert!(roles.find_by_name("architect").is_some());
        assert!(roles.find_by_name("backend").is_some());
        assert!(roles.find_by_name("frontend").is_some());
        assert!(roles.find_by_name("planner").is_some());
        assert!(roles.find_by_name("tester").is_some());
        assert!(roles.find_by_name("general").is_some());
    }

    #[test]
    fn available_roles_custom_overrides_default() {
        let temp_dir = TempDir::new().unwrap();

        // Custom architect with different description
        std::fs::write(
            temp_dir.path().join("architect.md"),
            "# Architect\n\nCustom architect description",
        )
        .unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();

        let architect = roles.find_by_name("architect").unwrap();
        assert_eq!(architect.description, "Custom architect description");
        // Other defaults should still be present
        assert!(roles.find_by_name("backend").is_some());
    }

    #[test]
    fn available_roles_core_excluded() {
        let temp_dir = TempDir::new().unwrap();

        std::fs::write(temp_dir.path().join("core.md"), "# Core\n\nCore instructions").unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        assert!(roles.find_by_name("core").is_none());
    }

    #[test]
    fn available_roles_names() {
        let temp_dir = TempDir::new().unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        let names = roles.names();

        // Should include all embedded defaults
        assert!(names.contains(&"architect"));
        assert!(names.contains(&"frontend"));
        assert!(names.contains(&"backend"));
        assert!(names.contains(&"planner"));
        assert!(names.contains(&"tester"));
        assert!(names.contains(&"general"));
    }

    #[test]
    fn role_info_display_name_capitalized() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("architect.md"), "# Architect").unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        let architect = roles.find_by_name("architect").unwrap();

        assert_eq!(architect.display_name, "Architect");
    }

    #[test]
    fn role_info_display_name_title_case_with_separators() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("full-stack-dev.md"),
            "# Full Stack Dev",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("backend_engineer.md"),
            "# Backend Engineer",
        )
        .unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();

        let full_stack = roles.find_by_name("full-stack-dev").unwrap();
        assert_eq!(full_stack.display_name, "Full-Stack-Dev");

        let backend = roles.find_by_name("backend_engineer").unwrap();
        assert_eq!(backend.display_name, "Backend Engineer");
    }
}
