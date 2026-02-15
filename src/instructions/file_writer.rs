use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub fn instruction_file_path(queue_path: &Path, expert_id: u32) -> PathBuf {
    queue_path
        .join("system_prompt")
        .join(format!("expert{}.md", expert_id))
}

pub fn write_instruction_file(queue_path: &Path, expert_id: u32, content: &str) -> Result<PathBuf> {
    let path = instruction_file_path(queue_path, expert_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create instruction directory: {:?}", parent))?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write instruction file: {:?}", path))?;
    Ok(path)
}

pub fn agents_file_path(queue_path: &Path, expert_id: u32) -> PathBuf {
    queue_path
        .join("system_prompt")
        .join(format!("expert{}_agents.json", expert_id))
}

pub fn write_agents_file(queue_path: &Path, expert_id: u32, json: &str) -> Result<PathBuf> {
    let path = agents_file_path(queue_path, expert_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create agents directory: {:?}", parent))?;
    }
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write agents file: {:?}", path))?;
    Ok(path)
}

#[allow(dead_code)]
pub fn cleanup_instruction_file(queue_path: &Path, expert_id: u32) -> Result<()> {
    let path = instruction_file_path(queue_path, expert_id);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove instruction file: {:?}", path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn instruction_file_path_returns_expected_path() {
        let path = instruction_file_path(Path::new("/tmp/queue"), 0);
        assert_eq!(
            path,
            PathBuf::from("/tmp/queue/system_prompt/expert0.md"),
            "instruction_file_path: should return queue_path/system_prompt/expertN.md"
        );
    }

    #[test]
    fn instruction_file_path_different_expert_ids() {
        let path = instruction_file_path(Path::new("/tmp/queue"), 3);
        assert_eq!(
            path,
            PathBuf::from("/tmp/queue/system_prompt/expert3.md"),
            "instruction_file_path: should include expert id in filename"
        );
    }

    #[test]
    fn write_instruction_file_creates_dir_and_file() {
        let tmp = TempDir::new().unwrap();
        let content = "# Test Instruction\n\nSome content.";

        let path = write_instruction_file(tmp.path(), 0, content).unwrap();

        assert!(
            path.exists(),
            "write_instruction_file: should create the file"
        );
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, content,
            "write_instruction_file: file content should match"
        );
    }

    #[test]
    fn write_instruction_file_overwrites_existing() {
        let tmp = TempDir::new().unwrap();

        write_instruction_file(tmp.path(), 1, "first").unwrap();
        let path = write_instruction_file(tmp.path(), 1, "second").unwrap();

        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, "second",
            "write_instruction_file: should overwrite existing file"
        );
    }

    #[test]
    fn cleanup_instruction_file_removes_existing() {
        let tmp = TempDir::new().unwrap();
        let path = write_instruction_file(tmp.path(), 2, "content").unwrap();
        assert!(path.exists());

        cleanup_instruction_file(tmp.path(), 2).unwrap();

        assert!(
            !path.exists(),
            "cleanup_instruction_file: should remove the file"
        );
    }

    #[test]
    fn cleanup_instruction_file_noop_when_missing() {
        let tmp = TempDir::new().unwrap();
        let result = cleanup_instruction_file(tmp.path(), 99);
        assert!(
            result.is_ok(),
            "cleanup_instruction_file: should succeed when file doesn't exist"
        );
    }

    #[test]
    fn agents_file_path_returns_expected_path() {
        let path = agents_file_path(Path::new("/tmp/queue"), 0);
        assert_eq!(
            path,
            PathBuf::from("/tmp/queue/system_prompt/expert0_agents.json"),
            "agents_file_path: should return queue_path/system_prompt/expertN_agents.json"
        );
    }

    #[test]
    fn agents_file_path_different_expert_ids() {
        let path = agents_file_path(Path::new("/tmp/queue"), 5);
        assert_eq!(
            path,
            PathBuf::from("/tmp/queue/system_prompt/expert5_agents.json"),
            "agents_file_path: should include expert id in filename"
        );
    }

    #[test]
    fn write_agents_file_creates_dir_and_file() {
        let tmp = TempDir::new().unwrap();
        let json = r#"{"messaging":{"description":"test","prompt":"hello"}}"#;

        let path = write_agents_file(tmp.path(), 0, json).unwrap();

        assert!(path.exists(), "write_agents_file: should create the file");
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, json,
            "write_agents_file: file content should match"
        );
    }

    #[test]
    fn write_agents_file_overwrites_existing() {
        let tmp = TempDir::new().unwrap();

        write_agents_file(tmp.path(), 1, "first").unwrap();
        let path = write_agents_file(tmp.path(), 1, "second").unwrap();

        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, "second",
            "write_agents_file: should overwrite existing file"
        );
    }
}
