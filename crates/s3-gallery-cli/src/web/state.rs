use std::collections::HashMap;
use std::sync::Arc;

use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use sqlx::SqlitePool;

/// Shared application state for multi-host serving.
#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub db: SqlitePool,
    /// All hosts from the database.
    pub hosts: Vec<HostConfigEntry>,
    /// S3 clients keyed by endpoint URL (endpoint "" = CLI default).
    pub s3_clients: HashMap<String, Arc<dyn S3Client>>,
    /// Tower-composed S3 service stack.
    pub s3_stack: S3Service,
    /// Optional traffic recorder.
    pub traffic_recorder: Option<Arc<TrafficRecorder>>,
    /// Optional prefix filter from CLI --prefix.
    pub prefix: Option<String>,
    /// CLI endpoint for fallback when host has no stored endpoint.
    pub cli_endpoint: String,
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
        self.hosts.iter().find(|h| h.host_id == host_id)
    }

    /// Get the effective endpoint for a host (stored or CLI fallback).
    pub fn effective_endpoint<'a>(&'a self, host: &'a HostConfigEntry) -> &'a str {
        if host.endpoint.is_empty() {
            &self.cli_endpoint
        } else {
            &host.endpoint
        }
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

    /// Get the S3 client for a host's endpoint.
    pub fn get_s3_client(&self, host: &HostConfigEntry) -> Option<&Arc<dyn S3Client>> {
        let endpoint = self.effective_endpoint(host);
        self.s3_clients.get(endpoint)
    }
}
