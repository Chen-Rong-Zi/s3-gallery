mod cli;
mod cmd_init;
mod cmd_scan;
mod cmd_view;
mod cmd_db;
mod cmd_serve;

use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Init { host } => {
            if let Err(e) = cmd_init::run_init(&cli, host).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Scan { host, incremental, schedule, force, no_metadata, with_thumbnails, concurrency } => {
            let opts = cmd_scan::ScanOptions {
                incremental: *incremental,
                schedule: *schedule,
                force: *force,
                extract_metadata: !no_metadata,
                with_thumbnails: *with_thumbnails,
                concurrency: *concurrency,
            };
            if let Err(e) = cmd_scan::run_scan(&cli, host, opts).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::View { host, view_command } => {
            if let Err(e) = cmd_view::run_view(&cli, host, view_command).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Db { host, db_command } => {
            if let Err(e) = cmd_db::run_db(&cli, host, db_command).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Serve { host, port, readonly } => {
            if let Err(e) = cmd_serve::run_serve(&cli, host, *port, *readonly).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}