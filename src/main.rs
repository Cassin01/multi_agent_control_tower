use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod config;
mod context;
mod instructions;
mod models;
mod queue;
mod session;
mod tower;
mod utils;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start(args) => commands::start::execute(args).await,
        Commands::Down(args) => commands::down::execute(args).await,
        Commands::Tower(args) => commands::tower::execute(args).await,
        Commands::Status(args) => commands::status::execute(args).await,
        Commands::Sessions => commands::sessions::execute().await,
        Commands::Reset(args) => commands::reset::execute(args).await,
    }
}
