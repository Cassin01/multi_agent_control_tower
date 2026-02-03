use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use tokio::task::JoinSet;

use crate::config::Config;
use crate::context::ContextStore;
use crate::instructions::load_instruction_with_template;
use crate::queue::QueueManager;
use crate::session::{ClaudeManager, TmuxManager};

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

    let tmux = TmuxManager::from_config(&config);

    if tmux.session_exists().await {
        bail!(
            "Session {} already exists. Run 'macot down' first.",
            config.session_name()
        );
    }

    println!("Creating session: {}", config.session_name());
    println!("Number of experts: {}", config.num_experts());

    let queue = QueueManager::new(config.queue_path.clone());
    queue.init().await.context("Failed to initialize queue")?;

    let context_store = ContextStore::new(config.queue_path.clone());
    context_store
        .init_session(&config.session_hash(), config.num_experts())
        .await
        .context("Failed to initialize context store")?;

    tmux.create_session(config.num_experts(), project_path.to_str().unwrap())
        .await
        .context("Failed to create tmux session")?;

    tmux.init_session_metadata(project_path.to_str().unwrap(), config.num_experts())
        .await?;

    let claude = ClaudeManager::new(config.session_name(), context_store);

    println!("Launching {} experts in parallel...", config.num_experts());

    let mut tasks: JoinSet<Result<(u32, String, bool)>> = JoinSet::new();

    for (i, expert) in config.experts.iter().enumerate() {
        let expert_id = i as u32;
        let expert_name = expert.name.clone();
        let tmux = tmux.clone();
        let claude = claude.clone();
        let session_hash = config.session_hash();
        let working_dir = project_path.to_str().unwrap().to_string();
        let timeout = config.timeouts.agent_ready;
        let instruction = load_instruction(&config, &expert.name)?;

        tasks.spawn(async move {
            tmux.set_pane_title(expert_id, &expert_name).await?;

            claude
                .launch_claude(expert_id, &session_hash, &working_dir)
                .await?;

            let ready = claude.wait_for_ready(expert_id, timeout).await?;

            if !instruction.is_empty() {
                claude.send_instruction(expert_id, &instruction).await?;
            }

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
            println!("  [{}] {} - Ready", expert_id, name);
        } else {
            println!(
                "  [{}] {} - Timeout (may still be starting)",
                expert_id, name
            );
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

fn load_instruction(config: &Config, expert_name: &str) -> Result<String> {
    let result = load_instruction_with_template(
        &config.core_instructions_path,
        &config.role_instructions_path,
        expert_name,
    )?;
    // Note: In start command, we don't show toast for general fallback
    // because the UI is not available yet
    Ok(result.content)
}
