use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use crate::commands::common;
use crate::config::Config;
use crate::session::TmuxManager;
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
        None => common::resolve_single_session("No macot sessions running. Run 'macot start' first.").await?,
    };

    let tmux = TmuxManager::new(session_name.clone());

    if !tmux.session_exists().await {
        bail!(
            "Session {} does not exist. Run 'macot start' first.",
            session_name
        );
    }

    let project_path = tmux
        .get_env("MACOT_PROJECT_PATH")
        .await?
        .context("Failed to get project path from session")?;

    let num_experts = tmux
        .get_env("MACOT_NUM_EXPERTS")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let config = Config::load(args.config)?
        .with_project_path(PathBuf::from(project_path))
        .with_num_experts(num_experts);

    let mut app = TowerApp::new(config);
    app.run().await?;

    Ok(())
}
