#![forbid(unsafe_code)]
#![deny(unreachable_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::let_underscore_must_use)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]

/// All built-in templates, embedded at compile time via `include_str!`.
const TEMPLATES: &[(&str, &str)] = &[
    ("browse.html", include_str!("../templates/browse.html")),
    (
        "browse_table.html",
        include_str!("../templates/browse_table.html"),
    ),
    ("gallery.html", include_str!("../templates/gallery.html")),
    (
        "gallery_items.html",
        include_str!("../templates/gallery_items.html"),
    ),
    (
        "file_detail.html",
        include_str!("../templates/file_detail.html"),
    ),
    ("search.html", include_str!("../templates/search.html")),
    (
        "search_results.html",
        include_str!("../templates/search_results.html"),
    ),
    ("tags.html", include_str!("../templates/tags.html")),
    ("traffic.html", include_str!("../templates/traffic.html")),
    ("stats.html", include_str!("../templates/stats.html")),
    (
        "duplicates.html",
        include_str!("../templates/duplicates.html"),
    ),
    ("settings.html", include_str!("../templates/settings.html")),
    ("layout.html", include_str!("../templates/layout.html")),
];

/// Register all built-in templates into a minijinja `Environment`.
///
/// Returns a list of template names that failed to register (empty on success).
/// Failures are non-fatal — the caller can decide how to handle them.
pub fn register_templates(env: &mut minijinja::Environment<'static>) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, content) in TEMPLATES {
        if let Err(e) = env.add_template(name, content) {
            tracing::warn!("s3-gallery-web: failed to register template '{name}': {e}");
            errors.push(name.to_string());
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_register_all_templates() {
        let mut env = minijinja::Environment::new();
        let errors = register_templates(&mut env);
        assert!(
            errors.is_empty(),
            "templates failed to register: {errors:?}"
        );
    }

    #[test]
    fn test_render_browse_template() {
        let mut env = minijinja::Environment::new();
        register_templates(&mut env);
        let tmpl = env.get_template("browse.html").unwrap();
        let result = tmpl.render(serde_json::json!({
            "entries": [],
            "breadcrumbs": [],
            "sort_by": "name",
            "sort_order": "asc",
        }));
        assert!(
            result.is_ok(),
            "browse.html rendering failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_render_gallery_template() {
        let mut env = minijinja::Environment::new();
        register_templates(&mut env);
        let tmpl = env.get_template("gallery.html").unwrap();
        let result = tmpl.render(serde_json::json!({
            "groups": [],
            "page": 0,
            "has_more": false,
            "tag": null,
            "all_tags": [],
        }));
        assert!(
            result.is_ok(),
            "gallery.html rendering failed: {:?}",
            result.err()
        );
    }
}
