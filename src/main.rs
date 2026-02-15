use anyhow::Result;
use clap::Parser;
use tracing_subscriber::fmt::format::FmtSpan;

mod cli;
mod commands;
mod config;
mod context;
mod experts;
mod feature;
mod instructions;
mod models;
mod queue;
mod session;
mod tower;
mod utils;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(debug_assertions)]
    {
        // Log to file only in debug builds.
        let file_appender = tracing_appender::rolling::never("/tmp", "macot-debug.log");
        tracing_subscriber::fmt()
            .with_writer(file_appender)
            .with_span_events(FmtSpan::CLOSE)
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

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
