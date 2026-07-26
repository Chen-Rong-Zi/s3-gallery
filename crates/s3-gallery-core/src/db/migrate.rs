use sea_orm::DatabaseConnection;
use sea_orm_migration::prelude::*;

use crate::error::S3GalleryError;

/// Run the full migration (create all tables, indexes, backfill data)
/// using the SeaORM `InitialMigration`.
///
/// This is the replacement for the old `db::schema::run_migrations`.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the migration fails.
pub async fn run_full_migration(db: &DatabaseConnection) -> crate::error::Result<()> {
    let manager = SchemaManager::new(db);
    InitialMigration
        .up(&manager)
        .await
        .map_err(|e| S3GalleryError::MigrationError(e.to_string()))?;
    Ok(())
}

#[derive(DeriveMigrationName)]
pub struct InitialMigration;

#[async_trait::async_trait]
impl MigrationTrait for InitialMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // host_config
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("host_config"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("host_id"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("host_name")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("host_type"))
                            .string()
                            .not_null()
                            .default("unknown"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("description"))
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Alias::new("created_at")).string().not_null())
                    .to_owned(),
            )
            .await?;

        // files
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("files"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("host_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("key")).string().not_null())
                    .col(ColumnDef::new(Alias::new("etag")).string().not_null())
                    .col(ColumnDef::new(Alias::new("size")).big_integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("last_modified"))
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("content_type")).string())
                    .col(ColumnDef::new(Alias::new("file_type")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("metadata_state"))
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_deleted"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Alias::new("effective_date"))
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .primary_key(
                        Index::create()
                            .col(Alias::new("host_id"))
                            .col(Alias::new("key")),
                    )
                    .to_owned(),
            )
            .await?;

        // metadata
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("metadata"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("file_key")).string().not_null())
                    .col(ColumnDef::new(Alias::new("namespace")).string().not_null())
                    .col(ColumnDef::new(Alias::new("namespace_custom")).string())
                    .col(ColumnDef::new(Alias::new("key")).string().not_null())
                    .col(ColumnDef::new(Alias::new("value")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("extracted_at"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("partial"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .primary_key(
                        Index::create()
                            .col(Alias::new("file_key"))
                            .col(Alias::new("namespace"))
                            .col(Alias::new("key")),
                    )
                    .to_owned(),
            )
            .await?;

        // classification_rules
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("classification_rules"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("extension"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("file_type")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("priority"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("description")).string())
                    .to_owned(),
            )
            .await?;

        // extractor_rules
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("extractor_rules"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("extension")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("extractor_name"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("priority"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .primary_key(
                        Index::create()
                            .col(Alias::new("extension"))
                            .col(Alias::new("extractor_name")),
                    )
                    .to_owned(),
            )
            .await?;

        // thumbnails
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("thumbnails"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("file_key"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("data")).blob().not_null())
                    .col(
                        ColumnDef::new(Alias::new("format"))
                            .string()
                            .not_null()
                            .default("jpeg"),
                    )
                    .col(ColumnDef::new(Alias::new("width")).integer())
                    .col(ColumnDef::new(Alias::new("height")).integer())
                    .col(ColumnDef::new(Alias::new("cached_at")).string().not_null())
                    .to_owned(),
            )
            .await?;

        // tags
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("tags"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("tag_id"))
                            .integer()
                            .auto_increment()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("tag_name"))
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("tag_type"))
                            .string()
                            .not_null()
                            .default("auto"),
                    )
                    .to_owned(),
            )
            .await?;

        // file_tags
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("file_tags"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("file_key")).string().not_null())
                    .col(ColumnDef::new(Alias::new("tag_id")).integer().not_null())
                    .primary_key(
                        Index::create()
                            .col(Alias::new("file_key"))
                            .col(Alias::new("tag_id")),
                    )
                    .to_owned(),
            )
            .await?;

        // scan_metadata
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("scan_metadata"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("host_id"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("last_scanned_key")).string())
                    .col(ColumnDef::new(Alias::new("last_scanned_at")).string())
                    .col(ColumnDef::new(Alias::new("total_files")).big_integer())
                    .col(ColumnDef::new(Alias::new("total_size")).big_integer())
                    .col(
                        ColumnDef::new(Alias::new("db_schema_version"))
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        // dir_sizes
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("dir_sizes"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("host_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("dir_path")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("total_size"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("total_files"))
                            .big_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(Alias::new("host_id"))
                            .col(Alias::new("dir_path")),
                    )
                    .to_owned(),
            )
            .await?;

        // scan_objects
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("scan_objects"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("scan_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("key")).string().not_null())
                    .col(ColumnDef::new(Alias::new("host_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("etag")).string().not_null())
                    .col(ColumnDef::new(Alias::new("size")).big_integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("last_modified"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_deleted"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .primary_key(
                        Index::create()
                            .col(Alias::new("scan_id"))
                            .col(Alias::new("key")),
                    )
                    .to_owned(),
            )
            .await?;

        // traffic_log
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("traffic_log"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .auto_increment()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("host_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("operation")).string().not_null())
                    .col(ColumnDef::new(Alias::new("business")).string().not_null())
                    .col(ColumnDef::new(Alias::new("direction")).string().not_null())
                    .col(ColumnDef::new(Alias::new("bytes")).big_integer().not_null())
                    .col(ColumnDef::new(Alias::new("count")).big_integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("recorded_at"))
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // traffic_file_log
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("traffic_file_log"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .auto_increment()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("host_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("file_key")).string().not_null())
                    .col(ColumnDef::new(Alias::new("business")).string().not_null())
                    .col(ColumnDef::new(Alias::new("bytes")).big_integer().not_null())
                    .col(ColumnDef::new(Alias::new("count")).big_integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("recorded_at"))
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // traffic_stats
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("traffic_stats"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("host_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("period")).string().not_null())
                    .col(ColumnDef::new(Alias::new("operation")).string().not_null())
                    .col(ColumnDef::new(Alias::new("business")).string().not_null())
                    .col(ColumnDef::new(Alias::new("direction")).string().not_null())
                    .col(
                        ColumnDef::new(Alias::new("total_bytes"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("total_count"))
                            .big_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(Alias::new("host_id"))
                            .col(Alias::new("period"))
                            .col(Alias::new("operation"))
                            .col(Alias::new("business"))
                            .col(Alias::new("direction")),
                    )
                    .to_owned(),
            )
            .await?;

        // ── Indexes ──
        let index_table_names = [
            "files",
            "files",
            "files",
            "metadata",
            "metadata",
            "metadata",
            "tags",
            "thumbnails",
            "traffic_log",
            "traffic_log",
            "traffic_file_log",
            "traffic_file_log",
            "scan_objects",
            "scan_objects",
        ];
        let index_names = [
            "idx_files_file_type",
            "idx_files_host_id",
            "idx_files_last_modified",
            "idx_metadata_namespace",
            "idx_metadata_file_key",
            "idx_metadata_key_value",
            "idx_tags_tag_type",
            "idx_thumbnails_cached_at",
            "idx_traffic_log_host_time",
            "idx_traffic_log_business",
            "idx_traffic_file_host_key",
            "idx_traffic_file_time",
            "idx_scan_objects_scan_id",
            "idx_scan_objects_host_id",
        ];
        let index_cols: [&[&str]; 14] = [
            &["file_type"],
            &["host_id"],
            &["last_modified"],
            &["namespace"],
            &["file_key"],
            &["key", "value"],
            &["tag_type"],
            &["cached_at"],
            &["host_id", "recorded_at"],
            &["business"],
            &["host_id", "file_key"],
            &["recorded_at"],
            &["scan_id"],
            &["host_id"],
        ];

        for (i, name) in index_names.iter().enumerate() {
            let table_name = index_table_names.get(i).ok_or_else(|| {
                DbErr::Custom(format!("Missing table name for index at position {i}"))
            })?;
            let cols = index_cols.get(i).ok_or_else(|| {
                DbErr::Custom(format!("Missing columns for index at position {i}"))
            })?;
            let mut idx = Index::create()
                .name(*name)
                .table(Alias::new(*table_name))
                .to_owned();
            for col in *cols {
                idx.col(Alias::new(*col));
            }
            manager.create_index(idx).await?;
        }

        // ── namespace_custom data migration ──
        let db = manager.get_connection();
        db.execute_unprepared(
            "UPDATE metadata SET namespace_custom = namespace, namespace = 'custom' \
             WHERE namespace NOT IN ('exif', 'video', 'audio', 'general')",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let tables = [
            "traffic_stats",
            "traffic_file_log",
            "traffic_log",
            "scan_objects",
            "dir_sizes",
            "scan_metadata",
            "file_tags",
            "tags",
            "thumbnails",
            "extractor_rules",
            "classification_rules",
            "metadata",
            "files",
            "host_config",
        ];
        for table in &tables {
            manager
                .drop_table(Table::drop().table(Alias::new(*table)).to_owned())
                .await
                .map_err(|e| DbErr::Custom(format!("Failed to drop table {table}: {e}")))?;
        }
        Ok(())
    }
}
