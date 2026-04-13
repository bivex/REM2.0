mod cli;
mod handlers;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Commands};
use handlers::{handle_extract, handle_verify};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // ── Initialise structured logging ─────────────────────────────────────
    let filter = EnvFilter::try_new(&cli.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if cli.json_logs {
        tracing_subscriber::fmt()
            .json()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .init();
    }

    // ── Dispatch to the appropriate handler ───────────────────────────────
    match cli.command {
        Commands::Extract(args) => handle_extract(args),
        Commands::Verify(args)  => handle_verify(args),
    }
}
