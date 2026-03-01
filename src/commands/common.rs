use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::context::ContextStore;
use crate::instructions::{
    generate_hooks_settings, load_instruction_with_template, write_agents_file,
    write_instruction_file, write_settings_file,
};
use crate::queue::QueueManager;
use crate::session::{ClaudeManager, ExpertStateDetector, SessionMetadata, TmuxManager};
use crate::utils::{compute_path_hash, path_to_str};

/// Try to find a running session that matches the current directory's hash.
/// Returns the session name if exactly one match is found.
async fn resolve_session_by_cwd() -> Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    let hash = compute_path_hash(&cwd);
    let expected_suffix = format!("-{hash}");

    let sessions = TmuxManager::list_all_macot_sessions().await?;
    let matched: Vec<_> = sessions
        .iter()
        .filter(|s| s.session_name.ends_with(&expected_suffix))
        .collect();

    match matched.len() {
        1 => Ok(Some(matched[0].session_name.clone())),
        _ => Ok(None),
    }
}

/// Resolve a single macot session from running sessions.
///
/// Resolution order:
/// 1. Compute hash from current working directory and find a matching session
/// 2. If no match by hash, fall back to single-session auto-detection
pub async fn resolve_single_session(no_sessions_msg: &str) -> Result<String> {
    if let Some(name) = resolve_session_by_cwd().await? {
        return Ok(name);
    }

    let sessions = TmuxManager::list_all_macot_sessions().await?;

    match sessions.len() {
        0 => bail!("{no_sessions_msg}"),
        1 => Ok(sessions[0].session_name.clone()),
        _ => {
            eprintln!("Multiple sessions running. Please specify one with --session:");
            for session in &sessions {
                eprintln!("  {} - {}", session.session_name, session.project_path);
            }
            bail!("Multiple sessions running, please specify session name")
        }
    }
}

/// Default message version for resolve_single_session
pub async fn resolve_single_session_default() -> Result<String> {
    resolve_single_session("No macot sessions running").await
}

pub struct SessionManagers {
    pub tmux: TmuxManager,
    pub claude: ClaudeManager,
}

/// Initialize a new macot session: check no existing session, set up queue/status/context,
/// create the tmux session, and return managers for tmux and claude.
pub async fn init_session(config: &Config, project_path: &Path) -> Result<SessionManagers> {
    let tmux = TmuxManager::from_config(config);

    if tmux.session_exists().await {
        bail!(
            "Session {} already exists. Run 'macot down' first.",
            config.session_name()
        );
    }

    let queue = QueueManager::new(config.queue_path.clone());
    queue.init().await.context("Failed to initialize queue")?;

    let detector = ExpertStateDetector::new(config.queue_path.join("status"));
    for i in 0..config.num_experts() {
        detector
            .set_marker(i, "pending")
            .context("Failed to initialize expert status")?;
    }

    let context_store = ContextStore::new(config.queue_path.clone());
    context_store
        .init_session(&config.session_hash(), config.num_experts())
        .await
        .context("Failed to initialize context store")?;

    let project_str = path_to_str(project_path)?;
    tmux.create_session(config.num_experts(), project_str)
        .await
        .context("Failed to create tmux session")?;

    tmux.init_session_metadata(project_str, config.num_experts())
        .await?;

    let claude = ClaudeManager::new(config.session_name());

    Ok(SessionManagers { tmux, claude })
}

pub struct PreparedExpertFiles {
    pub instruction_file: Option<PathBuf>,
    pub agents_file: Option<PathBuf>,
    pub settings_file: Option<PathBuf>,
    pub used_general_fallback: bool,
    pub requested_role: String,
}

/// Load instruction template and write instruction/agents/settings files for a single expert
/// using an explicitly provided role name and optional worktree path.
pub fn prepare_expert_files_with_role(
    config: &Config,
    expert_id: u32,
    role: &str,
    worktree_path: Option<&str>,
) -> Result<PreparedExpertFiles> {
    // Validate that the expert ID exists in the configuration
    config
        .get_expert(expert_id)
        .with_context(|| format!("No expert configured with id {}", expert_id))?;

    let expert_name = config.get_expert_name(expert_id);
    let manifest_path = config.queue_path.join("experts_manifest.json");
    let manifest_path_str = manifest_path.to_string_lossy();
    let status_dir = config.queue_path.join("status");
    let status_dir_str = status_dir.to_string_lossy();

    let instruction_result = load_instruction_with_template(
        &config.core_instructions_path,
        &config.role_instructions_path,
        role,
        expert_id,
        &expert_name,
        &config.status_file_path(expert_id),
        worktree_path,
        &manifest_path_str,
        &status_dir_str,
    )?;

    let instruction_file = if !instruction_result.content.is_empty() {
        Some(write_instruction_file(
            &config.queue_path,
            expert_id,
            &instruction_result.content,
        )?)
    } else {
        None
    };

    let agents_file = match &instruction_result.agents_json {
        Some(json) => Some(write_agents_file(&config.queue_path, expert_id, json)?),
        None => None,
    };

    let hooks_json = generate_hooks_settings(&config.status_file_path(expert_id));
    let settings_file = Some(write_settings_file(
        &config.queue_path,
        expert_id,
        &hooks_json,
    )?);

    Ok(PreparedExpertFiles {
        instruction_file,
        agents_file,
        settings_file,
        used_general_fallback: instruction_result.used_general_fallback,
        requested_role: instruction_result.requested_role,
    })
}

/// Load instruction template and write instruction/agents/settings files for a single expert.
/// Returns `(instruction_file, agents_file, settings_file)` paths.
pub fn prepare_expert_files(
    config: &Config,
    expert_id: u32,
) -> Result<(Option<PathBuf>, Option<PathBuf>, Option<PathBuf>)> {
    let role_name = config.get_expert_role(expert_id);
    let prepared = prepare_expert_files_with_role(config, expert_id, &role_name, None)?;
    Ok((
        prepared.instruction_file,
        prepared.agents_file,
        prepared.settings_file,
    ))
}

/// Send Escape + /exit to an expert, wait for it to stop, then set status to "pending".
pub async fn exit_expert_and_set_pending(
    claude: &ClaudeManager,
    detector: &ExpertStateDetector,
    expert_id: u32,
) -> Result<()> {
    claude.send_keys(expert_id, "Escape").await?;
    claude.send_exit(expert_id).await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    detector
        .set_marker(expert_id, "pending")
        .context("Failed to set expert status to pending")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_expert_files_uses_role_not_name() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config::default().with_project_path(tmp.path().to_path_buf());
        // Default config: expert 0 = name "Alyosha", role "architect"
        let expert = config.get_expert(0).unwrap();
        assert_eq!(expert.name, "Alyosha");
        assert_eq!(expert.role, "architect");

        std::fs::create_dir_all(config.queue_path.join("system_prompt")).ok();
        std::fs::create_dir_all(config.queue_path.join("status")).ok();

        let (instruction_file, _, _) = prepare_expert_files(&config, 0).unwrap();

        let content = std::fs::read_to_string(instruction_file.unwrap()).unwrap();
        assert!(
            content.contains("Expert Instructions: Architect"),
            "prepare_expert_files: should load architect role instructions, not general fallback"
        );
    }

    #[test]
    fn prepare_expert_files_with_role_uses_provided_role() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config::default().with_project_path(tmp.path().to_path_buf());

        std::fs::create_dir_all(config.queue_path.join("system_prompt")).ok();
        std::fs::create_dir_all(config.queue_path.join("status")).ok();

        let prepared = prepare_expert_files_with_role(&config, 0, "general", None).unwrap();

        let content = std::fs::read_to_string(prepared.instruction_file.unwrap()).unwrap();
        assert!(
            !content.contains("Expert Instructions: Architect"),
            "prepare_expert_files_with_role: should use 'general' role, not default 'architect'"
        );
        assert!(
            content.contains("Quality Principles"),
            "prepare_expert_files_with_role: should contain general role content"
        );
    }
}

/// Resolve and validate an existing session, returning its TmuxManager and metadata.
///
/// Handles the common pattern across commands: resolve session name, check existence, load metadata.
pub async fn resolve_existing_session(
    session_name: Option<String>,
) -> Result<(TmuxManager, SessionMetadata)> {
    let session_name = match session_name {
        Some(name) => name,
        None => resolve_single_session_default().await?,
    };

    let tmux = TmuxManager::new(session_name.clone());

    if !tmux.session_exists().await {
        bail!("Session {session_name} does not exist. Is it still running? Check with 'macot status'.");
    }

    let metadata = tmux.load_session_metadata().await?;
    Ok((tmux, metadata))
}
