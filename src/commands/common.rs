use crate::session::TmuxManager;
use crate::utils::compute_path_hash;
use anyhow::{bail, Result};

/// Try to find a running session that matches the current directory's hash.
/// Returns the session name if exactly one match is found.
async fn resolve_session_by_cwd() -> Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    let hash = compute_path_hash(&cwd);
    let expected_suffix = format!("-{}", hash);

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
        0 => bail!("{}", no_sessions_msg),
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
