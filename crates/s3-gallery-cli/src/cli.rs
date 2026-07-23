use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "s3-gallery")]
#[command(about = "OSS media file gallery browser", long_about = None)]
pub struct Cli {
    /// S3 endpoint URL
    #[arg(
        short = 'e',
        long,
        default_value = "https://localhost:9000",
        env = "S3_GALLERY_ENDPOINT"
    )]
    pub endpoint: String,

    /// Access key ID
    #[arg(
        short = 'k',
        long,
        default_value = "s3oss",
        env = "S3_GALLERY_ACCESS_KEY"
    )]
    pub access_key: String,

    /// Secret access key
    #[arg(
        short = 's',
        long,
        default_value = "s3oss1234",
        env = "S3_GALLERY_SECRET_KEY"
    )]
    pub secret_key: String,

    /// S3 region
    #[arg(
        short = 'r',
        long,
        default_value = "us-east-1",
        env = "S3_GALLERY_REGION"
    )]
    pub region: String,

    /// OSS bucket name
    #[arg(short = 'b', long)]
    pub bucket: Option<String>,

    /// Path to the local database file (default: s3-gallery.db)
    #[arg(
        long,
        global = true,
        default_value = "s3-gallery.db",
        env = "S3_GALLERY_DB_PATH"
    )]
    pub db_path: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a host directory and create host.config.json
    Init {
        /// OSS bucket name
        #[arg(short = 'b', long)]
        bucket: String,
        /// Directory prefix for the host (e.g. "photos"), defaults to bucket root
        #[arg(long)]
        prefix: Option<String>,
        /// Human-readable display name for this host
        #[arg(short = 'n', long)]
        name: String,
        /// Optional description
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// Scan/update the database
    #[command(subcommand)]
    Scan(ScanCommand),
    /// Query the database
    View {
        /// Host directory name
        host: String,
        #[command(subcommand)]
        view_command: ViewCommands,
    },
    /// Manage database files
    Db {
        /// Host directory name
        host: String,
        #[command(subcommand)]
        db_command: DbCommands,
    },
    /// Start the web server
    #[command(alias = "web")]
    Serve {
        /// File prefix filter (e.g. "photos/")
        #[arg(long)]
        prefix: Option<String>,
        /// Web server port
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Read-only mode
        #[arg(long)]
        readonly: bool,
    },
    /// Traffic analysis commands
    #[command(subcommand)]
    Traffic(TrafficCommands),
}

#[derive(Subcommand)]
pub enum TrafficCommands {
    /// Show traffic summary for a period
    Summary {
        /// Host ID filter
        #[arg(long)]
        host: Option<String>,
        /// Period: day|month
        #[arg(long, default_value = "day")]
        period: String,
        /// Start date (ISO-8601)
        #[arg(long)]
        since: Option<String>,
        /// End date (ISO-8601)
        #[arg(long)]
        until: Option<String>,
    },
    /// Show live traffic
    Live {
        /// Refresh interval in seconds
        #[arg(long, default_value = "2")]
        interval: u64,
    },
}

#[derive(Subcommand)]
pub enum ScanCommand {
    /// Fresh scan: create a new local database from scratch
    Init {
        /// File prefix to scan (e.g. "photos/"), defaults to bucket root
        #[arg(long)]
        prefix: Option<String>,
        /// Incremental scan (resume from last position)
        #[arg(long)]
        incremental: bool,
        /// Schedule mode (skip if locked)
        #[arg(long)]
        schedule: bool,
        /// Force scan (ignore lock)
        #[arg(long)]
        force: bool,
        /// Skip metadata extraction
        #[arg(long)]
        no_metadata: bool,
        /// Generate thumbnails (expensive)
        #[arg(long)]
        with_thumbnails: bool,
        /// Concurrency for S3 requests
        #[arg(long, default_value = "10")]
        concurrency: usize,
        /// Confirm overwrite of existing local database
        #[arg(long)]
        confirm: bool,
    },
    /// Incremental update: scan new/changed files into existing local database
    Update {
        /// File prefix to scan (e.g. "photos/")
        #[arg(long)]
        prefix: Option<String>,
        /// Incremental scan (resume from last position)
        #[arg(long)]
        incremental: bool,
        /// Schedule mode (skip if locked)
        #[arg(long)]
        schedule: bool,
        /// Force scan (ignore lock)
        #[arg(long)]
        force: bool,
        /// Skip metadata extraction
        #[arg(long)]
        no_metadata: bool,
        /// Generate thumbnails (expensive)
        #[arg(long)]
        with_thumbnails: bool,
        /// Concurrency for S3 requests
        #[arg(long, default_value = "10")]
        concurrency: usize,
    },
    /// Sync: pull remote database first, then update locally
    Sync {
        /// File prefix to scan (e.g. "photos/")
        #[arg(long)]
        prefix: Option<String>,
        /// Incremental scan (resume from last position)
        #[arg(long)]
        incremental: bool,
        /// Schedule mode (skip if locked)
        #[arg(long)]
        schedule: bool,
        /// Force scan (ignore lock)
        #[arg(long)]
        force: bool,
        /// Skip metadata extraction
        #[arg(long)]
        no_metadata: bool,
        /// Generate thumbnails (expensive)
        #[arg(long)]
        with_thumbnails: bool,
        /// Concurrency for S3 requests
        #[arg(long, default_value = "10")]
        concurrency: usize,
    },
}

#[derive(Subcommand)]
pub enum ViewCommands {
    /// List directory contents
    Ls {
        /// Path to list
        path: Option<String>,
        /// Sort by field
        #[arg(long, default_value = "name")]
        sort_by: String,
        /// Sort order
        #[arg(long, default_value = "asc")]
        sort_order: String,
    },
    /// Show directory tree
    Tree {
        /// Root path
        path: Option<String>,
    },
    /// File statistics
    Stat {
        /// Path filter
        path: Option<String>,
    },
    /// Show file details
    Files {
        /// File key
        key: String,
    },
    /// Search files
    Search {
        /// Search query
        query: String,
    },
    /// List metadata namespaces
    MetadataLs {
        /// Namespace filter
        namespace: Option<String>,
    },
    /// Query metadata by namespace:key
    MetadataQuery {
        /// Namespace:key query
        query: String,
    },
    /// File type distribution
    Types,
    /// Find duplicate files
    Duplicates,
    /// Show largest files
    Largest {
        /// Number of files
        n: Option<usize>,
    },
    /// Show recent files
    Recent {
        /// Number of files
        n: Option<usize>,
    },
    /// Show oldest files
    Oldest {
        /// Number of files
        n: Option<usize>,
    },
    /// Show timeline
    Timeline,
    /// Tag operations
    Tags {
        #[command(subcommand)]
        tag_command: TagCommands,
    },
    /// Show diff
    Diff {
        #[command(subcommand)]
        diff_command: DiffCommands,
    },
    /// Export file list
    Export {
        /// Export format (csv/json)
        format: String,
    },
    /// Show DB schema info
    Schema,
    /// Show GPS-located files
    Locations,
    /// Show deleted/orphan records
    Orphan,
    /// Health check
    Health,
}

#[derive(Subcommand)]
pub enum TagCommands {
    /// List all tags
    List,
    /// Show files for a tag
    Files {
        /// Tag name
        tag: String,
    },
    /// Tag statistics
    Stats,
}

#[derive(Subcommand)]
pub enum DiffCommands {
    /// Newly added files
    Added,
    /// Recently deleted files
    Removed,
    /// Changed files
    Changed,
}

#[derive(Subcommand)]
pub enum DbCommands {
    /// Download DB from OSS
    Pull,
    /// Upload DB to OSS
    Push,
    /// Show DB status
    Status,
    /// View lock info
    Lock,
    /// Force unlock (dangerous)
    Unlock,
}
