use s3_gallery_core::db::migrate::run_full_migration;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::error::Result;
use s3_gallery_core::view::traffic::get_traffic_summary;
use sqlx::SqlitePool;

use crate::cli::Cli;

pub async fn run_traffic_summary(
    cli: &Cli,
    host: Option<String>,
    period: String,
    since: Option<String>,
    until: Option<String>,
) -> Result<()> {
    let db = create_pool(&cli.db_path).await?;
    run_full_migration(&db).await?;

    let summary = get_traffic_summary(
        &db,
        host.as_deref(),
        Some(&period),
        since.as_deref(),
        until.as_deref(),
    )
    .await?;

    println!("Traffic Summary");
    println!("{}", "\u{2500}".repeat(60));
    println!(
        "{:<25} {:>12} {:>12} {:>10}",
        "Business", "Download", "Upload", "Requests"
    );
    println!("{}", "\u{2500}".repeat(60));

    for biz in &summary.businesses {
        println!(
            "{:<25} {:>8.1} MB {:>8.1} MB {:>8}",
            biz.business,
            biz.download_bytes as f64 / 1_048_576.0,
            biz.upload_bytes as f64 / 1_048_576.0,
            biz.requests,
        );
    }

    println!("{}", "\u{2500}".repeat(60));
    println!(
        "{:<25} {:>8.1} MB {:>8.1} MB {:>8}",
        "Total",
        summary.total_download_bytes as f64 / 1_048_576.0,
        summary.total_upload_bytes as f64 / 1_048_576.0,
        summary.total_requests,
    );
    println!(
        "Estimated Cost: ${:.4} (at $0.03/GB download)",
        summary.estimated_cost
    );

    Ok(())
}

pub async fn run_traffic_live(cli: &Cli, interval: u64) -> Result<()> {
    let db = create_pool(&cli.db_path).await?;
    run_full_migration(&db).await?;
    let pool = db.get_sqlite_connection_pool().clone();

    println!("Live Traffic (refreshing every {}s)", interval);
    println!("{}", "\u{2500}".repeat(50));

    loop {
        let row: std::result::Result<(i64, i64), _> = sqlx::query_as(
            "SELECT COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
             FROM traffic_log WHERE recorded_at > datetime('now', ?)",
        )
        .bind(format!("-{} seconds", interval * 2))
        .fetch_one(&pool)
        .await;

        if let Ok((bytes, count)) = row {
            let rate = bytes as f64 / interval as f64;
            print!(
                "\rDownload: {:.1} KB/s    Requests: {}/s    ",
                rate / 1024.0,
                count / interval as i64,
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
    }
}
