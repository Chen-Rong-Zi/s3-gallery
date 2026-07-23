mod cli;
mod cmd_db;
mod cmd_init;
mod cmd_scan;
mod cmd_serve;
mod cmd_view;
mod web;

use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Init {
            bucket,
            prefix,
            name,
            description,
        } => {
            if let Err(e) = cmd_init::run_init(
                &cli,
                bucket,
                prefix.as_deref(),
                name,
                description.as_deref(),
            )
            .await
            {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Scan(command) => match command {
            cli::ScanCommand::Init {
                prefix,
                incremental,
                schedule,
                force,
                no_metadata,
                with_thumbnails,
                concurrency,
                confirm,
            } => {
                let opts = cmd_scan::ScanOptions {
                    incremental: *incremental,
                    schedule: *schedule,
                    force: *force,
                    extract_metadata: !no_metadata,
                    with_thumbnails: *with_thumbnails,
                    concurrency: *concurrency,
                };
                if let Err(e) = cmd_scan::run_init(&cli, prefix.clone(), opts, *confirm).await {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            cli::ScanCommand::Update {
                prefix,
                incremental,
                schedule,
                force,
                no_metadata,
                with_thumbnails,
                concurrency,
            } => {
                let opts = cmd_scan::ScanOptions {
                    incremental: *incremental,
                    schedule: *schedule,
                    force: *force,
                    extract_metadata: !no_metadata,
                    with_thumbnails: *with_thumbnails,
                    concurrency: *concurrency,
                };
                if let Err(e) = cmd_scan::run_update(&cli, prefix.clone(), opts).await {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            cli::ScanCommand::Sync {
                prefix,
                incremental,
                schedule,
                force,
                no_metadata,
                with_thumbnails,
                concurrency,
            } => {
                let opts = cmd_scan::ScanOptions {
                    incremental: *incremental,
                    schedule: *schedule,
                    force: *force,
                    extract_metadata: !no_metadata,
                    with_thumbnails: *with_thumbnails,
                    concurrency: *concurrency,
                };
                if let Err(e) = cmd_scan::run_sync(&cli, prefix.clone(), opts).await {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        },
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
        Commands::Serve {
            prefix,
            port,
            readonly,
        } => {
            if let Err(e) = cmd_serve::run_serve(&cli, *port, *readonly, prefix.clone()).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
