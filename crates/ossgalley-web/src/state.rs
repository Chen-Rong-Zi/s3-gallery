use std::sync::Arc;

use ossgalley_core::s3::client::S3Client;
use ossgalley_core::types::BucketName;
use ossgalley_core::view::LocalView;
use ossgalley_core::view::RemoteView;

/// Shared application state.
#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub local_view: Arc<LocalView>,
    pub remote_view: Arc<RemoteView>,
    pub bucket: BucketName,
}

impl AppState {
    /// Create a new AppState with the given components.
    pub fn new(
        local_view: LocalView,
        s3: Arc<dyn S3Client>,
        bucket: BucketName,
    ) -> Self {
        let mut templates = minijinja::Environment::new();

        // Register templates
        let template_names: &[(&str, &str)] = &[
            ("browse.html", include_str!("../templates/browse.html")),
            ("browse_table.html", include_str!("../templates/browse_table.html")),
            ("gallery.html", include_str!("../templates/gallery.html")),
            ("gallery_items.html", include_str!("../templates/gallery_items.html")),
            ("file_detail.html", include_str!("../templates/file_detail.html")),
            ("search.html", include_str!("../templates/search.html")),
            ("search_results.html", include_str!("../templates/search_results.html")),
            ("timeline.html", include_str!("../templates/timeline.html")),
            ("tags.html", include_str!("../templates/tags.html")),
            ("stats.html", include_str!("../templates/stats.html")),
            ("duplicates.html", include_str!("../templates/duplicates.html")),
            ("settings.html", include_str!("../templates/settings.html")),
            ("layout.html", include_str!("../templates/layout.html")),
        ];

        for (name, content) in template_names {
            if let Err(e) = templates.add_template(name, content) {
                tracing::warn!("Failed to register template '{name}': {e}");
            }
        }

        let remote_view = RemoteView::new(
            local_view.db().clone(),
            s3,
            bucket.clone(),
        );

        Self {
            templates: Arc::new(templates),
            local_view: Arc::new(local_view),
            remote_view: Arc::new(remote_view),
            bucket,
        }
    }
}