use clap::{Parser, Subcommand};

use crate::commands::{down, reset, sessions, start, status, tower};

#[derive(Parser)]
#[command(name = "macot")]
#[command(about = "Multi Agent Control Tower - Orchestrate multiple Claude CLI instances")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize expert session with Claude agents
    Start(start::Args),

    /// Gracefully shut down expert session
    Down(down::Args),

    /// Launch the control tower UI
    Tower(tower::Args),

    /// Display current session status
    Status(status::Args),

    /// List all running macot sessions
    Sessions,

    /// Reset expert context and instructions
    Reset(reset::Args),
}
