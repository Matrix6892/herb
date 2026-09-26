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

/// Floats that differ by less than about 1e-9 compare equal (spec §5.5).
/// Rounding to a fixed grid keeps the comparison a total order, which an
/// epsilon comparison would not be.
#[allow(clippy::cast_possible_truncation)]
fn grid(x: f64) -> i64 {
    (x * 1e9).round() as i64
}

/// Scores for candidates that have both a unit price and an adjusted rating.
pub fn balance_scores(candidates: &[Candidate]) -> BTreeMap<IherbId, BalanceScore> {
    let rated: Vec<(&Candidate, f64)> = candidates.iter().filter_map(|c| c.adjusted_rating.map(|r| (c, r))).collect();
    let mut out = BTreeMap::new();
    let (Some(p_min), Some(p_max)) = (
        rated.iter().map(|(c, _)| c.unit_price).min(),
        rated.iter().map(|(c, _)| c.unit_price).max(),
    ) else {
        return out;
    };
    let r_min = rated.iter().map(|(_, r)| *r).fold(f64::INFINITY, f64::min);
    let r_max = rated.iter().map(|(_, r)| *r).fold(f64::NEG_INFINITY, f64::max);
    let ln_min = p_min.cents_f64().ln();
    let ln_max = p_max.cents_f64().ln();
    for (c, r) in rated {
        let q_price = if p_max == p_min {
            100.0
        } else {
            100.0 * (ln_max - c.unit_price.cents_f64().ln()) / (ln_max - ln_min)
        };
        // One rated product, or all ratings equal: nothing to spread.
        let q_rating = if r_max <= r_min {
            100.0
        } else {
            100.0 * (r - r_min) / (r_max - r_min)
        };
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

    #[test]
    fn preset_keys_round_trip() {
        for p in Preset::ALL {
            assert_eq!(Preset::from_key(p.key()), Some(p));
        }
    }
}
