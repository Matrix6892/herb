//! Feed rows to typed values. A row that cannot be read is rejected with a
//! reason and reported; it never becomes a guessed value (И7).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use catalog_core::{IherbId, Money, Slug};
use sha2::{Digest, Sha256};

use crate::FeedError;
use crate::columns::{Col, normalise_header};
use crate::guard::is_store_host;

#[derive(Debug, Clone, PartialEq)]
pub struct FeedRow {
    /// 1-based data row number, for reports.
    pub row: u64,
    pub iherb_id: IherbId,
    pub slug: Slug,
    pub brand: String,
    pub title: String,
    /// `Category > SubCategory`.
    pub category_path: String,
    pub tracking_url: Option<String>,
    /// `None` when the feed has no price or a zero price.
    pub price: Option<Money>,
    pub in_stock: bool,
    pub hidden_until_cart: bool,
    pub rating_avg: Option<f32>,
    pub rating_count: Option<u32>,
    /// SHA-256 of the raw row, stored with the price snapshot.
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub row: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFeed {
    pub rows: Vec<FeedRow>,
    pub rejected: Vec<Rejection>,
    pub rows_total: u32,
    /// Rows accepted without a usable price.
    pub rows_without_price: u32,
    pub sha256: String,
}

fn gunzip_if_needed(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, FeedError> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::MultiGzDecoder::new(bytes).read_to_end(&mut out)?;
        Ok(std::borrow::Cow::Owned(out))
    } else {
        Ok(std::borrow::Cow::Borrowed(bytes))
    }
}

/// Slug and id from an iHerb product URL `https://www.iherb.com/pr/<slug>/<id>`.
/// The URL is only parsed, never requested.
pub fn product_path(raw: &str) -> Result<(Slug, IherbId), String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| format!("product URL `{raw}` is not a URL"))?;
    if !u.host_str().is_some_and(is_store_host) {
        return Err(format!("product URL `{raw}` is not a store product page"));
    }
    let segments: Vec<&str> = u.path_segments().map(Iterator::collect).unwrap_or_default();
    match segments.as_slice() {
        ["pr", slug, id] | ["pr", slug, id, ""] => {
            let slug = Slug::new(*slug).map_err(|e| e.to_string())?;
            let id = IherbId::new(*id).map_err(|e| e.to_string())?;
            Ok((slug, id))
        }
        _ => Err(format!("product URL `{raw}` does not have the /pr/<slug>/<id> path")),
    }
}

/// The product URL carried in a tracking link's `u` parameter, if any.
fn embedded_product_url(tracking: &str) -> Option<String> {
    let u = url::Url::parse(tracking).ok()?;
    u.query_pairs().find(|(k, _)| k == "u").map(|(_, v)| v.into_owned())
}

fn check_tracking_url(raw: &str) -> Result<String, String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| format!("tracking URL `{raw}` is not a URL"))?;
    if u.scheme() != "https" {
        return Err(format!("tracking URL `{raw}` is not https"));
    }
    // A direct store link is not a network tracking link (И3).
    if u.host_str().is_none_or(is_store_host) {
        return Err(format!("tracking URL `{raw}` points at the store, not the network"));
    }
    Ok(u.to_string())
}

fn parse_price(raw: &str) -> Result<Option<Money>, String> {
    let t = raw.trim().trim_start_matches('$').trim_end_matches("USD").trim();
    if t.is_empty() {
        return Ok(None);
    }
    let m = Money::parse_dollars(t).map_err(|e| format!("price `{raw}`: {e}"))?;
    // A zero price is a placeholder, not a price.
    Ok((m.cents() > 0).then_some(m))
}

fn norm_token(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn parse_availability(raw: &str) -> Result<bool, String> {
    match norm_token(raw).as_str() {
        "instock" | "true" | "yes" | "1" | "available" | "limitedavailability" | "limitedstock" => Ok(true),
        "outofstock" | "false" | "no" | "0" | "soldout" | "discontinued" | "unavailable" | "preorder" | "backorder" => Ok(false),
        _ => Err(format!("availability `{raw}` is not recognised")),
    }
}

fn parse_flag(raw: &str) -> Result<bool, String> {
    match norm_token(raw).as_str() {
        "" | "false" | "no" | "0" => Ok(false),
        "true" | "yes" | "1" => Ok(true),
        _ => Err(format!("flag `{raw}` is not true/false")),
    }
}

pub fn parse_feed(bytes: &[u8]) -> Result<ParsedFeed, FeedError> {
    let sha256 = hex::encode(Sha256::digest(bytes));
    let data = gunzip_if_needed(bytes)?;
    let first_line = data.split(|b| *b == b'\n').next().unwrap_or_default();
    let delimiter = if first_line.contains(&b'\t') { b'\t' } else { b',' };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(false)
        .from_reader(data.as_ref());

    let headers = reader.headers()?.clone();
    let mut index: BTreeMap<Col, usize> = BTreeMap::new();
    for (i, h) in headers.iter().enumerate() {
        let n = normalise_header(h);
        if let Some(col) = Col::ALL.into_iter().find(|c| c.aliases().contains(&n.as_str())) {
            index.entry(col).or_insert(i);
        }
    }
    let mut missing: Vec<String> = Col::ALL
        .into_iter()
        .filter(|c| c.required() && !index.contains_key(c))
        .map(|c| format!("{c:?}"))
        .collect();
    if !index.contains_key(&Col::ProductUrl) && !index.contains_key(&Col::TrackingUrl) {
        missing.push("ProductUrl or TrackingUrl".into());
    }
    if !missing.is_empty() {
        return Err(FeedError::MissingColumns(missing.join(", ")));
    }

    let mut rows = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rows_total: u32 = 0;
    let mut rows_without_price: u32 = 0;
    for (i, record) in reader.records().enumerate() {
        let row = i as u64 + 1;
        rows_total = rows_total.saturating_add(1);
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                rejected.push(Rejection {
                    row,
                    reason: e.to_string(),
                });
                continue;
            }
        };
        let get = |c: Col| index.get(&c).and_then(|i| record.get(*i)).map(str::trim).unwrap_or_default();
        let parsed = (|| -> Result<FeedRow, String> {
            let tracking_url = match get(Col::TrackingUrl) {
                "" => None,
                t => Some(check_tracking_url(t)?),
            };
            let product_url = match get(Col::ProductUrl) {
                "" => tracking_url.as_deref().and_then(embedded_product_url).ok_or("no product URL")?,
                p => p.to_owned(),
            };
            let (slug, iherb_id) = product_path(&product_url)?;
            if let Ok(explicit) = IherbId::new(get(Col::Id))
                && explicit != iherb_id
            {
                return Err(format!("id column {explicit} disagrees with product URL id {iherb_id}"));
            }
            let currency = get(Col::Currency);
            if !currency.is_empty() && !currency.eq_ignore_ascii_case("USD") {
                return Err(format!("currency `{currency}` is not USD"));
            }
            let title = get(Col::Name).to_owned();
            if title.is_empty() {
                return Err("empty name".into());
            }
            let category_path = [get(Col::Category), get(Col::SubCategory)]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" > ");
            let rating_avg = get(Col::Rating)
                .parse::<f32>()
                .ok()
                .filter(|r| r.is_finite() && (0.0..=5.0).contains(r));
            let rating_count = get(Col::ReviewCount).parse::<u32>().ok();
            let mut hasher = Sha256::new();
            for field in &record {
                hasher.update(field.as_bytes());
                hasher.update([0x1f]);
            }
            Ok(FeedRow {
                row,
                iherb_id,
                slug,
                brand: get(Col::Brand).to_owned(),
                title,
                category_path,
                tracking_url,
                price: parse_price(get(Col::Price))?,
                in_stock: parse_availability(get(Col::Availability))?,
                hidden_until_cart: parse_flag(get(Col::HiddenUntilCart))?,
                rating_avg,
                rating_count,
                hash: hex::encode(hasher.finalize()),
            })
        })();
        match parsed {
            Ok(r) if !seen.insert(r.iherb_id.clone()) => rejected.push(Rejection {
                row,
                reason: format!("duplicate id {}", r.iherb_id),
            }),
            Ok(r) => {
                if r.price.is_none() {
                    rows_without_price += 1;
                }
                rows.push(r);
            }
            Err(reason) => rejected.push(Rejection { row, reason }),
        }
    }
    if rows_total == 0 {
        return Err(FeedError::Empty);
    }
    Ok(ParsedFeed {
        rows,
        rejected,
        rows_total,
        rows_without_price,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_product_path() {
        let (slug, id) = product_path("https://www.iherb.com/pr/fixture-labs-magnesium/900001?rcode=x").unwrap();
        assert_eq!((slug.as_str(), id.as_str()), ("fixture-labs-magnesium", "900001"));
        assert!(product_path("https://www.iherb.com/c/magnesium").is_err());
        assert!(product_path("https://evil.example/pr/a/1").is_err());
        assert!(product_path("https://www.iherb.com/pr/..%2Fetc/1").is_err());
    }

    #[test]
    fn prices_and_flags() {
        assert_eq!(parse_price("$12.34"), Ok(Some(Money::from_cents(1234))));
        assert_eq!(parse_price("12.34 USD"), Ok(Some(Money::from_cents(1234))));
        assert_eq!(parse_price(""), Ok(None));
        assert_eq!(parse_price("0.00"), Ok(None));
        assert!(parse_price("12,34").is_err());
        assert_eq!(parse_availability("InStock"), Ok(true));
        assert_eq!(parse_availability("Out of stock"), Ok(false));
        assert!(parse_availability("maybe").is_err());
        assert!(check_tracking_url("https://www.iherb.com/pr/a/1?rcode=X").is_err());
        assert!(check_tracking_url("javascript:alert(1)").is_err());
    }
}
