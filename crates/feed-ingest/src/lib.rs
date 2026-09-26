//! The daily feed step (spec §6, `vitrina ingest`).
//!
//! The feed's format belongs to the network and changes without notice, so
//! every assumption about it lives in this crate: column names in
//! [`columns`], value parsing in [`parse`]. The rest of the system sees only
//! [`catalog_store::FeedDay`].
//!
//! The only network access is [`fetch`], and it refuses iherb.com (И1).

pub mod columns;
pub mod guard;
pub mod parse;

use std::io::Read;
use std::time::Duration;

use catalog_core::{Date, RatingBranch, Reference, UtcTimestamp};
use catalog_store::{FeedDay, FeedItem, FeedProduct, PriceSnapshot, RatingSnapshot};

pub use guard::{GuardError, check_feed_url};
pub use parse::{FeedRow, ParsedFeed, Rejection, parse_feed};

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error(transparent)]
    Guard(#[from] GuardError),
    #[error("feed request failed: {0}")]
    Http(String),
    #[error("feed returned HTTP {0}")]
    Status(u16),
    #[error("too many redirects")]
    Redirects,
    #[error("feed is larger than {0} bytes")]
    TooLarge(u64),
    #[error("feed is not valid CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("feed is missing required columns: {0}")]
    MissingColumns(String),
    #[error("feed has no rows")]
    Empty,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

const MAX_FEED_BYTES: u64 = 1 << 30;
const MAX_REDIRECTS: usize = 5;

/// Downloads the feed. Redirects are followed by hand so that every hop
/// passes the same guard as the first URL.
pub fn fetch(url: &str) -> Result<Vec<u8>, FeedError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut current = check_feed_url(url)?;
    for _ in 0..=MAX_REDIRECTS {
        let mut resp = agent.get(current.as_str()).call().map_err(|e| FeedError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or(FeedError::Status(status))?;
            let next = current.join(location).map_err(|e| FeedError::Http(e.to_string()))?;
            current = check_feed_url(next.as_str())?;
            continue;
        }
        if status != 200 {
            return Err(FeedError::Status(status));
        }
        let mut body = Vec::new();
        resp.body_mut().as_reader().take(MAX_FEED_BYTES + 1).read_to_end(&mut body)?;
        if body.len() as u64 > MAX_FEED_BYTES {
            return Err(FeedError::TooLarge(MAX_FEED_BYTES));
        }
        return Ok(body);
    }
    Err(FeedError::Redirects)
}

/// A short, secret-free description of where the feed came from.
pub fn describe_source(url_or_path: &str) -> String {
    match url::Url::parse(url_or_path) {
        Ok(u) if u.host_str().is_some() => u.host_str().unwrap_or_default().to_owned(),
        _ => std::path::Path::new(url_or_path)
            .file_name()
            .map_or_else(|| "feed".to_owned(), |n| n.to_string_lossy().into_owned()),
    }
}

/// Keeps the rows that belong to a reference category and turns them into
/// one day of observations. Ratings are dropped in branch B (spec §5.4).
pub fn build_day(
    feed: &ParsedFeed,
    reference: &Reference,
    branch: RatingBranch,
    date: Date,
    fetched_at: UtcTimestamp,
    source: String,
) -> FeedDay {
    let mut items = Vec::new();
    for row in &feed.rows {
        let Some(category) = columns::match_category(&row.category_path, reference) else {
            continue;
        };
        let rating = (branch.uses_ratings() && (row.rating_avg.is_some() || row.rating_count.is_some())).then(|| RatingSnapshot {
            product: row.iherb_id.clone(),
            date,
            avg: row.rating_avg,
            count: row.rating_count,
        });
        items.push(FeedItem {
            product: FeedProduct {
                iherb_id: row.iherb_id.clone(),
                slug: row.slug.clone(),
                brand: row.brand.clone(),
                title: row.title.clone(),
                category: category.id.clone(),
                tracking_url: row.tracking_url.clone(),
            },
            price: PriceSnapshot {
                product: row.iherb_id.clone(),
                date,
                price: row.price,
                in_stock: row.in_stock,
                hidden_until_cart: row.hidden_until_cart,
                feed_row_hash: row.hash.clone(),
            },
            rating,
        });
    }
    FeedDay {
        date,
        fetched_at,
        source,
        feed_sha256: feed.sha256.clone(),
        rows_total: feed.rows_total,
        rows_rejected: u32::try_from(feed.rejected.len()).unwrap_or(u32::MAX),
        categories: reference.categories.keys().cloned().collect(),
        items,
    }
}
