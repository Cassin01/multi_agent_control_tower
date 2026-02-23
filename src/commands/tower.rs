use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use crate::commands::common;
use crate::config::Config;
use crate::session::{TmuxManager, WorktreeManager};
use crate::tower::TowerApp;

#[derive(ClapArgs)]
pub struct Args {
    /// Session name to connect to
    pub session_name: Option<String>,

    /// Custom config file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

pub async fn execute(args: Args) -> Result<()> {
    let session_name = match args.session_name {
        Some(name) => name,
        None => {
            common::resolve_single_session("No macot sessions running. Run 'macot start' first.")
                .await?
        }
    };

    let tmux = TmuxManager::new(session_name.clone());

    if !tmux.session_exists().await {
        bail!("Session {session_name} does not exist. Run 'macot start' first.");
    }

    let metadata = tmux.load_session_metadata().await?;
    let project_path = metadata
        .project_path
        .context("Failed to get project path from session")?;
    let project_path_buf = PathBuf::from(&project_path);
    let num_experts = metadata.num_experts.unwrap_or(4);

    let worktree_manager = WorktreeManager::resolve(project_path_buf.clone()).await?;

    let config = Config::load(args.config)?
        .with_project_path(project_path_buf)
        .with_num_experts(num_experts);

    let mut app = TowerApp::new(config, worktree_manager);
    app.run().await?;

    Ok(())
}
