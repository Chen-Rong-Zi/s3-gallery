use crate::web::handlers::{render_template, HandlerResult};
use axum::{extract::State, http::StatusCode};
use s3_gallery_core::view::stat as stat_view;
use serde_json::json;

use crate::web::state::AppState;

/// A category count entry for template rendering.
#[derive(Debug, Clone, serde::Serialize)]
struct CategoryCount {
    name: String,
    count: u64,
}

/// Stats handler -- renders the statistics page.
///
/// Displays file statistics including total files, total size, and
/// breakdown by category.
pub async fn stats(State(state): State<AppState>) -> HandlerResult {
    tracing::info!(handler = "stats", "computing stats");

    let (total_stats, per_host_stats) = match stat_view::get_all_host_stats(&state.db).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(handler = "stats", error = %e, "failed to get stats");
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": "failed to get stats",
                    "detail": e.to_string()
                }),
            );
        }
    };

    // Convert by_category HashMap to sorted list for template
    let mut by_category: Vec<CategoryCount> = total_stats
        .by_category
        .iter()
        .map(|(name, count)| CategoryCount {
            name: name.clone(),
            count: *count,
        })
        .collect();
    by_category.sort_by(|a, b| a.name.cmp(&b.name));

    // Build per-host list
    let per_host_list: Vec<serde_json::Value> = per_host_stats
        .iter()
        .map(|(hid, stats)| {
            let mut host_cat: Vec<CategoryCount> = stats
                .by_category
                .iter()
                .map(|(name, count)| CategoryCount {
                    name: name.clone(),
                    count: *count,
                })
                .collect();
            host_cat.sort_by(|a, b| a.name.cmp(&b.name));
            json!({
                "host_id": hid,
                "stats": {
                    "total_files": stats.total_files,
                    "total_size": stats.total_size.to_string(),
                    "by_category": host_cat,
                }
            })
        })
        .collect();

    let context = json!({
        "totals": {
            "total_files": total_stats.total_files,
            "total_size": total_stats.total_size.to_string(),
            "by_category": by_category,
        },
        "per_host": per_host_list,
    });

    render_template(&state, "stats.html", &context)
}
