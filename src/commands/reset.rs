use anyhow::{bail, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::path::PathBuf;

use crate::commands::common;
use crate::config::Config;
use crate::context::ContextStore;
use crate::instructions::load_instruction_with_template;
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
        None => common::resolve_single_session_default().await?,
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

    let expert_id = config.resolve_expert_id(&expert)?;
    let expert_name = config.get_expert_name(expert_id);

    println!("Resetting expert {} ({})...", expert_id, expert_name);

    let session_hash = session_name.strip_prefix("macot-").unwrap_or(&session_name);
    let context_store = ContextStore::new(config.queue_path.clone());
    let claude = ClaudeManager::new(session_name.clone(), context_store.clone());

    // Load session roles to get current role for instruction loading
    let instruction_role = match context_store.load_session_roles(session_hash).await {
        Ok(Some(roles)) => roles
            .get_role(expert_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| config.get_expert_role(expert_id)),
        Ok(None) => config.get_expert_role(expert_id), // No session roles file
        Err(e) => {
            eprintln!("Warning: Failed to load session roles: {}", e);
            config.get_expert_role(expert_id)
        }
    };

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
        } else if let Some(mut ctx) = context_store
            .load_expert_context(session_hash, expert_id)
            .await?
        {
            ctx.clear_knowledge();
            context_store.save_expert_context(&ctx).await?;
        }

        println!("  Sending /clear to Claude...");
        claude.send_clear(expert_id).await?;
    }

    println!("  Resending instructions (role: {})...", instruction_role);
    let instruction_result = load_instruction_with_template(
        &config.core_instructions_path,
        &config.role_instructions_path,
        &instruction_role,
        expert_id,
        &expert_name,
        &config.status_file_path(expert_id),
    )?;
    if !instruction_result.content.is_empty() {
        claude.send_instruction(expert_id, &instruction_result.content).await?;
    }
    if instruction_result.used_general_fallback {
        println!(
            "  Warning: Role '{}' not found, using 'general' instructions",
            instruction_result.requested_role
        );
    }

    println!("Expert {} reset complete.", expert_id);
    Ok(())
}
