//! Labels as printed, and the elemental dose computed from them (spec §5.1).
//!
//! What the label says (`Declared`) is kept apart from what we computed
//! (`LineDose`), so every number on a page can name its source.

use crate::date::Date;
use crate::ids::{FormId, IherbId, SubstanceId};
use crate::reference::{Reference, ReferenceError};
use crate::schema::{ConfidenceEntry, DeclaredAs, LabelFile};
use crate::units::{ExactMass, Iu, Mass, Overflow, Ratio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared {
    SaltMass(Mass),
    ElementalMass(Mass),
    Iu(Iu),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Verified,
    Recognized,
    Unknown,
}

impl Confidence {
    pub fn key(self) -> &'static str {
        match self {
            Confidence::Verified => "verified",
            Confidence::Recognized => "recognized",
            Confidence::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelLine {
    pub form: FormId,
    pub declared: Declared,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub product: IherbId,
    pub effective_from: Date,
    pub serving_size: u32,
    pub servings_per_container: u32,
    pub lines: Vec<LabelLine>,
    pub verified_by: String,
    pub verified_at: Date,
    pub label_photo_ref: Option<String>,
    pub third_party_test: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelError {
    #[error(transparent)]
    Reference(#[from] ReferenceError),
    #[error("{0}")]
    Invalid(String),
}

impl LabelFile {
    /// All label versions in the file, oldest first. Every form must exist in
    /// the reference.
    pub fn into_labels(self, reference: &Reference) -> Result<Vec<Label>, LabelError> {
        if self.schema != crate::reference::SCHEMA_VERSION {
            return Err(ReferenceError::Schema(self.schema).into());
        }
        let product = IherbId::new(self.iherb_id).map_err(ReferenceError::from)?;
        let invalid = |msg: String| LabelError::Invalid(format!("label {product}: {msg}"));
        if self.labels.is_empty() {
            return Err(invalid("no [[label]] entries".into()));
        }
        let mut out: Vec<Label> = Vec::with_capacity(self.labels.len());
        for entry in self.labels {
            let date = |field: &str, v: &str| Date::parse(v).map_err(|e| invalid(format!("{field}: {e}")));
            let effective_from = date("effective_from", &entry.effective_from)?;
            let verified_at = date("verified_at", &entry.verified_at)?;
            if entry.verified_by.trim().is_empty() {
                return Err(invalid("verified_by is required".into()));
            }
            if entry.serving_size == 0 || entry.servings_per_container == 0 {
                return Err(invalid("serving_size and servings_per_container must be positive".into()));
            }
            if entry.lines.is_empty() {
                return Err(invalid(format!("label from {effective_from} has no lines")));
            }
            if out.last().is_some_and(|prev| prev.effective_from >= effective_from) {
                return Err(invalid("labels must be listed oldest first with distinct effective_from".into()));
            }
            let mut lines = Vec::with_capacity(entry.lines.len());
            for l in entry.lines {
                let form = FormId::new(l.form).map_err(ReferenceError::from)?;
                if !reference.forms.contains_key(&form) {
                    return Err(invalid(format!("unknown form `{form}`")));
                }
                let bad_amount = |e| invalid(format!("line `{form}` amount: {e}"));
                let declared = match l.declared_as {
                    DeclaredAs::Salt => Declared::SaltMass(Mass::parse(&l.amount).map_err(bad_amount)?),
                    DeclaredAs::Elemental => Declared::ElementalMass(Mass::parse(&l.amount).map_err(bad_amount)?),
                    DeclaredAs::Iu => Declared::Iu(Iu::parse(&l.amount).map_err(bad_amount)?),
                };
                let confidence = match l.confidence {
                    ConfidenceEntry::Verified => Confidence::Verified,
                    ConfidenceEntry::Recognized => Confidence::Recognized,
                    ConfidenceEntry::Unknown => Confidence::Unknown,
                };
                lines.push(LabelLine {
                    form,
                    declared,
                    confidence,
                });
            }
            out.push(Label {
                product: product.clone(),
                effective_from,
                serving_size: entry.serving_size,
                servings_per_container: entry.servings_per_container,
                lines,
                verified_by: entry.verified_by,
                verified_at,
                label_photo_ref: entry.label_photo_ref,
                third_party_test: entry.third_party_test.filter(|s| !s.trim().is_empty()),
            });
        }
        Ok(out)
    }
}

/// How one label line turned into an elemental amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDose {
    pub line: usize,
    pub form: FormId,
    pub declared: Declared,
    /// Factor applied to the declared amount: the form's elemental ratio for
    /// a salt mass, one for an elemental mass, µg per IU for IU.
    pub factor: Ratio,
    pub elemental: ExactMass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoseBreakdown {
    pub substance: SubstanceId,
    pub lines: Vec<LineDose>,
    /// Sum of the lines, rounded down to a microgram once.
    pub per_serving: Mass,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DoseError {
    #[error("the label has no line for {0}")]
    NoLineForSubstance(SubstanceId),
    #[error("line {line} ({form}) has unknown confidence")]
    UnknownLine { line: usize, form: FormId },
    #[error("line {line} ({form}) is recognised but not verified")]
    NotVerified { line: usize, form: FormId },
    #[error("line {line}: form `{form}` is not in the reference")]
    UnknownForm { line: usize, form: FormId },
    #[error("line {line} ({form}) is in IU but there is no IU conversion for it")]
    NoIuConversion { line: usize, form: FormId },
    #[error("line {line} ({form}): IU conversion belongs to another substance")]
    ForeignIuConversion { line: usize, form: FormId },
    #[error("the dose is zero")]
    Zero,
    #[error(transparent)]
    Overflow(#[from] Overflow),
}

impl DoseError {
    /// Stable key for page text.
    pub fn key(&self) -> &'static str {
        match self {
            DoseError::NoLineForSubstance(_) => "no_line",
            DoseError::UnknownLine { .. } => "unknown_line",
            DoseError::NotVerified { .. } => "not_verified",
            DoseError::UnknownForm { .. } => "unknown_form",
            DoseError::NoIuConversion { .. } | DoseError::ForeignIuConversion { .. } => "no_iu_conversion",
            DoseError::Zero => "zero_dose",
            DoseError::Overflow(_) => "overflow",
        }
    }
}

/// Elemental dose of `substance` per serving.
///
/// A line with unknown confidence excludes the product from comparison, and
/// every line of the substance itself must be verified (spec §4.2, §5.2).
pub fn elemental_per_serving(label: &Label, substance: &SubstanceId, reference: &Reference) -> Result<DoseBreakdown, DoseError> {
    let mut lines = Vec::new();
    let mut total = ExactMass::ZERO;
    for (i, line) in label.lines.iter().enumerate() {
        let n = i + 1;
        if line.confidence == Confidence::Unknown {
            return Err(DoseError::UnknownLine {
                line: n,
                form: line.form.clone(),
            });
        }
        let form = reference.forms.get(&line.form).ok_or_else(|| DoseError::UnknownForm {
            line: n,
            form: line.form.clone(),
        })?;
        if &form.substance != substance {
            continue;
        }
        if line.confidence != Confidence::Verified {
            return Err(DoseError::NotVerified {
                line: n,
                form: line.form.clone(),
            });
        }
        let (factor, elemental) = match line.declared {
            Declared::ElementalMass(m) => (Ratio::ONE, ExactMass::from_mass(m)),
            Declared::SaltMass(m) => (form.elemental_ratio, ExactMass::from_mass(m).times(form.elemental_ratio)?),
            Declared::Iu(iu) => {
                let conv = reference.iu_conversion(form).ok_or_else(|| DoseError::NoIuConversion {
                    line: n,
                    form: line.form.clone(),
                })?;
                if conv.substance() != substance {
                    return Err(DoseError::ForeignIuConversion {
                        line: n,
                        form: line.form.clone(),
                    });
                }
                (conv.ug_per_iu(), conv.to_mass(iu)?)
            }
        };
        total = total.plus(elemental)?;
        lines.push(LineDose {
            line: n,
            form: line.form.clone(),
            declared: line.declared,
            factor,
            elemental,
        });
    }
    if lines.is_empty() {
        return Err(DoseError::NoLineForSubstance(substance.clone()));
    }
    let per_serving = total.floor()?;
    if per_serving == Mass::ZERO {
        return Err(DoseError::Zero);
    }
    Ok(DoseBreakdown {
        substance: substance.clone(),
        lines,
        per_serving,
    })
}
