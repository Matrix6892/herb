//! One category, evaluated: which products take part in presets, which do
//! not and why, and the facts behind the "why here" chips (spec §5.6).

use std::collections::BTreeMap;

use crate::date::Date;
use crate::ids::IherbId;
use crate::label::{DoseBreakdown, DoseError, Label, elemental_per_serving};
use crate::pricing::{UnitPrice, UnitPriceError};
use crate::rating::{RatingObs, RatingPrior, adjusted, prior};
use crate::reference::{Category, Reference};
use crate::scoring::{BalanceNorm, BalanceScore, Candidate, Preset, RatingBranch, balance_scores, rank};
use crate::units::{Money, Ratio};

/// The latest price snapshot of a product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceObs {
    pub date: Date,
    pub price: Option<Money>,
    pub in_stock: bool,
    pub hidden_until_cart: bool,
}

#[derive(Debug, Clone)]
pub struct ProductInput {
    pub id: IherbId,
    /// All label versions, oldest first.
    pub labels: Vec<Label>,
    pub price: Option<PriceObs>,
    pub rating: Option<RatingObs>,
}

/// Why a product has no unit price. Shown on its page; never replaced by a
/// zero (И7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exclusion {
    NoLabel,
    Dose(DoseError),
    NoPriceSnapshot,
    NoPrice,
    HiddenUntilCart,
    UnitPrice(UnitPriceError),
}

impl Exclusion {
    /// Stable key for page text.
    pub fn key(&self) -> &'static str {
        match self {
            Exclusion::NoLabel => "no_label",
            Exclusion::Dose(e) => e.key(),
            Exclusion::NoPriceSnapshot => "no_price_snapshot",
            Exclusion::NoPrice => "no_price",
            Exclusion::HiddenUntilCart => "hidden_until_cart",
            Exclusion::UnitPrice(_) => "unit_price_error",
        }
    }

    /// Products waiting for a verified label get the "added to the queue"
    /// page rather than a product page.
    pub fn is_pending_label(&self) -> bool {
        matches!(self, Exclusion::NoLabel)
    }
}

#[derive(Debug, Clone)]
pub struct Evaluation {
    pub id: IherbId,
    /// The label in force on the evaluation date.
    pub label: Option<Label>,
    /// Earlier label versions exist; the current one took effect on this date.
    pub formula_changed: Option<Date>,
    pub dose: Option<DoseBreakdown>,
    pub price: Option<PriceObs>,
    pub unit_price: Result<UnitPrice, Exclusion>,
    /// `None` in branch B even when the feed has one.
    pub rating: Option<RatingObs>,
    pub adjusted_rating: Option<f64>,
}

impl Evaluation {
    pub fn third_party_test(&self) -> Option<&str> {
        self.label.as_ref().and_then(|l| l.third_party_test.as_deref())
    }
}

#[derive(Debug, Clone)]
pub struct CategoryView {
    pub branch: RatingBranch,
    pub as_of: Date,
    pub evaluations: BTreeMap<IherbId, Evaluation>,
    /// Only the presets of `branch`.
    pub rankings: BTreeMap<Preset, Vec<IherbId>>,
    pub balance: BTreeMap<IherbId, BalanceScore>,
    /// The ranges `balance` was measured on; `None` in branch B or without
    /// rated candidates.
    pub balance_norm: Option<BalanceNorm>,
    pub prior: Option<RatingPrior>,
    /// Median unit price over ranked products, cents per unit.
    pub median_unit_price_cents: Option<f64>,
}

/// How far #1 is ahead of #2, on the preset's own scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lead {
    /// Difference of balance scores, in points of 0–100.
    BalancePoints(f64),
    /// #2 pays this many times as much per unit.
    PriceRatio(f64),
    /// Difference of adjusted ratings.
    RatingDiff(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeadOver {
    pub runner_up: IherbId,
    pub lead: Lead,
}

impl CategoryView {
    /// 1-based position of `id` in `preset`, if it is ranked there.
    pub fn position(&self, preset: Preset, id: &IherbId) -> Option<usize> {
        self.rankings.get(&preset)?.iter().position(|x| x == id).map(|p| p + 1)
    }

    fn unit_price(&self, id: &IherbId) -> Option<UnitPrice> {
        self.evaluations.get(id)?.unit_price.as_ref().ok().copied()
    }

    /// The lead of #1 over #2 under `preset`. `None` with fewer than two
    /// ranked products: there is no #2 to be ahead of (AC17).
    pub fn lead(&self, preset: Preset) -> Option<LeadOver> {
        let order = self.rankings.get(&preset)?;
        let (first, second) = (order.first()?, order.get(1)?);
        let lead = match preset {
            Preset::Balance => Lead::BalancePoints(self.balance.get(first)?.s - self.balance.get(second)?.s),
            Preset::CheapestPerUnit | Preset::CheapestVerified => {
                Lead::PriceRatio(self.unit_price(second)?.cents_f64() / self.unit_price(first)?.cents_f64())
            }
            Preset::FairRating => {
                let r = |id: &IherbId| self.evaluations.get(id).and_then(|e| e.adjusted_rating);
                Lead::RatingDiff(r(first)? - r(second)?)
            }
        };
        Some(LeadOver {
            runner_up: second.clone(),
            lead,
        })
    }

    /// Unit price of `id` as a multiple of the median over ranked products.
    pub fn price_vs_median(&self, id: &IherbId) -> Option<f64> {
        let median = self.median_unit_price_cents?;
        (median > 0.0).then(|| self.unit_price(id).map(|up| up.cents_f64() / median))?
    }

    /// Priciest over cheapest unit price among ranked products; `None`
    /// without ranked products.
    pub fn price_spread(&self) -> Option<f64> {
        let prices = || self.evaluations.values().filter_map(|e| e.unit_price.as_ref().ok());
        Some(prices().max()?.cents_f64() / prices().min()?.cents_f64())
    }
}

fn current_label(labels: &[Label], as_of: Date) -> (Option<&Label>, Option<Date>) {
    let in_force: Vec<&Label> = labels.iter().filter(|l| l.effective_from <= as_of).collect();
    match in_force.as_slice() {
        [] => (None, None),
        [only] => (Some(only), None),
        [.., last] => (Some(last), Some(last.effective_from)),
    }
}

fn evaluate_one(input: &ProductInput, category: &Category, reference: &Reference, as_of: Date) -> Evaluation {
    let (label, formula_changed) = current_label(&input.labels, as_of);
    let dose = label.map(|l| elemental_per_serving(l, &category.substance, reference));
    let unit_price = match (label, &dose) {
        (None, _) | (_, None) => Err(Exclusion::NoLabel),
        (_, Some(Err(e))) => Err(Exclusion::Dose(e.clone())),
        (Some(label), Some(Ok(dose))) => match &input.price {
            None => Err(Exclusion::NoPriceSnapshot),
            Some(p) if p.hidden_until_cart => Err(Exclusion::HiddenUntilCart),
            Some(PriceObs { price: None, .. }) => Err(Exclusion::NoPrice),
            Some(PriceObs { price: Some(price), .. }) => {
                // k_form is fixed at one in phase 1 (spec §5.2); the form-level
                // coefficient is stored for phase 2 but not applied.
                UnitPrice::compute(*price, label.servings_per_container, dose.per_serving, Ratio::ONE, category.unit)
                    .map_err(Exclusion::UnitPrice)
            }
        },
    };
    Evaluation {
        id: input.id.clone(),
        label: label.cloned(),
        formula_changed,
        dose: dose.and_then(Result::ok),
        price: input.price.clone(),
        unit_price,
        rating: None,
        adjusted_rating: None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    Some(if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    })
}

pub fn evaluate(category: &Category, reference: &Reference, branch: RatingBranch, as_of: Date, inputs: &[ProductInput]) -> CategoryView {
    let mut evaluations: BTreeMap<IherbId, Evaluation> = inputs
        .iter()
        .map(|i| (i.id.clone(), evaluate_one(i, category, reference, as_of)))
        .collect();

    let prior = if branch.uses_ratings() {
        let sample: Vec<RatingObs> = inputs.iter().filter_map(|i| i.rating).collect();
        prior(&sample)
    } else {
        None
    };
    if let Some(p) = prior {
        for input in inputs {
            if let (Some(r), Some(e)) = (input.rating, evaluations.get_mut(&input.id)) {
                e.rating = Some(r);
                e.adjusted_rating = Some(adjusted(r, p));
            }
        }
    }

    let candidates: Vec<Candidate> = evaluations
        .values()
        .filter_map(|e| {
            e.unit_price.as_ref().ok().map(|up| Candidate {
                id: e.id.clone(),
                unit_price: *up,
                adjusted_rating: e.adjusted_rating,
                third_party_tested: e.third_party_test().is_some(),
            })
        })
        .collect();

    let rankings = branch.presets().iter().map(|p| (*p, rank(*p, &candidates))).collect();
    let (balance, balance_norm) = if branch.uses_ratings() {
        (balance_scores(&candidates), BalanceNorm::of(&candidates))
    } else {
        (BTreeMap::new(), None)
    };
    let mut prices: Vec<f64> = candidates.iter().map(|c| c.unit_price.cents_f64()).collect();
    prices.sort_by(f64::total_cmp);

    CategoryView {
        branch,
        as_of,
        evaluations,
        rankings,
        balance,
        balance_norm,
        prior,
        median_unit_price_cents: median(&prices),
    }
}

/// One "why here" fact. Text comes from `site/templates/why/<key>.txt`.
#[derive(Debug, Clone, PartialEq)]
pub enum Why {
    CheaperThanMedian { percent: u32 },
    PricierThanMedian { percent: u32 },
    Rating { avg: f32, count: u32 },
    FormulaChanged { year: i32 },
    ThirdPartyTest { name: String },
}

impl Why {
    pub fn key(&self) -> &'static str {
        match self {
            Why::CheaperThanMedian { .. } => "cheaper_than_median",
            Why::PricierThanMedian { .. } => "pricier_than_median",
            Why::Rating { .. } => "rating",
            Why::FormulaChanged { .. } => "formula_changed",
            Why::ThirdPartyTest { .. } => "third_party_test",
        }
    }
}

pub const MAX_WHY: usize = 3;

/// Up to three facts for a ranked product, most relevant first.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn why(view: &CategoryView, id: &IherbId) -> Vec<Why> {
    let Some(e) = view.evaluations.get(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let (Ok(up), Some(median)) = (&e.unit_price, view.median_unit_price_cents) {
        let diff = (median - up.cents_f64()) / median * 100.0;
        let percent = diff.abs().round();
        if percent >= 1.0 {
            let percent = percent.min(f64::from(u32::MAX)) as u32;
            out.push(if diff > 0.0 {
                Why::CheaperThanMedian { percent }
            } else {
                Why::PricierThanMedian { percent }
            });
        }
    }
    if let Some(r) = e.rating {
        out.push(Why::Rating {
            avg: r.avg(),
            count: r.count(),
        });
    }
    if let Some(d) = e.formula_changed {
        out.push(Why::FormulaChanged { year: d.year() });
    }
    if let Some(name) = e.third_party_test() {
        out.push(Why::ThirdPartyTest { name: name.to_owned() });
    }
    out.truncate(MAX_WHY);
    out
}
