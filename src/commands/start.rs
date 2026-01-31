use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::ContextStore;
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

    let mut config = Config::load(args.config)?
        .with_project_path(project_path.clone());

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
    println!("Number of experts: {}", config.num_experts);

    let queue = QueueManager::new(config.queue_path.clone());
    queue.init().await.context("Failed to initialize queue")?;

    let context_store = ContextStore::new(config.queue_path.clone());
    context_store
        .init_session(&config.session_hash(), config.num_experts)
        .await
        .context("Failed to initialize context store")?;

    tmux.create_session(config.num_experts, project_path.to_str().unwrap())
        .await
        .context("Failed to create tmux session")?;

    tmux.init_session_metadata(project_path.to_str().unwrap(), config.num_experts)
        .await?;

    let claude = ClaudeManager::new(config.session_name(), context_store);

    for (i, expert) in config.experts.iter().enumerate() {
        let expert_id = i as u32;
        println!("  [{}] {} - Launching Claude...", expert_id, expert.name);

        tmux.set_pane_title(expert_id, &expert.name).await?;

        claude
            .launch_claude(
                expert_id,
                &config.session_hash(),
                project_path.to_str().unwrap(),
            )
            .await?;
    }

    println!("\nWaiting for agents to be ready...");
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    for (i, expert) in config.experts.iter().enumerate() {
        let expert_id = i as u32;
        let ready = claude
            .wait_for_ready(expert_id, config.timeouts.agent_ready)
            .await?;

        if ready {
            println!("  [{}] {} - Ready", expert_id, expert.name);

            let instruction = load_instruction(&config, &expert.name)?;
            if !instruction.is_empty() {
                claude.send_instruction(expert_id, &instruction).await?;
            }
        } else {
            println!("  [{}] {} - Timeout (may still be starting)", expert_id, expert.name);
        }
    }

    println!("\nSession started successfully!");
    println!("Run 'macot tower' to open the control tower UI");
    println!("Run 'tmux attach -t {}' to view agents directly", config.session_name());

    Ok(())
}

fn load_instruction(config: &Config, expert_name: &str) -> Result<String> {
    let core_path = config.instructions_path.join("core.md");
    let expert_path = config.instructions_path.join(format!("{}.md", expert_name));

    let mut instruction = String::new();

    if core_path.exists() {
        instruction.push_str(&std::fs::read_to_string(&core_path)?);
        instruction.push_str("\n\n");
    }

    if expert_path.exists() {
        instruction.push_str(&std::fs::read_to_string(&expert_path)?);
    }

    Ok(instruction)
}
