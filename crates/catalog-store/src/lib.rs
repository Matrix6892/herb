//! SQLite storage (spec §4.3): one file in WAL mode, forward-only numbered
//! migrations, and repositories behind traits so that a later move to
//! Postgres changes this crate only. SQL does not appear anywhere else.

pub mod files;
mod migrate;
mod records;
mod sqlite;

pub use migrate::{LATEST_VERSION, MIGRATIONS};
pub use records::{
    ClickEvent, ErrorReport, FeedDay, FeedItem, FeedProduct, ImportReport, IngestRun, LabelSet, PriceSnapshot, RatingSnapshot,
    ReferenceFiles,
};
pub use sqlite::SqliteStore;

use catalog_core::{CategoryId, Date, IherbId, Label, Product, Reference};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than this program ({supported})")]
    TooNew { found: u32, supported: u32 },
    #[error("snapshot schema version {found} does not match this program ({supported}); rebuild with the matching release")]
    SnapshotVersion { found: u32, supported: u32 },
    #[error("corrupt row in `{table}`: {detail}")]
    Corrupt { table: &'static str, detail: String },
    #[error("stored reference is invalid: {0}")]
    Reference(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// What the site generator and checks read.
pub trait CatalogRead {
    fn reference(&self) -> Result<Reference, StoreError>;
    fn latest_ingest(&self) -> Result<Option<IngestRun>, StoreError>;
    fn products_in_category(&self, category: &CategoryId) -> Result<Vec<Product>, StoreError>;
    /// All label versions of a product, oldest first.
    fn labels(&self, product: &IherbId) -> Result<Vec<Label>, StoreError>;
    fn price_on(&self, product: &IherbId, date: Date) -> Result<Option<PriceSnapshot>, StoreError>;
    fn rating_on(&self, product: &IherbId, date: Date) -> Result<Option<RatingSnapshot>, StoreError>;
    fn pending_labels(&self) -> Result<Vec<(IherbId, Date)>, StoreError>;
}

/// What the daily ingest and the label import write.
pub trait CatalogWrite {
    /// Replaces the reference and all labels with the repository's files, in
    /// one transaction.
    fn import_reference_and_labels(&mut self, files: &ReferenceFiles, labels: &[LabelSet]) -> Result<ImportReport, StoreError>;
    /// Writes one day of feed observations, in one transaction, and records
    /// the run.
    fn record_feed_day(&mut self, day: &FeedDay) -> Result<IngestRun, StoreError>;
}

/// What edge-api writes.
pub trait EventSink {
    fn insert_click(&self, event: &ClickEvent) -> Result<(), StoreError>;
    fn insert_report(&self, report: &ErrorReport) -> Result<(), StoreError>;
}
