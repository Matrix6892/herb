//! `c/<slug>.json`: the category as `catalog-core` computed it, for scripts
//! that draw it (ADR 0020). Every order, score, threshold, ratio and number
//! text a script shows comes from here; a script positions and formats, it
//! never ranks or scores (AC23).
//!
//! Numbers that are not finite are written as `null`, never as `NaN`, and a
//! value the view does not have is left out rather than zeroed (И7).

use std::collections::BTreeMap;

use catalog_core::{Exclusion, IherbId, Lead, Preset, RULES_VERSION};
use serde::Serialize;

use crate::{Built, Ctx, category_path, label_view, product_path};

pub const SCHEMA: &str = "vitrina.category-evaluation/1";

/// Site path of the evaluation file of a category page.
pub fn evaluation_file(category_page_file: &str) -> String {
    category_page_file.trim_end_matches(".html").to_owned() + ".json"
}

fn fin(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

#[derive(Serialize)]
pub struct EvaluationFile {
    pub schema: &'static str,
    /// Version of the formulas in `catalog-core`.
    pub rules: &'static str,
    pub snapshot: Snapshot,
    pub branch: &'static str,
    pub category: CategoryOut,
    /// Median unit price over ranked products.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_unit_price: Option<Num>,
    /// Priciest over cheapest unit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_spread: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_norm: Option<NormOut>,
    pub presets: Vec<PresetOut>,
    pub products: Vec<ProductOut>,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub date: String,
    pub feed_fetched_at: String,
    pub feed_fetched_at_text: String,
    pub feed_sha256: String,
}

#[derive(Serialize)]
pub struct CategoryOut {
    pub slug: String,
    pub name: String,
    pub href: String,
    pub substance: String,
    /// `100 mg`.
    pub unit: String,
    /// `per 100 mg of magnesium`.
    pub per_unit: String,
    pub products: usize,
    pub compared: usize,
}

/// A number with the text the site shows for it.
#[derive(Serialize)]
pub struct Num {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub text: String,
}

#[derive(Serialize)]
pub struct NormOut {
    pub price_min_cents: Option<f64>,
    pub price_max_cents: Option<f64>,
    pub rating_min: Option<f64>,
    pub rating_max: Option<f64>,
    pub price_spreads: bool,
    pub rating_spreads: bool,
}

#[derive(Serialize)]
pub struct PresetOut {
    pub key: &'static str,
    pub label: String,
    pub note: String,
    pub default: bool,
    /// Product ids, best first.
    pub order: Vec<String>,
    /// Why the ranking is empty; only when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty: Option<String>,
    /// #1 over #2; absent with fewer than two ranked products.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead: Option<LeadOut>,
}

#[derive(Serialize)]
pub struct LeadOut {
    pub runner_up: String,
    /// `balance_points`, `price_ratio` or `rating_diff`.
    pub kind: &'static str,
    pub value: Option<f64>,
}

#[derive(Serialize)]
pub struct ProductOut {
    pub id: String,
    pub brand: String,
    pub title: String,
    pub href: String,
    /// `compared`, `not_compared` or `pending_label`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_key: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_price: Option<Num>,
    /// Unit price as a multiple of the median.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_median: Option<f64>,
    /// 1-based place in each preset that ranks the product.
    pub positions: BTreeMap<&'static str, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jar_price: Option<Num>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_stock: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servings: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serving_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_serving: Option<Num>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_container: Option<Num>,
    /// Absent in branch B and without a rating in the feed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<RatingOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<BalanceOut>,
    /// The mark as printed on the label; not a confirmed certification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_mark: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formula_changed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<LabelOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_href: Option<String>,
}

#[derive(Serialize)]
pub struct RatingOut {
    /// The feed's average as it was written (`4.7`), not its binary
    /// neighbour (`4.699999809…`).
    pub avg: f64,
    pub avg_text: String,
    pub count: u32,
    pub adjusted: Option<f64>,
    pub adjusted_text: Option<String>,
}

#[derive(Serialize)]
pub struct BalanceOut {
    pub q_price: Option<f64>,
    pub q_rating: Option<f64>,
    pub s: Option<f64>,
    /// `price`, `rating` or `both`.
    pub weaker: &'static str,
    /// Products cheaper than this, in cents per unit, score above `s` on
    /// price; `null` when prices do not spread.
    pub price_at_cents: Option<f64>,
    /// Products rated above this score above `s` on rating; `null` when
    /// ratings do not spread.
    pub rating_at: Option<f64>,
}

#[derive(Serialize)]
pub struct LabelOut {
    pub verified_by: String,
    pub verified_at: String,
    pub per_serving_text: Option<String>,
    pub lines: Vec<LineOut>,
}

#[derive(Serialize)]
pub struct LineOut {
    pub printed: String,
    pub counted_as: String,
    pub factor: String,
    pub result: String,
}

pub(crate) fn evaluation_file_for(ctx: &Ctx<'_>, b: &Built) -> EvaluationFile {
    let (t, c, view) = (ctx.t, &b.category, &b.view);
    let branch = ctx.opts.branch;
    let unit = t.mass(c.unit);
    let default = branch.default_preset();
    let mut presets: Vec<Preset> = vec![default];
    presets.extend(branch.presets().iter().copied().filter(|p| *p != default));

    let presets_out = presets
        .iter()
        .map(|p| {
            let order: Vec<String> = view
                .rankings
                .get(p)
                .map_or_else(Vec::new, |ids| ids.iter().map(ToString::to_string).collect());
            PresetOut {
                key: p.key(),
                label: ctx.preset_label(*p, c),
                note: ctx.preset_note(*p, c),
                default: *p == default,
                empty: order.is_empty().then(|| ctx.preset_empty(*p, c)),
                lead: view.lead(*p).map(|l| {
                    let (kind, value) = match l.lead {
                        Lead::BalancePoints(v) => ("balance_points", v),
                        Lead::PriceRatio(v) => ("price_ratio", v),
                        Lead::RatingDiff(v) => ("rating_diff", v),
                    };
                    LeadOut {
                        runner_up: l.runner_up.to_string(),
                        kind,
                        value: fin(value),
                    }
                }),
                order,
            }
        })
        .collect();

    let products = b
        .products
        .iter()
        .filter_map(|p| {
            let e = view.evaluations.get(&p.iherb_id)?;
            let id: &IherbId = &p.iherb_id;
            let (status, reason, reason_key) = match &e.unit_price {
                Ok(_) => ("compared", None, None),
                Err(x) if x.is_pending_label() => ("pending_label", Some(ctx.exclusion(x, c)), Some(x.key())),
                Err(x) => ("not_compared", Some(ctx.exclusion(x, c)), Some(Exclusion::key(x))),
            };
            let positions = presets
                .iter()
                .filter_map(|preset| view.position(*preset, id).map(|pos| (preset.key(), pos)))
                .collect();
            let price = e.price.as_ref().and_then(|x| x.price);
            let servings = e.label.as_ref().map(|l| l.servings_per_container);
            Some(ProductOut {
                id: id.to_string(),
                brand: p.brand.clone(),
                title: p.title.clone(),
                href: product_path(p),
                status,
                reason,
                reason_key,
                unit_price: e.unit_price.as_ref().ok().map(|up| Num {
                    value: fin(up.cents_f64()),
                    text: t.unit_price(*up),
                }),
                vs_median: view.price_vs_median(id).and_then(fin),
                positions,
                jar_price: price.map(|m| Num {
                    value: i32::try_from(m.cents()).ok().map(f64::from),
                    text: t.money(m),
                }),
                in_stock: e.price.as_ref().map(|x| x.in_stock),
                servings,
                serving_size: e.label.as_ref().map(|l| l.serving_size),
                per_serving: e.dose.as_ref().map(|d| Num {
                    value: None,
                    text: t.mass(d.per_serving),
                }),
                per_container: e.dose.as_ref().zip(servings).and_then(|(d, n)| d.per_container(n)).map(|m| Num {
                    value: None,
                    text: t.mass(m),
                }),
                rating: branch.uses_ratings().then_some(()).and(e.rating).map(|r| RatingOut {
                    avg: r.avg().to_string().parse().unwrap_or_else(|_| f64::from(r.avg())),
                    avg_text: t.decimal(f64::from(r.avg()), 1),
                    count: r.count(),
                    adjusted: e.adjusted_rating.and_then(fin),
                    adjusted_text: e.adjusted_rating.map(|a| t.decimal(a, 2)),
                }),
                balance: view.balance.get(id).map(|s| BalanceOut {
                    q_price: fin(s.q_price),
                    q_rating: fin(s.q_rating),
                    s: fin(s.s),
                    weaker: s.weaker().key(),
                    price_at_cents: view.balance_norm.and_then(|n| n.price_at(s.s)).and_then(fin),
                    rating_at: view.balance_norm.and_then(|n| n.rating_at(s.s)).and_then(fin),
                }),
                label_mark: e.third_party_test().map(ToOwned::to_owned),
                formula_changed: e.formula_changed.map(|d| d.to_string()),
                label: e.label.as_ref().map(|l| {
                    let v = label_view(ctx, c, e, l);
                    LabelOut {
                        verified_by: l.verified_by.clone(),
                        verified_at: l.verified_at.to_string(),
                        per_serving_text: v.per_serving,
                        lines: v
                            .lines
                            .into_iter()
                            .map(|x| LineOut {
                                printed: x.printed,
                                counted_as: x.counted_as,
                                factor: x.factor,
                                result: x.result,
                            })
                            .collect(),
                    }
                }),
                buy_href: ctx.buy_href(p),
            })
        })
        .collect();

    EvaluationFile {
        schema: SCHEMA,
        rules: RULES_VERSION,
        snapshot: Snapshot {
            date: ctx.run.date.to_string(),
            feed_fetched_at: ctx.run.fetched_at.to_string(),
            feed_fetched_at_text: t.timestamp(ctx.run.fetched_at),
            feed_sha256: ctx.run.feed_sha256.clone(),
        },
        branch: branch.key(),
        category: CategoryOut {
            slug: c.slug.to_string(),
            name: c.name.clone(),
            href: category_path(c),
            substance: ctx.substance_name(c),
            per_unit: c.unit_label.clone(),
            unit,
            products: b.products.len(),
            compared: view.evaluations.values().filter(|e| e.unit_price.is_ok()).count(),
        },
        median_unit_price: view.median_unit_price_cents.and_then(fin).map(|m| Num {
            value: Some(m),
            text: t.cents_as_dollars(m),
        }),
        price_spread: view.price_spread().and_then(fin),
        balance_norm: view.balance_norm.map(|n| NormOut {
            price_min_cents: fin(n.price_min.cents_f64()),
            price_max_cents: fin(n.price_max.cents_f64()),
            rating_min: fin(n.rating_min),
            rating_max: fin(n.rating_max),
            price_spreads: n.price_spreads(),
            rating_spreads: n.rating_spreads(),
        }),
        presets: presets_out,
        products,
    }
}
