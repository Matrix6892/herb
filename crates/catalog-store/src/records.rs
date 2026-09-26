use catalog_core::schema::{CategoriesFile, FormsFile, SubstancesFile};
use catalog_core::{CategoryId, Date, IherbId, Label, Money, Slug, UtcTimestamp};

/// A day's observation of one product's price. `price: None` means the feed
/// had no usable price; it is stored as NULL, not zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceSnapshot {
    pub product: IherbId,
    pub date: Date,
    pub price: Option<Money>,
    pub in_stock: bool,
    pub hidden_until_cart: bool,
    pub feed_row_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatingSnapshot {
    pub product: IherbId,
    pub date: Date,
    pub avg: Option<f32>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRun {
    pub date: Date,
    pub fetched_at: UtcTimestamp,
    /// Host or file name of the feed; never the full URL, which may carry a
    /// key.
    pub source: String,
    pub feed_sha256: String,
    pub rows_total: u32,
    pub rows_rejected: u32,
    pub rows_in_scope: u32,
    pub new_products: u32,
}

/// Product facts from one feed row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedProduct {
    pub iherb_id: IherbId,
    pub slug: Slug,
    pub brand: String,
    pub title: String,
    pub category: CategoryId,
    pub tracking_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedItem {
    pub product: FeedProduct,
    pub price: PriceSnapshot,
    /// Only in rating branch A.
    pub rating: Option<RatingSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedDay {
    pub date: Date,
    pub fetched_at: UtcTimestamp,
    pub source: String,
    pub feed_sha256: String,
    pub rows_total: u32,
    pub rows_rejected: u32,
    /// Categories the feed was matched against; their products missing from
    /// this feed become `delisted`.
    pub categories: Vec<CategoryId>,
    pub items: Vec<FeedItem>,
}

/// The reference files as written by hand; stored verbatim so that a snapshot
/// rebuilds exactly the same `Reference`.
#[derive(Debug, Clone)]
pub struct ReferenceFiles {
    pub substances: SubstancesFile,
    pub forms: FormsFile,
    pub categories: CategoriesFile,
}

/// All label versions of one product from `data/labels/<iherb_id>.toml`.
#[derive(Debug, Clone)]
pub struct LabelSet {
    pub product: IherbId,
    pub label_ref: String,
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub substances: usize,
    pub forms: usize,
    pub categories: usize,
    pub products_with_labels: usize,
    pub label_versions: usize,
    /// Labels for products the feed has not shown yet.
    pub labels_without_product: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickEvent {
    pub product: IherbId,
    pub preset: String,
    pub page: String,
    pub ts: UtcTimestamp,
    pub session_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    pub product: Option<IherbId>,
    pub field: String,
    pub text: String,
    pub created_at: UtcTimestamp,
    pub page_url: String,
}
