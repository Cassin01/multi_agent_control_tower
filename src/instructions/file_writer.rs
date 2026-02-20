use anyhow::{Context, Result};
use serde_json::json;
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

pub fn settings_file_path(queue_path: &Path, expert_id: u32) -> PathBuf {
    queue_path
        .join("system_prompt")
        .join(format!("expert{}_settings.json", expert_id))
}

pub fn write_settings_file(queue_path: &Path, expert_id: u32, json: &str) -> Result<PathBuf> {
    let path = settings_file_path(queue_path, expert_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create settings directory: {:?}", parent))?;
    }
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write settings file: {:?}", path))?;
    Ok(path)
}

pub fn generate_hooks_settings(status_file_path: &str) -> String {
    let quoted_path = shell_single_quote(status_file_path);
    let pre_tool_use_command = concat!(
        "INPUT=$(cat); ",
        "TARGET=$(echo \"$INPUT\" | jq -r '(.tool_input.file_path // .tool_input.command // \"\")'); ",
        "if echo \"$TARGET\" | grep -q 'messages/queue/'; then ",
        "printf '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",",
        "\"permissionDecision\":\"deny\",",
        "\"permissionDecisionReason\":\"ERROR: Writing directly to messages/queue/ is forbidden. ",
        "Write to messages/outbox/ instead.\"}}'; ",
        "fi"
    );
    json!({
        "hooks": {
            "UserPromptSubmit": [{
                "hooks": [{
                    "type": "command",
                    "command": format!("printf '%s' processing >| {}", quoted_path),
                }]
            }],
            "Stop": [{
                "hooks": [{
                    "type": "command",
                    "command": format!("printf '%s' pending >| {}", quoted_path),
                }]
            }],
            "PreToolUse": [{
                "matcher": "Write|Edit|Bash",
                "hooks": [{
                    "type": "command",
                    "command": pre_tool_use_command,
                }]
            }]
        }
    })
    .to_string()
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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

    #[test]
    fn settings_file_path_returns_expected_path() {
        let path = settings_file_path(Path::new("/tmp/queue"), 0);
        assert_eq!(
            path,
            PathBuf::from("/tmp/queue/system_prompt/expert0_settings.json"),
            "settings_file_path: should return queue_path/system_prompt/expertN_settings.json"
        );
    }

    #[test]
    fn settings_file_path_different_expert_ids() {
        let path = settings_file_path(Path::new("/tmp/queue"), 7);
        assert_eq!(
            path,
            PathBuf::from("/tmp/queue/system_prompt/expert7_settings.json"),
            "settings_file_path: should include expert id in filename"
        );
    }

    #[test]
    fn write_settings_file_creates_dir_and_file() {
        let tmp = TempDir::new().unwrap();
        let json = r#"{"hooks":{}}"#;

        let path = write_settings_file(tmp.path(), 0, json).unwrap();

        assert!(path.exists(), "write_settings_file: should create the file");
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            read_back, json,
            "write_settings_file: file content should match"
        );
    }

    #[test]
    fn generate_hooks_settings_contains_user_prompt_submit() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        assert!(
            json.contains("UserPromptSubmit"),
            "generate_hooks_settings: should contain UserPromptSubmit hook"
        );
    }

    #[test]
    fn generate_hooks_settings_contains_stop_hook() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        assert!(
            json.contains("Stop"),
            "generate_hooks_settings: should contain Stop hook"
        );
    }

    #[test]
    fn generate_hooks_settings_contains_status_path() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        assert!(
            json.contains("/tmp/status/expert0"),
            "generate_hooks_settings: should contain the status file path"
        );
    }

    #[test]
    fn generate_hooks_settings_contains_processing_command() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        assert!(
            json.contains("processing"),
            "generate_hooks_settings: UserPromptSubmit hook should write 'processing'"
        );
    }

    #[test]
    fn generate_hooks_settings_contains_pending_command() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        assert!(
            json.contains("pending"),
            "generate_hooks_settings: Stop hook should write 'pending'"
        );
    }

    #[test]
    fn generate_hooks_settings_is_valid_json() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("generate_hooks_settings: output should be valid JSON");
        assert!(
            parsed.get("hooks").is_some(),
            "generate_hooks_settings: should have a 'hooks' top-level key"
        );
    }

    #[test]
    fn generate_hooks_settings_contains_pre_tool_use_hook() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        assert!(
            json.contains("PreToolUse"),
            "generate_hooks_settings: should contain PreToolUse hook"
        );
    }

    #[test]
    fn generate_hooks_settings_pre_tool_use_has_write_edit_bash_matcher() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let pre_tool_use = &parsed["hooks"]["PreToolUse"];
        assert!(
            !pre_tool_use.is_null(),
            "generate_hooks_settings: PreToolUse hook should exist"
        );
        let matcher = pre_tool_use[0]["matcher"].as_str().unwrap_or("");
        assert!(
            matcher.contains("Write") && matcher.contains("Edit") && matcher.contains("Bash"),
            "generate_hooks_settings: PreToolUse matcher should include Write, Edit, and Bash"
        );
    }

    #[test]
    fn generate_hooks_settings_pre_tool_use_blocks_queue_writes() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let command = parsed["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap_or("");
        assert!(
            command.contains("messages/queue/"),
            "generate_hooks_settings: PreToolUse command should check for messages/queue/ path"
        );
        assert!(
            command.contains("deny"),
            "generate_hooks_settings: PreToolUse command should deny writes to queue"
        );
    }

    #[test]
    fn generate_hooks_settings_pre_tool_use_suggests_outbox() {
        let json = generate_hooks_settings("/tmp/status/expert0");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let command = parsed["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap_or("");
        assert!(
            command.contains("outbox"),
            "generate_hooks_settings: PreToolUse deny reason should suggest outbox"
        );
    }

    #[test]
    fn generate_hooks_settings_escapes_single_quote_in_status_path() {
        let json = generate_hooks_settings("/tmp/status/it's/me");
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("generate_hooks_settings: output should be valid JSON");

        let processing_cmd = parsed["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
            .as_str()
            .expect("generate_hooks_settings: command should be string");
        let stop_cmd = parsed["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .expect("generate_hooks_settings: command should be string");

        assert_eq!(
            processing_cmd, "printf '%s' processing >| '/tmp/status/it'\\''s/me'",
            "generate_hooks_settings: processing command should safely quote path"
        );
        assert_eq!(
            stop_cmd, "printf '%s' pending >| '/tmp/status/it'\\''s/me'",
            "generate_hooks_settings: stop command should safely quote path"
        );
    }
}
