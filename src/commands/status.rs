use anyhow::Result;
use clap::Args as ClapArgs;

use crate::commands::common;
use crate::config::Config;
use crate::session::ExpertStateDetector;

#[derive(ClapArgs)]
pub struct Args {
    /// Session name to check
    pub session_name: Option<String>,
}

pub async fn execute(args: Args) -> Result<()> {
    let (tmux, metadata) = common::resolve_existing_session(args.session_name).await?;

    println!("Session: {} (running)", tmux.session_name());
    println!("Project: {}", metadata.project_path);
    println!("Created: {}", metadata.created_at);
    println!("\nExperts:");

    let config = Config::default().with_num_experts(metadata.num_experts);
    let detector =
        ExpertStateDetector::new(std::path::PathBuf::from(&metadata.queue_path).join("status"));

    for (i, expert_config) in config.experts.iter().enumerate() {
        let expert_id = i as u32;
        let state = detector.detect_state(expert_id);
        println!(
            "  [{}] {:<12} {} - {}",
            expert_id,
            expert_config.name,
            state.symbol(),
            state.description()
        );
    }

    Ok(())
}
