//! File schemas for `data/reference/*.toml` and `data/labels/*.toml`.
//!
//! These are the only shapes that humans and tools (`tools/label-assist`)
//! write. Parsing text into them happens in the caller; turning them into
//! domain types happens here, with validation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubstancesFile {
    pub schema: u32,
    #[serde(default)]
    pub substance: Vec<SubstanceEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubstanceEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub synonyms: Vec<String>,
    pub default_unit: String,
    pub element: Option<String>,
    /// `"1/40"` for vitamin D.
    pub ug_per_iu: Option<String>,
    pub source_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormsFile {
    pub schema: u32,
    #[serde(default)]
    pub form: Vec<FormEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormEntry {
    pub id: String,
    pub substance: String,
    pub name: String,
    #[serde(default)]
    pub synonyms: Vec<String>,
    /// `"24305/40304"`.
    pub elemental_ratio: String,
    pub formula: Option<String>,
    pub bioavailability_k: Option<String>,
    pub bioavailability_note: Option<String>,
    pub ug_per_iu: Option<String>,
    pub source_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CategoriesFile {
    pub schema: u32,
    #[serde(default)]
    pub category: Vec<CategoryEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryEntry {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub substance: String,
    /// `"100 mg"`.
    pub unit: String,
    pub unit_label: String,
    #[serde(default)]
    pub feed_match: Vec<String>,
}

/// `data/labels/<iherb_id>.toml`. A formula change appends a new `[[label]]`
/// with a later `effective_from`; old entries stay.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabelFile {
    pub schema: u32,
    pub iherb_id: String,
    #[serde(rename = "label", default)]
    pub labels: Vec<LabelEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabelEntry {
    pub effective_from: String,
    /// Units (capsules, tablets, scoops) per serving.
    pub serving_size: u32,
    pub servings_per_container: u32,
    pub verified_by: String,
    pub verified_at: String,
    pub label_photo_ref: Option<String>,
    /// Certification mark printed on the label, e.g. `USP Verified`.
    pub third_party_test: Option<String>,
    pub notes: Option<String>,
    #[serde(rename = "line", default)]
    pub lines: Vec<LineEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LineEntry {
    pub form: String,
    /// `"200 mg"` or `"1000 IU"`, exactly as printed.
    pub amount: String,
    pub declared_as: DeclaredAs,
    pub confidence: ConfidenceEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredAs {
    /// Mass of the compound, e.g. `Magnesium citrate 500 mg`.
    Salt,
    /// Mass of the element, e.g. `Magnesium (as citrate) 200 mg`.
    Elemental,
    Iu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceEntry {
    Verified,
    Recognized,
    Unknown,
}
