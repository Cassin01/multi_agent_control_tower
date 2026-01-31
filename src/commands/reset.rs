use anyhow::{bail, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::path::PathBuf;

use crate::config::Config;
use crate::context::ContextStore;
use crate::session::{ClaudeManager, TmuxManager};

#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub command: ResetCommand,
}

#[derive(Subcommand)]
pub enum ResetCommand {
    /// Reset a specific expert
    Expert {
        /// Expert ID or name
        expert: String,

        /// Session name (optional if only one session)
        #[arg(short, long)]
        session: Option<String>,

        /// Keep conversation history
        #[arg(long)]
        keep_history: bool,

        /// Full reset including Claude session restart
        #[arg(long)]
        full: bool,
    },
}

pub async fn execute(args: Args) -> Result<()> {
    match args.command {
        ResetCommand::Expert {
            expert,
            session,
            keep_history,
            full,
        } => reset_expert(expert, session, keep_history, full).await,
    }
}

async fn reset_expert(
    expert: String,
    session: Option<String>,
    keep_history: bool,
    full: bool,
) -> Result<()> {
    let session_name = match session {
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
        .unwrap_or_else(|| ".".to_string());

    let num_experts = tmux
        .get_env("MACOT_NUM_EXPERTS")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let config = Config::default()
        .with_project_path(PathBuf::from(&project_path))
        .with_num_experts(num_experts);

    let expert_id = resolve_expert_id(&expert, &config)?;
    let expert_name = config
        .get_expert(expert_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("expert{}", expert_id));

    println!(
        "Resetting expert {} ({})...",
        expert_id, expert_name
    );

    let session_hash = session_name
        .strip_prefix("macot-")
        .unwrap_or(&session_name);
    let context_store = ContextStore::new(config.queue_path.clone());
    let claude = ClaudeManager::new(session_name.clone(), context_store.clone());

    if full {
        println!("  Sending /exit to Claude...");
        claude.send_exit(expert_id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        println!("  Clearing context...");
        context_store
            .clear_expert_context(session_hash, expert_id)
            .await?;

        println!("  Restarting Claude...");
        claude
            .launch_claude(expert_id, session_hash, &project_path)
            .await?;
    } else {
        println!("  Clearing context (keep_history={})...", keep_history);

        if !keep_history {
            context_store
                .clear_expert_context(session_hash, expert_id)
                .await?;
        } else {
            if let Some(mut ctx) = context_store
                .load_expert_context(session_hash, expert_id)
                .await?
            {
                ctx.clear_knowledge();
                context_store.save_expert_context(&ctx).await?;
            }
        }

        println!("  Sending /clear to Claude...");
        claude.send_clear(expert_id).await?;
    }

    println!("  Resending instructions...");
    let instruction = load_instruction(&config, &expert_name)?;
    if !instruction.is_empty() {
        claude.send_instruction(expert_id, &instruction).await?;
    }

    println!("Expert {} reset complete.", expert_id);
    Ok(())
}

fn resolve_expert_id(expert: &str, config: &Config) -> Result<u32> {
    if let Ok(id) = expert.parse::<u32>() {
        if id < config.experts.len() as u32 {
            return Ok(id);
        }
        bail!(
            "Expert ID {} out of range (0-{})",
            id,
            config.experts.len() - 1
        );
    }

    if let Some((id, _)) = config.get_expert_by_name(expert) {
        return Ok(id);
    }

    bail!("Unknown expert: {}", expert)
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

async fn resolve_single_session() -> Result<String> {
    let sessions = TmuxManager::list_all_macot_sessions().await?;

    match sessions.len() {
        0 => bail!("No macot sessions running"),
        1 => Ok(sessions[0].session_name.clone()),
        _ => {
            eprintln!("Multiple sessions running. Please specify one with --session:");
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
