use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::web::state::AppState;

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
    let template = state.templates.get_template(template_name).map_err(|e| {
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

/// Settings handler -- renders the settings page.
///
/// Displays basic application configuration information, such as the
/// configured S3 bucket name.
pub async fn settings(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!(handler = "settings", "rendering settings");
    let host_names: Vec<String> = state.hosts.iter().map(|h| h.host_id.clone()).collect();
    let context = json!({
        "hosts": host_names,
        "prefix": state.prefix,
    });

    match render_template(&state, "settings.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}
