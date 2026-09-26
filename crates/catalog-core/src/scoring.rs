//! Presets and the `balance` score (spec §5.4, §5.5).
//!
//! Commission rates do not exist anywhere in this crate, by design (И2).

use core::cmp::Ordering;
use std::collections::BTreeMap;

use crate::ids::IherbId;
use crate::pricing::UnitPrice;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Preset {
    CheapestPerUnit,
    FairRating,
    Balance,
    CheapestVerified,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::CheapestPerUnit,
        Preset::FairRating,
        Preset::Balance,
        Preset::CheapestVerified,
    ];

    /// Stable key used in URLs, events and translation keys.
    pub fn key(self) -> &'static str {
        match self {
            Preset::CheapestPerUnit => "cheapest_per_unit",
            Preset::FairRating => "fair_rating",
            Preset::Balance => "balance",
            Preset::CheapestVerified => "cheapest_verified",
        }
    }

    pub fn from_key(key: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// Whether ratings may be shown (spec §5.4). Chosen by the owner; question О2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingBranch {
    /// Ratings allowed and available.
    A,
    /// No permission to show ratings.
    B,
}

impl RatingBranch {
    pub fn presets(self) -> &'static [Preset] {
        match self {
            RatingBranch::A => &[Preset::CheapestPerUnit, Preset::FairRating, Preset::Balance],
            RatingBranch::B => &[Preset::CheapestPerUnit, Preset::CheapestVerified],
        }
    }

    pub fn default_preset(self) -> Preset {
        match self {
            RatingBranch::A => Preset::Balance,
            RatingBranch::B => Preset::CheapestPerUnit,
        }
    }

    pub fn uses_ratings(self) -> bool {
        self == RatingBranch::A
    }

    pub fn key(self) -> &'static str {
        match self {
            RatingBranch::A => "A",
            RatingBranch::B => "B",
        }
    }
}

/// A product that has a unit price and so takes part in presets.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: IherbId,
    pub unit_price: UnitPrice,
    /// `R_b`; `None` without a rating or in branch B.
    pub adjusted_rating: Option<f64>,
    pub third_party_tested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceScore {
    pub q_price: f64,
    pub q_rating: f64,
    /// `min(q_price, q_rating)`.
    pub s: f64,
}

/// Which side of a balance score is the weaker one, and so sets `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Price,
    Rating,
    /// Both sides score the same.
    Both,
}

impl Side {
    pub fn key(self) -> &'static str {
        match self {
            Side::Price => "price",
            Side::Rating => "rating",
            Side::Both => "both",
        }
    }
}

impl BalanceScore {
    pub fn weaker(&self) -> Side {
        match grid(self.q_price).cmp(&grid(self.q_rating)) {
            Ordering::Less => Side::Price,
            Ordering::Greater => Side::Rating,
            Ordering::Equal => Side::Both,
        }
    }
}

/// The ranges balance scores are measured on (spec §5.5): price per unit on
/// a log scale between the cheapest and the priciest rated candidate, rating
/// linearly between the lowest and the highest. A side that does not spread
/// gives every candidate full marks on it, so nothing divides by zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceNorm {
    pub price_min: UnitPrice,
    pub price_max: UnitPrice,
    pub rating_min: f64,
    pub rating_max: f64,
}

impl BalanceNorm {
    /// `None` when no candidate has an adjusted rating.
    pub fn of(candidates: &[Candidate]) -> Option<BalanceNorm> {
        let rated = || candidates.iter().filter_map(|c| c.adjusted_rating.map(|r| (c.unit_price, r)));
        Some(BalanceNorm {
            price_min: rated().map(|(p, _)| p).min()?,
            price_max: rated().map(|(p, _)| p).max()?,
            rating_min: rated().map(|(_, r)| r).fold(f64::INFINITY, f64::min),
            rating_max: rated().map(|(_, r)| r).fold(f64::NEG_INFINITY, f64::max),
        })
    }

    pub fn price_spreads(&self) -> bool {
        self.price_max != self.price_min
    }

    /// One rated product, or all ratings equal: nothing to spread.
    pub fn rating_spreads(&self) -> bool {
        self.rating_max > self.rating_min
    }

    fn ln_range(&self) -> (f64, f64) {
        (self.price_min.cents_f64().ln(), self.price_max.cents_f64().ln())
    }

    pub fn q_price(&self, unit_price: UnitPrice) -> f64 {
        if !self.price_spreads() {
            return 100.0;
        }
        let (ln_min, ln_max) = self.ln_range();
        100.0 * (ln_max - unit_price.cents_f64().ln()) / (ln_max - ln_min)
    }

    pub fn q_rating(&self, adjusted_rating: f64) -> f64 {
        if !self.rating_spreads() {
            return 100.0;
        }
        100.0 * (adjusted_rating - self.rating_min) / (self.rating_max - self.rating_min)
    }

    /// Price per unit, in cents, at which `q_price` equals `s`: a cheaper
    /// candidate scores above `s` on price. `None` when prices do not spread,
    /// so every candidate scores 100 on price.
    pub fn price_at(&self, s: f64) -> Option<f64> {
        if !self.price_spreads() {
            return None;
        }
        let (ln_min, ln_max) = self.ln_range();
        Some((ln_max - s / 100.0 * (ln_max - ln_min)).exp())
    }

    /// Adjusted rating at which `q_rating` equals `s`; `None` when ratings do
    /// not spread.
    pub fn rating_at(&self, s: f64) -> Option<f64> {
        self.rating_spreads()
            .then(|| self.rating_min + s / 100.0 * (self.rating_max - self.rating_min))
    }
}

/// Floats that differ by less than about 1e-9 compare equal (spec §5.5).
/// Rounding to a fixed grid keeps the comparison a total order, which an
/// epsilon comparison would not be.
#[allow(clippy::cast_possible_truncation)]
fn grid(x: f64) -> i64 {
    (x * 1e9).round() as i64
}

/// Scores for candidates that have both a unit price and an adjusted rating.
pub fn balance_scores(candidates: &[Candidate]) -> BTreeMap<IherbId, BalanceScore> {
    let mut out = BTreeMap::new();
    let Some(norm) = BalanceNorm::of(candidates) else {
        return out;
    };
    for c in candidates {
        let Some(r) = c.adjusted_rating else { continue };
        let (q_price, q_rating) = (norm.q_price(c.unit_price), norm.q_rating(r));
        out.insert(
            c.id.clone(),
            BalanceScore {
                q_price,
                q_rating,
                s: q_price.min(q_rating),
            },
        );
    }
    out
}

fn by_price_then_id(a: &Candidate, b: &Candidate) -> Ordering {
    a.unit_price.cmp(&b.unit_price).then_with(|| a.id.cmp(&b.id))
}

/// Candidates of `preset`, best first. Products without the data a preset
/// needs are left out, not placed last.
pub fn rank(preset: Preset, candidates: &[Candidate]) -> Vec<IherbId> {
    let mut list: Vec<&Candidate> = match preset {
        Preset::CheapestPerUnit | Preset::Balance => candidates.iter().collect(),
        Preset::FairRating => candidates.iter().filter(|c| c.adjusted_rating.is_some()).collect(),
        Preset::CheapestVerified => candidates.iter().filter(|c| c.third_party_tested).collect(),
    };
    match preset {
        Preset::CheapestPerUnit | Preset::CheapestVerified => list.sort_by(|a, b| by_price_then_id(a, b)),
        Preset::FairRating => list.sort_by(|a, b| {
            let (ra, rb) = (a.adjusted_rating.map_or(0, grid), b.adjusted_rating.map_or(0, grid));
            rb.cmp(&ra).then_with(|| by_price_then_id(a, b))
        }),
        Preset::Balance => {
            let scores = balance_scores(candidates);
            list.retain(|c| scores.contains_key(&c.id));
            list.sort_by(|a, b| {
                let (sa, sb) = (&scores[&a.id], &scores[&b.id]);
                grid(sb.s)
                    .cmp(&grid(sa.s))
                    .then_with(|| grid(sb.q_price + sb.q_rating).cmp(&grid(sa.q_price + sa.q_rating)))
                    .then_with(|| by_price_then_id(a, b))
            });
        }
    }
    list.into_iter().map(|c| c.id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, dollars: &str, r: Option<f64>, tested: bool) -> Candidate {
        Candidate {
            id: IherbId::new(id).unwrap(),
            unit_price: UnitPrice::from_dollars(dollars).unwrap(),
            adjusted_rating: r,
            third_party_tested: tested,
        }
    }

    #[test]
    fn single_rated_product_gets_full_scores() {
        let c = [cand("1", "0.10", Some(4.5), false), cand("2", "0.05", None, false)];
        let s = balance_scores(&c);
        assert_eq!(s.len(), 1);
        let one = s[&IherbId::new("1").unwrap()];
        assert!((one.q_rating - 100.0).abs() < 1e-12);
        assert!((one.q_price - 100.0).abs() < 1e-12);
        assert_eq!(rank(Preset::Balance, &c), vec![IherbId::new("1").unwrap()]);
    }

    #[test]
    fn cheapest_verified_filters() {
        let c = [
            cand("1", "0.10", None, true),
            cand("2", "0.05", None, false),
            cand("3", "0.07", None, true),
        ];
        let ids: Vec<String> = rank(Preset::CheapestVerified, &c).iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["3", "1"]);
    }

    #[test]
    fn equal_prices_tie_break_on_id() {
        let c = [cand("20", "0.10", None, false), cand("3", "0.10", None, false)];
        let ids: Vec<String> = rank(Preset::CheapestPerUnit, &c).iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["3", "20"]);
    }

    fn scores(c: &[Candidate]) -> Vec<BalanceScore> {
        balance_scores(c).into_values().collect()
    }

    #[test]
    fn degenerate_ranges_give_finite_scores() {
        // AC21: equal prices, then equal ratings, never divide by zero.
        let equal_prices = [cand("1", "0.10", Some(4.4), false), cand("2", "0.10", Some(4.6), false)];
        let equal_ratings = [cand("1", "0.10", Some(4.5), false), cand("2", "0.20", Some(4.5), false)];
        for set in [&equal_prices[..], &equal_ratings[..]] {
            let s = scores(set);
            assert_eq!(s.len(), 2);
            assert!(s.iter().all(|x| x.q_price.is_finite() && x.q_rating.is_finite() && x.s.is_finite()));
        }
        let norm = BalanceNorm::of(&equal_prices).unwrap();
        assert!(!norm.price_spreads() && norm.rating_spreads());
        assert_eq!(norm.price_at(50.0), None);
        assert!((norm.rating_at(50.0).unwrap() - 4.5).abs() < 1e-12);
        let norm = BalanceNorm::of(&equal_ratings).unwrap();
        assert!(norm.price_spreads() && !norm.rating_spreads());
        assert_eq!(norm.rating_at(50.0), None);
        assert_eq!(rank(Preset::Balance, &equal_prices).len(), 2);
    }

    #[test]
    fn no_candidates_and_no_rated_candidates() {
        // AC20 and AC22 at the core: empty inputs give empty rankings, not errors.
        for p in Preset::ALL {
            assert!(rank(p, &[]).is_empty());
        }
        assert!(BalanceNorm::of(&[]).is_none());
        let unrated = [cand("1", "0.10", None, false)];
        assert!(balance_scores(&unrated).is_empty());
        assert!(rank(Preset::Balance, &unrated).is_empty());
        assert!(rank(Preset::FairRating, &unrated).is_empty());
        assert!(rank(Preset::CheapestVerified, &unrated).is_empty());
    }

    #[test]
    fn thresholds_invert_the_scores() {
        // The map's "ranks above" area is drawn from price_at and rating_at;
        // they must land exactly on each candidate's own score.
        let c = [
            cand("1", "0.010", Some(4.32), false),
            cand("2", "0.060", Some(4.49), false),
            cand("3", "0.646", Some(4.67), false),
        ];
        let norm = BalanceNorm::of(&c).unwrap();
        for x in &c {
            let q_p = norm.q_price(x.unit_price);
            let q_r = norm.q_rating(x.adjusted_rating.unwrap());
            assert!((norm.price_at(q_p).unwrap() - x.unit_price.cents_f64()).abs() < 1e-9);
            assert!((norm.rating_at(q_r).unwrap() - x.adjusted_rating.unwrap()).abs() < 1e-9);
        }
        let mid = balance_scores(&c)[&IherbId::new("2").unwrap()];
        assert_eq!(mid.weaker(), Side::Rating);
    }

    #[test]
    fn display_rounding_does_not_change_order() {
        // AC24: both show as $0.060, but the exact values decide the order.
        let c = [cand("1", "0.0604", None, false), cand("2", "0.0596", None, false)];
        assert_eq!(c[0].unit_price.to_string(), c[1].unit_price.to_string());
        let ids: Vec<String> = rank(Preset::CheapestPerUnit, &c).iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["2", "1"]);
    }

    #[test]
    fn preset_keys_round_trip() {
        for p in Preset::ALL {
            assert_eq!(Preset::from_key(p.key()), Some(p));
        }
    }
}
