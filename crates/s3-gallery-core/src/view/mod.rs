//! Local view module - pure database queries, no S3 operations.
//!
//! This module provides a read-only view of the local database with no
//! ability to perform S3 operations. The absence of an S3Client field
//! in LocalView is a compiler-enforced guarantee.

pub mod duplicates;
pub mod export;
pub mod ls;
pub mod remote;
pub mod search;
pub mod stat;
pub mod tags;
pub mod timeline;
pub mod timeline_gallery;
pub mod traffic;
pub mod tree;

pub use remote::RemoteView;

use sqlx::SqlitePool;

/// Local view - pure database queries, no S3 operations.
///
/// The absence of an S3Client field is a compiler-enforced guarantee
/// that no S3 I/O can be performed through this struct.
pub struct LocalView {
    db: SqlitePool,
}

impl LocalView {
    /// Create a new LocalView from a database connection pool.
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Get a reference to the database connection pool.
    pub fn db(&self) -> &SqlitePool {
        &self.db
    }

    /// List files in a directory prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list_directory(
        &self,
        host_id: &str,
        prefix: &str,
        sort_by: crate::types::SortField,
        sort_order: crate::types::SortOrder,
    ) -> crate::error::Result<Vec<ls::LsEntry>> {
        ls::list_directory(&self.db, host_id, prefix, sort_by, sort_order).await
    }

    /// Get file statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if any database query fails.
    pub async fn get_stats(&self, host_id: &str) -> crate::error::Result<stat::FileStats> {
        stat::get_stats(&self.db, Some(host_id)).await
    }

    /// Search files by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn search_by_name(
        &self,
        host_id: &str,
        query: &str,
    ) -> crate::error::Result<search::SearchResult> {
        search::search_by_name(&self.db, host_id, query).await
    }

    /// Search files by tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn search_by_tag(
        &self,
        host_id: &str,
        tag_name: &str,
    ) -> crate::error::Result<search::SearchResult> {
        search::search_by_tag(&self.db, host_id, tag_name).await
    }

    /// Get timeline grouped by date.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_timeline(
        &self,
        host_id: &str,
    ) -> crate::error::Result<Vec<timeline::TimelineEntry>> {
        timeline::get_timeline(&self.db, Some(host_id)).await
    }

    /// List all tags.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list_tags(
        &self,
        host_id: &str,
    ) -> crate::error::Result<Vec<crate::db::models::TagEntry>> {
        tags::list_tags(&self.db, Some(host_id)).await
    }

    /// Get files for a tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_files_by_tag(
        &self,
        host_id: &str,
        tag_name: &str,
    ) -> crate::error::Result<Vec<crate::db::models::FileEntry>> {
        tags::get_files_by_tag(&self.db, Some(host_id), tag_name).await
    }

    /// Find duplicate files.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_duplicates(
        &self,
        host_id: &str,
    ) -> crate::error::Result<Vec<duplicates::DuplicateGroup>> {
        duplicates::find_duplicates(&self.db, Some(host_id)).await
    }

    /// Export file list.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails or serialization fails.
    pub async fn export_files(
        &self,
        host_id: &str,
        format: export::ExportFormat,
    ) -> crate::error::Result<String> {
        export::export_files(&self.db, host_id, format).await
    }

    /// Build a directory tree.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn build_tree(
        &self,
        host_id: &str,
        root_prefix: &str,
    ) -> crate::error::Result<tree::TreeNode> {
        tree::build_tree(&self.db, host_id, root_prefix).await
    }
}
