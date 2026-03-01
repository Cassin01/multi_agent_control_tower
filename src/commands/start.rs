use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use tokio::task::JoinSet;

use crate::commands::common;
use crate::config::Config;
use crate::utils::path_to_str;

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

    println!("Starting macot session for: {}", project_path.display());

    let mut config = Config::load(args.config)?.with_project_path(project_path.clone());

    if let Some(n) = args.num_experts {
        config = config.with_num_experts(n);
    }

    println!("Creating session: {}", config.session_name());
    println!("Number of experts: {}", config.num_experts());

    let managers = common::init_session(&config, &project_path).await?;

    println!("Launching {} experts in parallel...", config.num_experts());

    let mut tasks: JoinSet<Result<(u32, String, bool)>> = JoinSet::new();

    for (i, expert) in config.experts.iter().enumerate() {
        let expert_id = i as u32;
        let expert_name = expert.name.clone();
        let tmux = managers.tmux.clone();
        let claude = managers.claude.clone();
        let working_dir = path_to_str(&project_path)?.to_string();
        let timeout = config.timeouts.agent_ready;

        let (instruction_file, agents_file, settings_file) =
            common::prepare_expert_files(&config, expert_id)?;

        tasks.spawn(async move {
            tmux.set_pane_title(expert_id, &expert_name).await?;

            claude
                .launch_claude(
                    expert_id,
                    &working_dir,
                    instruction_file.as_deref(),
                    agents_file.as_deref(),
                    settings_file.as_deref(),
                )
                .await?;

            let ready = claude.wait_for_ready(expert_id, timeout).await?;

            Ok((expert_id, expert_name, ready))
        });
    }

    let mut results: Vec<(u32, String, bool)> = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results.push(result.context("Task panicked")??);
    }

    results.sort_by_key(|(id, _, _)| *id);

    for (expert_id, name, ready) in results {
        if ready {
            println!("  [{expert_id}] {name} - Ready");
        } else {
            println!("  [{expert_id}] {name} - Timeout (may still be starting)");
        }
    }

    println!("\nSession started successfully!");
    println!("Run 'macot tower' to open the control tower UI");
    println!(
        "Run 'tmux attach -t {}' to view agents directly",
        config.session_name()
    );

    Ok(())
}
