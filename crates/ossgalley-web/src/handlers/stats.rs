use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde_json::json;

use crate::state::AppState;

/// Render a minijinja template with the given context.
///
/// # Errors
///
/// Returns an error response if template lookup or rendering fails.
fn render_template(
    state: &AppState,
    template_name: &str,
    context: &serde_json::Value,
) -> Result<Html<String>, Box<Response>> {
    let template = state
        .templates
        .get_template(template_name)
        .map_err(|e| {
            Box::new(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "template not found",
                        "detail": e.to_string()
                    })),
                )
                    .into_response(),
            )
        })?;

    let html = template.render(context).map_err(|e| {
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "template rendering failed",
                    "detail": e.to_string()
                })),
            )
                .into_response(),
        )
    })?;

    Ok(Html(html))
}

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
pub async fn stats(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let file_stats = match state.local_view.get_stats().await {
        Ok(stats) => stats,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to get stats",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    // Convert the by_category HashMap to a sorted list of {name, count} objects
    // for the template, since minijinja iteration order over HashMap is unstable.
    let mut by_category: Vec<CategoryCount> = file_stats
        .by_category
        .iter()
        .map(|(name, count)| CategoryCount {
            name: name.clone(),
            count: *count,
        })
        .collect();

    by_category.sort_by(|a, b| a.name.cmp(&b.name));

    let context = json!({
        "stats": {
            "total_files": file_stats.total_files,
            "total_size": file_stats.total_size.to_string(),
            "by_category": by_category,
        }
    });

    match render_template(&state, "stats.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}