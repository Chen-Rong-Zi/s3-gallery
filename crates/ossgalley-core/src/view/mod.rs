//! Local view module - pure database queries, no S3 operations.
//!
//! This module provides a read-only view of the local database with no
//! ability to perform S3 operations. The absence of an S3Client field
//! in LocalView is a compiler-enforced guarantee.

pub mod ls;
pub mod tree;
pub mod stat;
pub mod search;
pub mod timeline;
pub mod tags;
pub mod duplicates;
pub mod export;
pub mod remote;

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
        prefix: &str,
        sort_by: crate::types::SortField,
        sort_order: crate::types::SortOrder,
    ) -> crate::error::Result<Vec<ls::LsEntry>> {
        ls::list_directory(&self.db, prefix, sort_by, sort_order).await
    }

    /// Get file statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if any database query fails.
    pub async fn get_stats(&self) -> crate::error::Result<stat::FileStats> {
        stat::get_stats(&self.db).await
    }

    /// Search files by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn search_by_name(&self, query: &str) -> crate::error::Result<search::SearchResult> {
        search::search_by_name(&self.db, query).await
    }

    /// Search files by tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn search_by_tag(&self, tag_name: &str) -> crate::error::Result<search::SearchResult> {
        search::search_by_tag(&self.db, tag_name).await
    }

    /// Get timeline grouped by date.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_timeline(&self) -> crate::error::Result<Vec<timeline::TimelineEntry>> {
        timeline::get_timeline(&self.db).await
    }

    /// List all tags.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list_tags(&self) -> crate::error::Result<Vec<crate::db::models::TagEntry>> {
        tags::list_tags(&self.db).await
    }

    /// Get files for a tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_files_by_tag(
        &self,
        tag_name: &str,
    ) -> crate::error::Result<Vec<crate::db::models::FileEntry>> {
        tags::get_files_by_tag(&self.db, tag_name).await
    }

    /// Find duplicate files.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_duplicates(&self) -> crate::error::Result<Vec<duplicates::DuplicateGroup>> {
        duplicates::find_duplicates(&self.db).await
    }

    /// Export file list.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails or serialization fails.
    pub async fn export_files(
        &self,
        format: export::ExportFormat,
    ) -> crate::error::Result<String> {
        export::export_files(&self.db, format).await
    }

    /// Build a directory tree.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn build_tree(&self, root_prefix: &str) -> crate::error::Result<tree::TreeNode> {
        tree::build_tree(&self.db, root_prefix).await
    }
}
