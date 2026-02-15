use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::task::JoinHandle;

fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Path contains non-UTF8 characters: {:?}", path))
}

pub struct WorktreeLaunchResult {
    #[allow(dead_code)]
    pub expert_id: u32,
    pub expert_name: String,
    pub branch_name: String,
    #[allow(dead_code)]
    pub worktree_path: String,
    pub claude_ready: bool,
}

#[derive(Default)]
pub enum WorktreeLaunchState {
    #[default]
    Idle,
    InProgress {
        handle: JoinHandle<Result<WorktreeLaunchResult>>,
        expert_name: String,
        branch_name: String,
    },
}

async fn resolve_git_root(project_path: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(project_path)
        .output()
        .await
        .context("Failed to resolve git root — is this a git repository?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Failed to resolve git root — is this a git repository? {}",
            stderr.trim()
        );
    }

    let common_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    // --git-common-dir returns the .git directory (e.g. /project/.git)
    // Its parent is the working tree root
    Ok(common_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| project_path.to_path_buf()))
}

#[derive(Clone)]
pub struct WorktreeManager {
    git_root: PathBuf,
    macot_path: PathBuf,
}

impl WorktreeManager {
    pub fn new(git_root: PathBuf) -> Self {
        let macot_path = git_root.join(".macot");
        Self {
            git_root,
            macot_path,
        }
    }

    pub async fn resolve(project_path: PathBuf) -> Result<Self> {
        let git_root = resolve_git_root(&project_path).await?;
        Ok(Self::new(git_root))
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
            .current_dir(&self.git_root)
            .output()
            .await
            .context("Failed to run git worktree add")?;

        if output.status.success() {
            return Ok(wt_path);
        }

        let output = Command::new("git")
            .args(["worktree", "add", wt_path_str, "-b", branch_name])
            .current_dir(&self.git_root)
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

    #[allow(dead_code)]
    pub async fn remove_worktree(&self, branch_name: &str) -> Result<()> {
        let wt_path = self.worktree_path(branch_name);
        let wt_path_str = path_to_str(&wt_path)?;

        let output = Command::new("git")
            .args(["worktree", "remove", wt_path_str])
            .current_dir(&self.git_root)
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
mod resolve_tests {
    use super::*;

    #[tokio::test]
    async fn resolve_from_main_repo_returns_same_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().canonicalize().unwrap();

        // Initialize a git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let mgr = WorktreeManager::resolve(repo_path.clone()).await.unwrap();
        assert_eq!(
            mgr.git_root, repo_path,
            "resolve: from main repo should return the same path as git_root"
        );
        assert_eq!(
            mgr.macot_path,
            repo_path.join(".macot"),
            "resolve: macot_path should be git_root/.macot"
        );
    }

    #[tokio::test]
    async fn resolve_from_worktree_returns_main_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().to_path_buf();

        // Initialize a git repo with a commit
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create a worktree
        let wt_path = tmp.path().join("worktree-branch");
        std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                wt_path.to_str().unwrap(),
                "-b",
                "test-wt-branch",
            ])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Resolve from inside the worktree
        let mgr = WorktreeManager::resolve(wt_path.clone()).await.unwrap();

        let canonical_repo = repo_path.canonicalize().unwrap();
        assert_eq!(
            mgr.git_root, canonical_repo,
            "resolve: from worktree should return main repo root as git_root"
        );
        assert_eq!(
            mgr.macot_path,
            canonical_repo.join(".macot"),
            "resolve: macot_path should be under main repo root"
        );
    }

    #[tokio::test]
    async fn resolve_idempotent_across_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().to_path_buf();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let wt1 = tmp.path().join("wt1");
        let wt2 = tmp.path().join("wt2");
        std::process::Command::new("git")
            .args(["worktree", "add", wt1.to_str().unwrap(), "-b", "branch1"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["worktree", "add", wt2.to_str().unwrap(), "-b", "branch2"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let mgr1 = WorktreeManager::resolve(wt1).await.unwrap();
        let mgr2 = WorktreeManager::resolve(wt2).await.unwrap();
        let mgr_main = WorktreeManager::resolve(repo_path).await.unwrap();

        assert_eq!(
            mgr1.git_root, mgr2.git_root,
            "resolve: different worktrees should produce the same git_root"
        );
        assert_eq!(
            mgr1.git_root, mgr_main.git_root,
            "resolve: worktree and main repo should produce the same git_root"
        );
        assert_eq!(
            mgr1.macot_path, mgr2.macot_path,
            "resolve: different worktrees should produce the same macot_path"
        );
    }

    #[tokio::test]
    async fn resolve_fails_for_non_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let result = WorktreeManager::resolve(tmp.path().to_path_buf()).await;
        assert!(
            result.is_err(),
            "resolve: should fail for non-git directory"
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
            name1 in "[a-z][a-z0-9-]{0,30}",
            name2 in "[a-z][a-z0-9-]{0,30}",
        ) {
            let mgr = WorktreeManager::new(PathBuf::from("/project"));

            if name1 != name2 {
                prop_assert_ne!(
                    mgr.worktree_path(&name1),
                    mgr.worktree_path(&name2),
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
