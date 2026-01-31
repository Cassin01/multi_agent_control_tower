use crate::session::TmuxManager;
use anyhow::{bail, Result};

/// Resolve a single macot session from running sessions.
pub async fn resolve_single_session(no_sessions_msg: &str) -> Result<String> {
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
