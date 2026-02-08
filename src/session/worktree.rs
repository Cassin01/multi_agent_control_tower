use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::task::JoinHandle;

fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Path contains non-UTF8 characters: {:?}", path))
}

pub struct WorktreeLaunchResult {
    pub expert_id: u32,
    pub expert_name: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub claude_ready: bool,
}

pub enum WorktreeLaunchState {
    Idle,
    InProgress {
        handle: JoinHandle<Result<WorktreeLaunchResult>>,
        expert_name: String,
        branch_name: String,
    },
}

impl Default for WorktreeLaunchState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Clone)]
pub struct WorktreeManager {
    project_path: PathBuf,
    macot_path: PathBuf,
}

impl WorktreeManager {
    pub fn new(project_path: PathBuf) -> Self {
        let macot_path = project_path.join(".macot");
        Self {
            project_path,
            macot_path,
        }
    }

    pub fn worktree_dir(&self) -> PathBuf {
        self.macot_path.join("worktrees")
    }

    pub fn worktree_path(&self, branch_name: &str) -> PathBuf {
        self.worktree_dir().join(branch_name)
    }

    pub fn worktree_exists(&self, branch_name: &str) -> bool {
        self.worktree_path(branch_name).exists()
    }

    pub async fn create_worktree(&self, branch_name: &str) -> Result<PathBuf> {
        let wt_path = self.worktree_path(branch_name);

        tokio::fs::create_dir_all(self.worktree_dir())
            .await
            .context("Failed to create worktrees directory")?;

        let wt_path_str = path_to_str(&wt_path)?;

        let output = Command::new("git")
            .args(["worktree", "add", wt_path_str, branch_name])
            .current_dir(&self.project_path)
            .output()
            .await
            .context("Failed to run git worktree add")?;

        if output.status.success() {
            return Ok(wt_path);
        }

        let output = Command::new("git")
            .args(["worktree", "add", wt_path_str, "-b", branch_name])
            .current_dir(&self.project_path)
            .output()
            .await
            .context("Failed to run git worktree add -b")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree add failed: {}", stderr);
        }

        Ok(wt_path)
    }

    pub async fn setup_macot_symlink(&self, worktree_path: &Path) -> Result<()> {
        let symlink_path = worktree_path.join(".macot");
        let target = self
            .macot_path
            .canonicalize()
            .context("Failed to canonicalize .macot path")?;

        if symlink_path.exists() || symlink_path.is_symlink() {
            tokio::fs::remove_file(&symlink_path).await.ok();
            tokio::fs::remove_dir_all(&symlink_path).await.ok();
        }

        #[cfg(unix)]
        tokio::fs::symlink(&target, &symlink_path)
            .await
            .context("Failed to create .macot symlink")?;

        #[cfg(not(unix))]
        anyhow::bail!("Worktree symlink creation is only supported on Unix platforms");

        #[cfg(unix)]
        Ok(())
    }

    pub async fn remove_worktree(&self, branch_name: &str) -> Result<()> {
        let wt_path = self.worktree_path(branch_name);
        let wt_path_str = path_to_str(&wt_path)?;

        let output = Command::new("git")
            .args(["worktree", "remove", wt_path_str])
            .current_dir(&self.project_path)
            .output()
            .await
            .context("Failed to remove git worktree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree remove failed: {}", stderr);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_dir_returns_macot_worktrees_path() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/project"));
        assert_eq!(
            mgr.worktree_dir(),
            PathBuf::from("/tmp/project/.macot/worktrees"),
            "worktree_dir: should return .macot/worktrees/ under project path"
        );
    }

    #[test]
    fn worktree_path_returns_branch_subdirectory() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/project"));
        assert_eq!(
            mgr.worktree_path("feature-auth"),
            PathBuf::from("/tmp/project/.macot/worktrees/feature-auth"),
            "worktree_path: should return .macot/worktrees/<branch>/"
        );
    }

    #[test]
    fn worktree_exists_returns_false_for_nonexistent() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/nonexistent-project-abc123"));
        assert!(
            !mgr.worktree_exists("no-such-branch"),
            "worktree_exists: should return false for nonexistent path"
        );
    }

    #[test]
    fn worktree_launch_state_default_is_idle() {
        let state = WorktreeLaunchState::default();
        assert!(
            matches!(state, WorktreeLaunchState::Idle),
            "WorktreeLaunchState::default: should be Idle"
        );
    }

    #[test]
    fn worktree_path_different_branches_produce_different_paths() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/project"));
        let path_a = mgr.worktree_path("branch-a");
        let path_b = mgr.worktree_path("branch-b");
        assert_ne!(
            path_a, path_b,
            "worktree_path: different branches should produce different paths"
        );
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn worktree_path_uniqueness(
            id1 in 0u32..100,
            ts1 in "[0-9]{8}-[0-9]{6}",
            id2 in 0u32..100,
            ts2 in "[0-9]{8}-[0-9]{6}",
        ) {
            let mgr = WorktreeManager::new(PathBuf::from("/project"));
            let branch1 = format!("expert-{}-{}", id1, ts1);
            let branch2 = format!("expert-{}-{}", id2, ts2);

            if branch1 != branch2 {
                prop_assert_ne!(
                    mgr.worktree_path(&branch1),
                    mgr.worktree_path(&branch2),
                    "worktree_path: unique branch names must produce unique paths"
                );
            }
        }

        #[test]
        fn worktree_path_always_under_macot(branch in "[a-zA-Z0-9_-]{1,50}") {
            let mgr = WorktreeManager::new(PathBuf::from("/project"));
            let path = mgr.worktree_path(&branch);
            let path_str = path.to_string_lossy();
            prop_assert!(
                path_str.contains(".macot/worktrees/"),
                "worktree_path: all paths must be under .macot/worktrees/"
            );
        }
    }
}
