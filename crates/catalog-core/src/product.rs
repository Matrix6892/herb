//! Products as the catalog knows them.

use crate::date::Date;
use crate::ids::{CategoryId, IherbId, Slug};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProductStatus {
    /// In today's feed with a label on file.
    Active,
    /// In the feed, waiting for a hand-entered label.
    PendingLabel,
    /// Not in the latest feed.
    Delisted,
}

impl ProductStatus {
    pub fn key(self) -> &'static str {
        match self {
            ProductStatus::Active => "active",
            ProductStatus::PendingLabel => "pending_label",
            ProductStatus::Delisted => "delisted",
        }
    }

    pub fn from_key(key: &str) -> Option<ProductStatus> {
        [ProductStatus::Active, ProductStatus::PendingLabel, ProductStatus::Delisted]
            .into_iter()
            .find(|s| s.key() == key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub iherb_id: IherbId,
    /// Copied from the feed's product path unchanged (spec §7.1).
    pub slug: Slug,
    pub brand: String,
    pub title: String,
    pub category: CategoryId,
    pub status: ProductStatus,
    /// The affiliate network's tracking link, the only link to the store
    /// that pages may carry (И3).
    pub tracking_url: Option<String>,
    /// Path of the label file, `data/labels/<iherb_id>.toml`.
    pub label_ref: Option<String>,
    pub first_seen: Date,
    pub last_seen: Date,
}
