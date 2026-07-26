use std::sync::Arc;

use s3_gallery_core::entity::host_config::Model as HostConfigEntry;
use s3_gallery_core::s3::layers::TrafficLayer;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use sea_orm::DatabaseConnection;
use tower::ServiceBuilder;

/// Shared application state for multi-host serving.
#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub db: DatabaseConnection,
    /// All hosts from the database.
    pub hosts: Vec<HostConfigEntry>,
    /// Tower-composed S3 service stack.
    pub s3_stack: S3Service,
    /// Optional traffic recorder.
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
    /// Optional prefix filter from CLI --prefix.
    pub prefix: Option<String>,
    /// CLI region for fallback when host has no stored region.
    pub cli_region: String,
    /// CLI access key for S3 connections.
    #[allow(dead_code)]
    pub access_key: String,
    /// CLI secret key for S3 connections.
    #[allow(dead_code)]
    pub secret_key: String,
}

impl AppState {
    /// Find a host config by host_id.
    pub fn get_host(&self, host_id: &str) -> Option<&HostConfigEntry> {
        self.hosts.iter().find(|h| h.host_id.as_str() == host_id)
    }

    /// Get the effective region for a host (stored or CLI fallback).
    #[allow(dead_code)]
    pub fn effective_region<'a>(&'a self, host: &'a HostConfigEntry) -> &'a str {
        if host.region.is_empty() {
            &self.cli_region
        } else {
            &host.region
        }
    }

    /// Get an S3Service with per-business traffic recording.
    pub fn s3_with_traffic(&self, host_id: &str, business: &str) -> S3Service {
        if let Some(ref recorder) = self.traffic_recorder {
            ServiceBuilder::new()
                .layer(TrafficLayer::new(recorder.clone(), host_id, business))
                .service(self.s3_stack.clone())
        } else {
            self.s3_stack.clone()
        }
    }
}
