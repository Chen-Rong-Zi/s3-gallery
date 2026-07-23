use axum::extract::State;
use crate::web::handlers::{HandlerResult, render_template};
use serde_json::json;

use crate::web::state::AppState;

/// Settings handler -- renders the settings page.
///
/// Displays basic application configuration information, such as the
/// configured S3 bucket name.
pub async fn settings(State(state): State<AppState>) -> HandlerResult {
    tracing::info!(handler = "settings", "rendering settings");
    let host_names: Vec<String> = state.hosts.iter().map(|h| h.host_id.clone()).collect();
    let context = json!({
        "hosts": host_names,
        "prefix": state.prefix,
    });

    render_template(&state, "settings.html", &context)
}
