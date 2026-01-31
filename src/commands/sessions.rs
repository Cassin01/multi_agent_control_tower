use anyhow::Result;

use crate::session::TmuxManager;

pub async fn execute() -> Result<()> {
    let sessions = TmuxManager::list_all_macot_sessions().await?;

    if sessions.is_empty() {
        println!("No macot sessions running.");
        return Ok(());
    }

    println!(
        "{:<18} {:<40} {:>8} {}",
        "SESSION", "PROJECT PATH", "EXPERTS", "CREATED"
    );
    println!("{}", "-".repeat(80));

    for session in sessions {
        let created = session.created_at.format("%Y-%m-%d %H:%M");
        let path = if session.project_path.len() > 38 {
            format!(
                "...{}",
                &session.project_path[session.project_path.len() - 35..]
            )
        } else {
            session.project_path.clone()
        };

        println!(
            "{:<18} {:<40} {:>8} {}",
            session.session_name, path, session.num_experts, created
        );
    }

    Ok(())
}
