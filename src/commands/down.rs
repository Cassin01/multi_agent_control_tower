use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use tokio::time::{sleep, Duration};

use crate::commands::common;
use crate::config::Config;
use crate::context::ContextStore;
use crate::session::ClaudeManager;

#[derive(ClapArgs)]
pub struct Args {
    /// Session name to stop (e.g., macot-a1b2c3d4)
    pub session_name: Option<String>,

    /// Force kill without graceful shutdown
    #[arg(short, long)]
    pub force: bool,

    /// Clean up context and queue files
    #[arg(long)]
    pub cleanup: bool,
}

pub async fn execute(args: Args) -> Result<()> {
    let (tmux, metadata) = common::resolve_existing_session(args.session_name).await?;
    let session_name = tmux.session_name().to_string();

    println!("Stopping session: {session_name}");

    if !args.force {
        let num_experts = metadata.num_experts;
        println!("Sending exit commands to {num_experts} agents...");

        let claude = ClaudeManager::new(session_name.clone());

        for i in 0..num_experts {
            if let Err(e) = claude.send_exit(i).await {
                eprintln!("  Warning: Failed to send exit to expert {i}: {e}");
            }
        }

        let delay = Duration::from_secs(2);
        println!(
            "Waiting for graceful shutdown ({} seconds)...",
            delay.as_secs_f32()
        );
        sleep(delay).await;
    }

    println!("Killing tmux session...");
    tmux.kill_session()
        .await
        .context("Failed to kill tmux session")?;

    if args.cleanup {
        println!("Cleaning up session data...");

        let session_hash = session_name.strip_prefix("macot-").unwrap_or(&session_name);

        let config =
            Config::default().with_project_path(std::path::PathBuf::from(&metadata.project_path));
        let context_store = ContextStore::new(config.queue_path.clone());

        if let Err(e) = context_store.cleanup_session(session_hash).await {
            eprintln!("Warning: Failed to clean up context: {e}");
        }
    }

    println!("Session {session_name} stopped successfully");
    Ok(())
}
