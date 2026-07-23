//! Shared types for the scan pipeline — ScanRequest, ScanResponse, and
//! AggregateReport with traffic/file/status breakdowns.

use std::collections::HashMap;

use crate::s3::config::HostIdentifier;
use crate::types::ObjectKey;

/// Request — same for all pipeline layers.
#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub bucket: crate::types::BucketName,
    pub scope_prefix: ObjectKey,
    pub concurrency: usize,
    pub extract_metadata: bool,
    pub generate_thumbnails: bool,
    pub client_id: String,
}

/// Response — each layer fills its section.
#[derive(Debug, Default)]
pub struct ScanResponse {
    // DiscoverLayer fills:
    pub scan_id: String,
    pub hosts: Vec<HostInfo>,

    // DiffLayer fills:
    pub diff_results: Vec<HostDiffResult>,

    // ProcessLayer fills:
    pub process_results: Vec<HostProcessResult>,

    // AggregateLayer fills:
    pub report: Option<AggregateReport>,
}

/// A discovered host.
#[derive(Debug, Clone)]
pub struct HostInfo {
    pub host_id: String,
    pub host_name: String,
    pub prefix: ObjectKey,
    pub config: Option<HostIdentifier>,
}

/// Result of diffing scan_objects against files table for one host.
#[derive(Debug, Clone)]
pub struct HostDiffResult {
    pub host_id: String,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub unchanged_count: u64,
    pub file_type_counts: HashMap<String, u64>,
}

/// Result of processing (EXIF extraction) for one host.
#[derive(Debug, Clone)]
pub struct HostProcessResult {
    pub host_id: String,
    pub processed_count: u64,
    pub failed_count: u64,
}

/// Traffic breakdown by operation type for a single stage.
#[derive(Debug, Clone, Default)]
pub struct TrafficByOperation {
    pub count: u64,
    pub bytes: u64,
}

/// Size distribution ranges.
#[derive(Debug, Clone, Default)]
pub struct SizeRanges {
    pub tiny: u64,   // 0-1KB
    pub small: u64,  // 1KB-100KB
    pub medium: u64, // 100KB-1MB
    pub large: u64,  // 1MB-10MB
    pub huge: u64,   // 10MB+
}

/// Final aggregate report with all statistics.
#[derive(Debug, Clone)]
pub struct AggregateReport {
    // A: Traffic statistics
    pub traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>>,
    pub total_download_bytes: u64,
    pub total_upload_bytes: u64,
    pub total_requests: u64,
    pub estimated_cost: f64,

    // B: File type statistics
    pub file_type_breakdown: HashMap<String, u64>,
    pub size_ranges: SizeRanges,

    // C: Scan status statistics
    pub total_files: u64,
    pub total_size: u64,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub host_count: u64,
    pub duration_secs: f64,
}

impl Default for AggregateReport {
    fn default() -> Self {
        Self {
            traffic_by_stage: HashMap::new(),
            total_download_bytes: 0,
            total_upload_bytes: 0,
            total_requests: 0,
            estimated_cost: 0.0,
            file_type_breakdown: HashMap::new(),
            size_ranges: SizeRanges::default(),
            total_files: 0,
            total_size: 0,
            new_files: 0,
            changed_files: 0,
            deleted_files: 0,
            host_count: 0,
            duration_secs: 0.0,
        }
    }
}