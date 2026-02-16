use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use crate::commands::common;
use crate::config::Config;
use crate::session::WorktreeManager;
use crate::tower::TowerApp;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to project directory (default: current directory)
    #[arg(default_value = ".")]
    pub project_path: PathBuf,

    /// Number of experts (overrides config)
    #[arg(short = 'n', long)]
    pub num_experts: Option<u32>,

    /// Custom config file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

pub async fn execute(args: Args) -> Result<()> {
    let project_path = args
        .project_path
        .canonicalize()
        .context("Failed to resolve project path")?;

    println!("Launching macot session for: {}", project_path.display());

    let mut config = Config::load(args.config)?.with_project_path(project_path.clone());

    if let Some(n) = args.num_experts {
        config = config.with_num_experts(n);
    }

    println!("Creating session: {}", config.session_name());
    println!("Number of experts: {}", config.num_experts());

    let managers = common::init_session(&config, &project_path).await?;

    let config_clone = config.clone();
    let tmux_clone = managers.tmux.clone();
    let claude_clone = managers.claude.clone();
    let project_path_clone = project_path.clone();

    tokio::spawn(async move {
        let config = config_clone;
        let tmux = tmux_clone;
        let claude = claude_clone;
        let project_path = project_path_clone;

        for (i, expert) in config.experts.iter().enumerate() {
            let expert_id = i as u32;
            let expert_name = expert.name.clone();
            let working_dir = project_path.to_str().unwrap().to_string();
            let timeout = config.timeouts.agent_ready;

            let (instruction_file, agents_file, settings_file) =
                match common::prepare_expert_files(&config, expert_id) {
                    Ok(files) => files,
                    Err(e) => {
                        eprintln!("Failed to prepare files for expert {}: {}", expert_id, e);
                        continue;
                    }
                };

            if let Err(e) = tmux.set_pane_title(expert_id, &expert_name).await {
                eprintln!("Failed to set pane title for expert {}: {}", expert_id, e);
            }

            if let Err(e) = claude
                .launch_claude(
                    expert_id,
                    &working_dir,
                    instruction_file.as_deref(),
                    agents_file.as_deref(),
                    settings_file.as_deref(),
                )
                .await
            {
                eprintln!("Failed to launch Claude for expert {}: {}", expert_id, e);
                continue;
            }

            match claude.wait_for_ready(expert_id, timeout).await {
                Ok(true) => {
                    tracing::info!("Expert {} ({}) ready", expert_id, expert_name);
                }
                Ok(false) => {
                    tracing::warn!("Expert {} ({}) timeout", expert_id, expert_name);
                }
                Err(e) => {
                    tracing::error!("Expert {} ({}) failed: {}", expert_id, expert_name, e);
                }
            }
        }
    });

    println!("Session infrastructure ready. Launching experts in background...");

    let worktree_manager = WorktreeManager::resolve(project_path).await?;
    let mut app = TowerApp::new(config, worktree_manager);
    app.run().await?;

    Ok(())
}
