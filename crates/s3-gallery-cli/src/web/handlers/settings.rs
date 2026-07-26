use crate::web::handlers::{render_template, HandlerResult};
use axum::extract::State;
use serde_json::json;

use crate::web::state::AppState;

/// Settings handler -- renders the settings page.
///
/// Displays basic application configuration information, such as the
/// configured S3 bucket name.
pub async fn settings(State(state): State<AppState>) -> HandlerResult {
    tracing::info!(handler = "settings", "rendering settings");
    let host_names: Vec<String> = state.hosts.iter().map(|h| h.host_id.to_string()).collect();
    let context = json!({
        "hosts": host_names,
        "prefix": state.prefix,
    });

    render_template(&state, "settings.html", &context)
}
