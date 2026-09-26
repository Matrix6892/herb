//! The `balance` control set from spec §5.5 (`data/fixtures/balance_abcdef.toml`).
//!
//! Three consumers check the same numbers: the core tests, `vitrina verify`
//! before every publication, and the `/how` page test.

use serde::Deserialize;

use crate::ids::IherbId;
use crate::pricing::UnitPrice;
use crate::scoring::{BalanceScore, Candidate, Preset, balance_scores, rank};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceFixture {
    pub tolerance: f64,
    pub expected_order: Vec<String>,
    pub product: Vec<FixtureProduct>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureProduct {
    pub name: String,
    pub id: String,
    /// Dollars per 100 mg.
    pub p_unit: String,
    pub r_b: f64,
    pub q_price: f64,
    pub q_rating: f64,
    pub s: f64,
}

/// One product of the control set with the scores the code computed.
#[derive(Debug, Clone)]
pub struct ControlRow {
    pub name: String,
    pub p_unit: UnitPrice,
    pub r_b: f64,
    pub score: BalanceScore,
}

#[derive(Debug, Clone)]
pub struct ControlResult {
    /// In computed `balance` order.
    pub rows: Vec<ControlRow>,
    pub order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ControlError {
    #[error("fixture: {0}")]
    Fixture(String),
    #[error("{name}: {field} is {got:.4}, expected {expected} ± {tolerance}")]
    Score {
        name: String,
        field: &'static str,
        got: f64,
        expected: f64,
        tolerance: f64,
    },
    #[error("order is {got:?}, expected {expected:?}")]
    Order { got: Vec<String>, expected: Vec<String> },
}

impl BalanceFixture {
    /// Computes the control set and compares it with the expected values.
    pub fn check(&self) -> Result<ControlResult, ControlError> {
        let mut candidates = Vec::new();
        let mut names = std::collections::BTreeMap::new();
        for p in &self.product {
            let id = IherbId::new(p.id.clone()).map_err(|e| ControlError::Fixture(e.to_string()))?;
            let unit_price = UnitPrice::from_dollars(&p.p_unit).map_err(|e| ControlError::Fixture(e.to_string()))?;
            names.insert(id.clone(), p);
            candidates.push(Candidate {
                id,
                unit_price,
                adjusted_rating: Some(p.r_b),
                third_party_tested: false,
            });
        }
        let scores = balance_scores(&candidates);
        let order_ids = rank(Preset::Balance, &candidates);
        let mut rows = Vec::new();
        for id in &order_ids {
            let p = names[id];
            let score = scores[id];
            for (field, got, expected) in [
                ("q_price", score.q_price, p.q_price),
                ("q_rating", score.q_rating, p.q_rating),
                ("S", score.s, p.s),
            ] {
                if (got - expected).abs() > self.tolerance {
                    return Err(ControlError::Score {
                        name: p.name.clone(),
                        field,
                        got,
                        expected,
                        tolerance: self.tolerance,
                    });
                }
            }
            let unit_price = candidates
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.unit_price)
                .expect("candidate exists");
            rows.push(ControlRow {
                name: p.name.clone(),
                p_unit: unit_price,
                r_b: p.r_b,
                score,
            });
        }
        let order: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        if order != self.expected_order {
            return Err(ControlError::Order {
                got: order,
                expected: self.expected_order.clone(),
            });
        }
        Ok(ControlResult { rows, order })
    }
}
