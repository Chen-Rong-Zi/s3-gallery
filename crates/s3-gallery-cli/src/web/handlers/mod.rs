use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub mod browse;
pub mod dashboard;
pub mod download;
pub mod duplicates;
pub mod file_detail;
pub mod gallery;
pub mod search;
pub mod settings;
pub mod stats;
pub mod tags;
pub mod thumbnail;

pub use browse::browse;
pub use dashboard::dashboard;
pub use download::download;
pub use duplicates::duplicates;
pub use file_detail::file_detail;
pub use gallery::gallery;
pub use search::search;
pub use settings::settings;
pub use stats::stats;
pub use tags::tags;
pub use thumbnail::thumbnail;

/// Unified handler return type — eliminates 15 lines of boilerplate per handler.
pub enum HandlerResult {
    Html(String),
    Json(serde_json::Value),
    Redirect(String),
    Error(StatusCode, serde_json::Value),
}

impl IntoResponse for HandlerResult {
    fn into_response(self) -> Response {
        match self {
            HandlerResult::Html(html) => Html(html).into_response(),
            HandlerResult::Json(json) => Json(json).into_response(),
            HandlerResult::Redirect(url) => axum::response::Redirect::to(&url).into_response(),
            HandlerResult::Error(status, json) => (status, Json(json)).into_response(),
        }
    }
}

/// Shared template renderer — used by all handlers.
pub fn render_template(
    state: &crate::web::state::AppState,
    template_name: &str,
    context: &serde_json::Value,
) -> HandlerResult {
    match state.templates.get_template(template_name) {
        Ok(tmpl) => match tmpl.render(context) {
            Ok(html) => HandlerResult::Html(html),
            Err(e) => HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "template rendering failed", "detail": e.to_string()}),
            ),
        },
        Err(e) => HandlerResult::Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "template not found", "detail": e.to_string()}),
        ),
    }
}