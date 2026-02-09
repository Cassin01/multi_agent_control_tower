use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use tokio::time::{sleep, Duration};

use crate::commands::common;
use crate::config::Config;
use crate::context::ContextStore;
use crate::session::{ClaudeManager, TmuxManager};

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
    let session_name = match args.session_name {
        Some(name) => name,
        None => common::resolve_single_session_default().await?,
    };

    println!("Stopping session: {}", session_name);

    let tmux = TmuxManager::new(session_name.clone());

    if !tmux.session_exists().await {
        bail!("Session {} does not exist", session_name);
    }

    let num_experts = tmux
        .get_env("MACOT_NUM_EXPERTS")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let project_path = tmux
        .get_env("MACOT_PROJECT_PATH")
        .await?
        .unwrap_or_else(|| ".".to_string());

    if !args.force {
        println!("Sending exit commands to {} agents...", num_experts);

        let claude = ClaudeManager::new(session_name.clone());

        for i in 0..num_experts {
            if let Err(e) = claude.send_exit(i).await {
                eprintln!("  Warning: Failed to send exit to expert {}: {}", i, e);
            }
        }

        println!("Waiting for graceful shutdown ({} seconds)...", 10);
        sleep(Duration::from_secs(10)).await;
    }

    println!("Killing tmux session...");
    tmux.kill_session()
        .await
        .context("Failed to kill tmux session")?;

    if args.cleanup {
        println!("Cleaning up session data...");

        let session_hash = session_name.strip_prefix("macot-").unwrap_or(&session_name);

        let config = Config::default().with_project_path(std::path::PathBuf::from(&project_path));
        let context_store = ContextStore::new(config.queue_path.clone());

        if let Err(e) = context_store.cleanup_session(session_hash).await {
            eprintln!("Warning: Failed to clean up context: {}", e);
        }
    }

    println!("Session {} stopped successfully", session_name);
    Ok(())
}
