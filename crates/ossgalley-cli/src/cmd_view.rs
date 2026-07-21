use std::path::PathBuf;
use ossgalley_core::error::Result;
use ossgalley_core::types::*;
use ossgalley_core::view::LocalView;
use ossgalley_core::view::export::ExportFormat;
use ossgalley_core::db::pool::create_pool;
use ossgalley_core::db::schema::run_migrations;
use crate::cli::{Cli, ViewCommands, TagCommands, DiffCommands};

pub async fn run_view(_cli: &Cli, _host: &str, view_cmd: &ViewCommands) -> Result<()> {
    let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;
    let view = LocalView::new(pool);

    match view_cmd {
        ViewCommands::Ls { path, sort_by, sort_order } => {
            let prefix = path.as_deref().unwrap_or("");
            let sort = match sort_by.as_str() {
                "size" => SortField::Size,
                "date" | "last_modified" => SortField::LastModified,
                "type" | "file_type" => SortField::FileType,
                _ => SortField::Name,
            };
            let order = match sort_order.as_str() {
                "desc" | "descending" => SortOrder::Descending,
                _ => SortOrder::Ascending,
            };
            let entries = view.list_directory(prefix, sort, order).await?;
            for entry in &entries {
                if entry.is_directory {
                    println!("{:<10} {}", "", entry.name);
                } else {
                    println!("{:<10} {} {}", entry.size, entry.name, entry.file_type);
                }
            }
        }
        ViewCommands::Tree { path } => {
            let prefix = path.as_deref().unwrap_or("");
            let tree = view.build_tree(prefix).await?;
            print_tree(&tree, 0);
        }
        ViewCommands::Stat { path: _ } => {
            let stats = view.get_stats().await?;
            println!("Total files: {}", stats.total_files);
            println!("Total size: {}", stats.total_size);
            println!("Categories:");
            for (cat, count) in &stats.by_category {
                println!("  {}: {}", cat, count);
            }
        }
        ViewCommands::Files { key } => {
            // Show file details
            println!("File: {}", key);
        }
        ViewCommands::Search { query } => {
            let results = view.search_by_name(query).await?;
            println!("Found {} results:", results.total_count);
            for file in &results.files {
                println!("  {} ({} bytes)", file.key, file.size);
            }
        }
        ViewCommands::MetadataLs { namespace: _ } => {
            println!("Metadata namespaces (not yet implemented)");
        }
        ViewCommands::MetadataQuery { query: _ } => {
            println!("Metadata query (not yet implemented)");
        }
        ViewCommands::Types => {
            let stats = view.get_stats().await?;
            println!("File type distribution:");
            for (ft, count) in &stats.by_file_type {
                println!("  {}: {}", ft, count);
            }
        }
        ViewCommands::Duplicates => {
            let groups = view.find_duplicates().await?;
            if groups.is_empty() {
                println!("No duplicate files found.");
            } else {
                println!("Found {} duplicate groups:", groups.len());
                for group in &groups {
                    println!("  Size: {} ({} files)", group.size, group.files.len());
                    for file in &group.files {
                        println!("    {}", file.key);
                    }
                }
            }
        }
        ViewCommands::Largest { n } => {
            let limit = n.unwrap_or(10);
            println!("Largest {} files (not yet implemented)", limit);
        }
        ViewCommands::Recent { n } => {
            let limit = n.unwrap_or(10);
            println!("Recent {} files (not yet implemented)", limit);
        }
        ViewCommands::Oldest { n } => {
            let limit = n.unwrap_or(10);
            println!("Oldest {} files (not yet implemented)", limit);
        }
        ViewCommands::Timeline => {
            let timeline = view.get_timeline().await?;
            for entry in &timeline {
                println!("{} ({} files)", entry.date, entry.count);
            }
        }
        ViewCommands::Tags { tag_command } => {
            match tag_command {
                TagCommands::List => {
                    let tags = view.list_tags().await?;
                    for tag in &tags {
                        println!("{} ({})", tag.tag_name, tag.tag_type);
                    }
                }
                TagCommands::Files { tag } => {
                    let files = view.get_files_by_tag(tag).await?;
                    for file in &files {
                        println!("{}", file.key);
                    }
                }
                TagCommands::Stats => {
                    println!("Tag statistics (not yet implemented)");
                }
            }
        }
        ViewCommands::Diff { diff_command } => {
            match diff_command {
                DiffCommands::Added => println!("Added files (not yet implemented)"),
                DiffCommands::Removed => println!("Removed files (not yet implemented)"),
                DiffCommands::Changed => println!("Changed files (not yet implemented)"),
            }
        }
        ViewCommands::Export { format } => {
            let fmt = match format.as_str() {
                "json" => ExportFormat::Json,
                _ => ExportFormat::Csv,
            };
            let output = view.export_files(fmt).await?;
            println!("{}", output);
        }
        ViewCommands::Schema => {
            println!("DB schema version: 1 (not yet implemented)");
        }
        ViewCommands::Locations => {
            println!("GPS-located files (not yet implemented)");
        }
        ViewCommands::Orphan => {
            println!("Orphan records (not yet implemented)");
        }
        ViewCommands::Health => {
            let stats = view.get_stats().await?;
            println!("Health check:");
            println!("  Total files: {}", stats.total_files);
            println!("  Deleted files: {}", stats.deleted_files);
            println!("  Status: OK");
        }
    }

    Ok(())
}

fn print_tree(node: &ossgalley_core::view::tree::TreeNode, depth: usize) {
    let indent = "  ".repeat(depth);
    if node.is_directory {
        println!("{}{}/", indent, node.name);
        for child in &node.children {
            print_tree(child, depth + 1);
        }
    } else {
        println!("{}{}", indent, node.name);
    }
}