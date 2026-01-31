use anyhow::{bail, Result};
use clap::Args as ClapArgs;

use crate::config::Config;
use crate::session::{CaptureManager, TmuxManager};

#[derive(ClapArgs)]
pub struct Args {
    /// Session name to check
    pub session_name: Option<String>,
}

pub async fn execute(args: Args) -> Result<()> {
    let session_name = match args.session_name {
        Some(name) => name,
        None => resolve_single_session().await?,
    };

    let tmux = TmuxManager::new(session_name.clone());

    if !tmux.session_exists().await {
        bail!("Session {} does not exist", session_name);
    }

    let project_path = tmux
        .get_env("MACOT_PROJECT_PATH")
        .await?
        .unwrap_or_else(|| "unknown".to_string());

    let num_experts = tmux
        .get_env("MACOT_NUM_EXPERTS")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let created_at = tmux
        .get_env("MACOT_CREATED_AT")
        .await?
        .unwrap_or_else(|| "unknown".to_string());

    println!("Session: {} (running)", session_name);
    println!("Project: {}", project_path);
    println!("Created: {}", created_at);
    println!("\nExperts:");

    let config = Config::default().with_num_experts(num_experts);
    let capture = CaptureManager::new(session_name);

    let experts: Vec<(u32, String)> = config
        .experts
        .iter()
        .enumerate()
        .map(|(i, e)| (i as u32, e.name.clone()))
        .collect();

    let captures = capture.capture_all(&experts).await;

    for cap in captures {
        println!(
            "  [{}] {:<12} {} - {}",
            cap.expert_id,
            cap.expert_name,
            cap.status.symbol(),
            cap.status.description()
        );
    }

    Ok(())
}

async fn resolve_single_session() -> Result<String> {
    let sessions = TmuxManager::list_all_macot_sessions().await?;

    match sessions.len() {
        0 => bail!("No macot sessions running"),
        1 => Ok(sessions[0].session_name.clone()),
        _ => {
            eprintln!("Multiple sessions running. Please specify one:");
            for session in &sessions {
                eprintln!(
                    "  {} - {}",
                    session.session_name, session.project_path
                );
            }
            bail!("Multiple sessions running, please specify session name")
        }
    }
}
