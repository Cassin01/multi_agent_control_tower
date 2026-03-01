use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use std::path::PathBuf;

use crate::commands::common::{self, exit_expert_and_set_pending, prepare_expert_files_with_role};
use crate::config::Config;
use crate::context::ContextStore;
use crate::session::{ClaudeManager, ExpertStateDetector};

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
    let (tmux, metadata) = common::resolve_existing_session(session).await?;
    let session_name = tmux.session_name().to_string();
    let project_path = metadata.project_path.unwrap_or_else(|| ".".to_string());
    let num_experts = metadata.num_experts.unwrap_or(4);

    let config = Config::default()
        .with_project_path(PathBuf::from(&project_path))
        .with_num_experts(num_experts);

    let expert_id = config.resolve_expert_id(&expert)?;
    let expert_name = config.get_expert_name(expert_id);

    println!("Resetting expert {expert_id} ({expert_name})...");

    let session_hash = session_name.strip_prefix("macot-").unwrap_or(&session_name);
    let context_store = ContextStore::new(config.queue_path.clone());
    let claude = ClaudeManager::new(session_name.clone());

    // Load session roles to get current role for instruction loading
    let instruction_role = match context_store.load_session_roles(session_hash).await {
        Ok(Some(roles)) => roles
            .get_role(expert_id)
            .map(ToString::to_string)
            .unwrap_or_else(|| config.get_expert_role(expert_id)),
        Ok(None) => config.get_expert_role(expert_id), // No session roles file
        Err(e) => {
            eprintln!("Warning: Failed to load session roles: {e}");
            config.get_expert_role(expert_id)
        }
    };

    println!("  Sending /exit to Claude...");
    let detector = ExpertStateDetector::new(config.queue_path.join("status"));
    exit_expert_and_set_pending(&claude, &detector, expert_id).await?;

    if full {
        println!("  Clearing context (full)...");
        context_store
            .clear_expert_context(session_hash, expert_id)
            .await?;
    } else {
        println!("  Clearing context (keep_history={keep_history})...");

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
    }

    println!("  Loading instructions (role: {instruction_role})...");
    let prepared = prepare_expert_files_with_role(&config, expert_id, &instruction_role, None)?;
    if prepared.used_general_fallback {
        println!(
            "  Warning: Role '{}' not found, using 'general' instructions",
            prepared.requested_role
        );
    }

    println!("  Restarting Claude...");
    claude
        .launch_claude(
            expert_id,
            &project_path,
            prepared.instruction_file.as_deref(),
            prepared.agents_file.as_deref(),
            prepared.settings_file.as_deref(),
        )
        .await?;

    println!("Expert {expert_id} reset complete.");
    Ok(())
}
