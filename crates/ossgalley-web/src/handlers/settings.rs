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

/// Settings handler -- renders the settings page.
///
/// Displays basic application configuration information, such as the
/// configured S3 bucket name.
pub async fn settings(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let context = json!({
        "bucket": state.bucket.to_string(),
    });

    match render_template(&state, "settings.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}