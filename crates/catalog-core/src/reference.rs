//! The hand-maintained reference: substances, their forms and the categories
//! built on them (`data/reference/*.toml`).

use std::collections::BTreeMap;

use crate::chem::{Formula, FormulaError};
use crate::ids::{CategoryId, FormId, IdError, Slug, SubstanceId};
use crate::schema::{CategoriesFile, FormsFile, SubstancesFile};
use crate::units::{IuConversion, Mass, Ratio, UnitError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substance {
    pub id: SubstanceId,
    pub name: String,
    pub synonyms: Vec<String>,
    /// Unit that labels normally use for this substance, e.g. `mg`.
    pub default_unit: String,
    /// Chemical symbol when the substance is an element (`Mg`); used to check
    /// form ratios against their formulas.
    pub element: Option<String>,
    pub iu_conversion: Option<IuConversion>,
    pub source_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub id: FormId,
    pub substance: SubstanceId,
    pub name: String,
    pub synonyms: Vec<String>,
    /// Mass of the substance per mass of the declared compound.
    pub elemental_ratio: Ratio,
    pub formula: Option<Formula>,
    /// Stored for phase 2; phase 1 always uses `k_form = 1` (spec §5.2).
    pub bioavailability_k: Option<Ratio>,
    pub bioavailability_note: Option<String>,
    /// Overrides the substance's IU conversion where the factor depends on
    /// the form (vitamins A and E).
    pub iu_conversion: Option<IuConversion>,
    pub source_url: String,
}

impl Form {
    /// The coefficient used in `P_unit`. Fixed at one in phase 1.
    pub fn k_form(&self) -> Ratio {
        Ratio::ONE
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub slug: Slug,
    pub name: String,
    pub substance: SubstanceId,
    /// The mass that one displayed unit price refers to, e.g. 100 mg.
    pub unit: Mass,
    /// `per 100 mg magnesium`.
    pub unit_label: String,
    /// Feed category strings that place a product in this category.
    pub feed_match: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error(transparent)]
    Id(#[from] IdError),
    #[error("{context}: {source}")]
    Unit { context: String, source: UnitError },
    #[error("{context}: {source}")]
    Formula { context: String, source: FormulaError },
    #[error("unsupported schema version {0}")]
    Schema(u32),
    #[error("duplicate id `{0}`")]
    Duplicate(String),
    #[error("{0} refers to unknown substance `{1}`")]
    UnknownSubstance(String, String),
    #[error("form `{0}`: elemental ratio {1} is above one")]
    RatioAboveOne(String, Ratio),
    #[error("form `{form}`: elemental ratio {stored} does not match {derived} derived from formula `{formula}`")]
    RatioMismatch {
        form: String,
        stored: Ratio,
        derived: Ratio,
        formula: String,
    },
    #[error("form `{0}`: has a formula but its substance has no element symbol")]
    FormulaWithoutElement(String),
    #[error("{0}: source_url must be an https link")]
    Source(String),
    #[error("{0}: must not be empty")]
    Empty(String),
}

fn check_source(owner: &str, url: &str) -> Result<(), ReferenceError> {
    if !url.starts_with("https://") || url.len() <= "https://".len() {
        return Err(ReferenceError::Source(owner.to_owned()));
    }
    Ok(())
}

fn unit_err(context: String) -> impl FnOnce(UnitError) -> ReferenceError {
    move |source| ReferenceError::Unit { context, source }
}

/// Supported schema version of every reference and label file.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    pub substances: BTreeMap<SubstanceId, Substance>,
    pub forms: BTreeMap<FormId, Form>,
    pub categories: BTreeMap<CategoryId, Category>,
}

impl Reference {
    /// Validates the three reference files together.
    pub fn from_files(substances: SubstancesFile, forms: FormsFile, categories: CategoriesFile) -> Result<Reference, ReferenceError> {
        for v in [substances.schema, forms.schema, categories.schema] {
            if v != SCHEMA_VERSION {
                return Err(ReferenceError::Schema(v));
            }
        }
        let mut r = Reference::default();

        for s in substances.substance {
            let id = SubstanceId::new(s.id)?;
            let ctx = format!("substance `{id}`");
            check_source(&ctx, &s.source_url)?;
            if s.name.trim().is_empty() {
                return Err(ReferenceError::Empty(format!("{ctx} name")));
            }
            let iu_conversion = s
                .ug_per_iu
                .as_deref()
                .map(|v| Ratio::parse(v).map(|r| IuConversion::new(id.clone(), r)))
                .transpose()
                .map_err(unit_err(format!("{ctx} ug_per_iu")))?;
            let sub = Substance {
                id: id.clone(),
                name: s.name,
                synonyms: s.synonyms,
                default_unit: s.default_unit,
                element: s.element,
                iu_conversion,
                source_url: s.source_url,
            };
            if r.substances.insert(id.clone(), sub).is_some() {
                return Err(ReferenceError::Duplicate(id.to_string()));
            }
        }

        for f in forms.form {
            let id = FormId::new(f.id)?;
            let ctx = format!("form `{id}`");
            let substance = SubstanceId::new(f.substance)?;
            let Some(sub) = r.substances.get(&substance) else {
                return Err(ReferenceError::UnknownSubstance(ctx, substance.to_string()));
            };
            check_source(&ctx, &f.source_url)?;
            let elemental_ratio = Ratio::parse(&f.elemental_ratio).map_err(unit_err(format!("{ctx} elemental_ratio")))?;
            if !elemental_ratio.is_at_most_one() {
                return Err(ReferenceError::RatioAboveOne(id.to_string(), elemental_ratio));
            }
            let formula = f
                .formula
                .as_deref()
                .map(Formula::parse)
                .transpose()
                .map_err(|source| ReferenceError::Formula {
                    context: ctx.clone(),
                    source,
                })?;
            if let Some(formula) = &formula {
                let Some(element) = &sub.element else {
                    return Err(ReferenceError::FormulaWithoutElement(id.to_string()));
                };
                let derived = formula.elemental_ratio(element).map_err(|source| ReferenceError::Formula {
                    context: ctx.clone(),
                    source,
                })?;
                if derived != elemental_ratio {
                    return Err(ReferenceError::RatioMismatch {
                        form: id.to_string(),
                        stored: elemental_ratio,
                        derived,
                        formula: formula.text().to_owned(),
                    });
                }
            }
            let bioavailability_k = f
                .bioavailability_k
                .as_deref()
                .map(Ratio::parse)
                .transpose()
                .map_err(unit_err(format!("{ctx} bioavailability_k")))?;
            let iu_conversion = f
                .ug_per_iu
                .as_deref()
                .map(|v| Ratio::parse(v).map(|r| IuConversion::new(substance.clone(), r)))
                .transpose()
                .map_err(unit_err(format!("{ctx} ug_per_iu")))?;
            let form = Form {
                id: id.clone(),
                substance,
                name: f.name,
                synonyms: f.synonyms,
                elemental_ratio,
                formula,
                bioavailability_k,
                bioavailability_note: f.bioavailability_note,
                iu_conversion,
                source_url: f.source_url,
            };
            if r.forms.insert(id.clone(), form).is_some() {
                return Err(ReferenceError::Duplicate(id.to_string()));
            }
        }

        for c in categories.category {
            let id = CategoryId::new(c.id)?;
            let ctx = format!("category `{id}`");
            let substance = SubstanceId::new(c.substance)?;
            if !r.substances.contains_key(&substance) {
                return Err(ReferenceError::UnknownSubstance(ctx, substance.to_string()));
            }
            let unit = Mass::parse(&c.unit).map_err(unit_err(format!("{ctx} unit")))?;
            if unit == Mass::ZERO {
                return Err(ReferenceError::Empty(format!("{ctx} unit")));
            }
            let cat = Category {
                id: id.clone(),
                slug: Slug::new(c.slug)?,
                name: c.name,
                substance,
                unit,
                unit_label: c.unit_label,
                feed_match: c.feed_match,
            };
            if r.categories.insert(id.clone(), cat).is_some() {
                return Err(ReferenceError::Duplicate(id.to_string()));
            }
        }
        Ok(r)
    }

    pub fn category_by_slug(&self, slug: &str) -> Option<&Category> {
        self.categories.values().find(|c| c.slug.as_str() == slug)
    }

    /// The IU conversion for a form: its own when the factor depends on the
    /// form, otherwise its substance's.
    pub fn iu_conversion<'a>(&'a self, form: &'a Form) -> Option<&'a IuConversion> {
        form.iu_conversion
            .as_ref()
            .or_else(|| self.substances.get(&form.substance).and_then(|s| s.iu_conversion.as_ref()))
    }
}
