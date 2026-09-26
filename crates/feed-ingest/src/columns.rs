//! Column names of the feed and category matching.
//!
//! Until a real feed sample arrives (question О3) the columns follow the
//! general shape of Impact product catalogs, with aliases for common
//! spellings. Headers match case-insensitively, ignoring spaces, `_` and `-`.
//! See docs/adr/0013-feed-format-assumed.md.

use catalog_core::{Category, Reference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Col {
    /// iHerb product page URL; slug and id are read from its path.
    ProductUrl,
    /// Network tracking link for the buy button (И3).
    TrackingUrl,
    Id,
    Name,
    Brand,
    Price,
    Currency,
    Availability,
    Category,
    SubCategory,
    HiddenUntilCart,
    Rating,
    ReviewCount,
}

impl Col {
    pub const ALL: [Col; 13] = [
        Col::ProductUrl,
        Col::TrackingUrl,
        Col::Id,
        Col::Name,
        Col::Brand,
        Col::Price,
        Col::Currency,
        Col::Availability,
        Col::Category,
        Col::SubCategory,
        Col::HiddenUntilCart,
        Col::Rating,
        Col::ReviewCount,
    ];

    /// Normalised header names that map to this column.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Col::ProductUrl => &["producturl", "originalurl", "landingpageurl", "productpageurl"],
            Col::TrackingUrl => &["url", "trackingurl", "trackinglink", "producttrackingurl", "clickurl"],
            Col::Id => &["catalogitemid", "productid", "iherbid"],
            Col::Name => &["name", "productname", "title"],
            Col::Brand => &["manufacturer", "brand", "brandname"],
            Col::Price => &["currentprice", "saleprice", "price"],
            Col::Currency => &["currency", "currencycode"],
            Col::Availability => &["stockavailability", "availability", "instock"],
            Col::Category => &["category", "productcategory"],
            Col::SubCategory => &["subcategory", "productsubcategory"],
            Col::HiddenUntilCart => &["pricehiddenuntilcart", "hiddenuntilcart", "priceincart"],
            Col::Rating => &["rating", "averagerating", "reviewrating"],
            Col::ReviewCount => &["reviewcount", "numberofreviews", "numreviews", "reviews"],
        }
    }

    pub fn required(self) -> bool {
        matches!(self, Col::Name | Col::Price | Col::Availability | Col::Category)
    }
}

pub fn normalise_header(h: &str) -> String {
    h.trim_start_matches('\u{feff}')
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The first reference category whose `feed_match` equals one segment of the
/// product's category path (`Supplements > Minerals > Magnesium`). Whole
/// segments only, so `Calcium Magnesium Zinc` does not land in magnesium.
pub fn match_category<'r>(path: &str, reference: &'r Reference) -> Option<&'r Category> {
    let segments: Vec<String> = path
        .split(['>', '/', '|', ','])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    reference
        .categories
        .values()
        .find(|c| c.feed_match.iter().any(|m| segments.iter().any(|s| *s == m.trim().to_lowercase())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_headers() {
        assert_eq!(normalise_header("\u{feff}Current Price"), "currentprice");
        assert_eq!(normalise_header("Stock_Availability"), "stockavailability");
    }

    #[test]
    fn aliases_do_not_overlap() {
        let mut seen = std::collections::BTreeSet::new();
        for c in Col::ALL {
            for a in c.aliases() {
                assert!(seen.insert(*a), "alias `{a}` used twice");
            }
        }
    }
}
