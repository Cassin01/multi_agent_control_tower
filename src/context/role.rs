use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

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

#[derive(Debug, Clone)]
pub struct AvailableRoles {
    pub roles: Vec<RoleInfo>,
}

impl AvailableRoles {
    pub fn from_instructions_path(path: &Path) -> Result<Self> {
        let mut roles = Vec::new();

        if !path.exists() {
            return Ok(Self { roles });
        }

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

                    let display_name = name
                        .chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string() + &name[1..])
                        .unwrap_or_else(|| name.to_string());

                    roles.push(RoleInfo {
                        name: name.to_string(),
                        display_name,
                        description,
                    });
                }
            }
        }

        roles.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Self { roles })
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

impl Default for AvailableRoles {
    fn default() -> Self {
        Self { roles: Vec::new() }
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
    fn available_roles_from_empty_path() {
        let temp_dir = TempDir::new().unwrap();
        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        assert!(roles.roles.is_empty());
    }

    #[test]
    fn available_roles_from_instructions_path() {
        let temp_dir = TempDir::new().unwrap();

        std::fs::write(
            temp_dir.path().join("architect.md"),
            "# Architect\n\nSystem design expert",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("backend.md"),
            "# Backend\n\nServer-side development",
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("core.md"), "# Core\n\nCore instructions").unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();

        assert_eq!(roles.roles.len(), 2);
        assert!(roles.find_by_name("architect").is_some());
        assert!(roles.find_by_name("backend").is_some());
        assert!(roles.find_by_name("core").is_none());
    }

    #[test]
    fn available_roles_names() {
        let temp_dir = TempDir::new().unwrap();

        std::fs::write(temp_dir.path().join("architect.md"), "# Architect").unwrap();
        std::fs::write(temp_dir.path().join("frontend.md"), "# Frontend").unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        let names = roles.names();

        assert!(names.contains(&"architect"));
        assert!(names.contains(&"frontend"));
    }

    #[test]
    fn role_info_display_name_capitalized() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("architect.md"), "# Architect").unwrap();

        let roles = AvailableRoles::from_instructions_path(temp_dir.path()).unwrap();
        let architect = roles.find_by_name("architect").unwrap();

        assert_eq!(architect.display_name, "Architect");
    }
}
