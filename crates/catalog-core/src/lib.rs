//! Domain core of the showcase: units, reference data, labels, dose and price
//! normalisation, rating adjustment and presets.
//!
//! No IO and no clock: every function is a calculation on its arguments, so
//! the crate builds for `wasm32-unknown-unknown` and each formula lives in
//! exactly one place (spec §3.1). The formulas are restated in words on the
//! `/how` page, and `site-gen` tests that the two agree.

pub mod category;
pub mod chem;
pub mod control;
pub mod date;
pub mod ids;
pub mod label;
pub mod pricing;
pub mod product;
pub mod rating;
pub mod reference;
pub mod schema;
pub mod scoring;
pub mod units;

pub use category::{CategoryView, Evaluation, Exclusion, PriceObs, ProductInput, Why, evaluate, why};
pub use date::{Date, UtcTimestamp};
pub use ids::{CategoryId, FormId, IherbId, Slug, SubstanceId};
pub use label::{Confidence, Declared, DoseBreakdown, DoseError, Label, LabelLine, LineDose, elemental_per_serving};
pub use pricing::UnitPrice;
pub use product::{Product, ProductStatus};
pub use rating::RatingObs;
pub use reference::{Category, Form, Reference, Substance};
pub use scoring::{BalanceScore, Candidate, Preset, RatingBranch};
pub use units::{ExactMass, Iu, IuConversion, Mass, Money, Ratio};
