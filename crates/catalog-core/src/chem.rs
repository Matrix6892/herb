//! Chemical formulas and the elemental fraction derived from them.
//!
//! The reference stores each salt's elemental ratio explicitly (spec §4.2)
//! together with its formula. Validation recomputes the ratio from the formula
//! and the standard atomic weights below and rejects any mismatch, so a typo
//! in either field cannot reach a price.

use std::collections::BTreeMap;

use crate::units::Ratio;

/// Conventional standard atomic weights, IUPAC CIAAW 2021
/// (<https://www.ciaaw.org/atomic-weights.htm>), in milligrams per mole so
/// they stay integers.
const ATOMIC_WEIGHTS_MG_PER_MOL: &[(&str, u64)] = &[
    ("H", 1_008),
    ("C", 12_011),
    ("N", 14_007),
    ("O", 15_999),
    ("Na", 22_990),
    ("Mg", 24_305),
    ("P", 30_974),
    ("S", 32_060),
    ("Cl", 35_450),
    ("K", 39_098),
    ("Ca", 40_078),
    ("Fe", 55_845),
    ("Zn", 65_380),
];

pub const ATOMIC_WEIGHTS_SOURCE: &str = "https://www.ciaaw.org/atomic-weights.htm";

pub fn atomic_weight(symbol: &str) -> Option<u64> {
    ATOMIC_WEIGHTS_MG_PER_MOL.iter().find(|(s, _)| *s == symbol).map(|(_, w)| *w)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormulaError {
    #[error("formula `{0}`: unexpected character at byte {1}")]
    Syntax(String, usize),
    #[error("formula `{0}`: unknown element `{1}`")]
    UnknownElement(String, String),
    #[error("formula `{0}`: unbalanced parentheses")]
    Parentheses(String),
    #[error("formula `{0}`: does not contain `{1}`")]
    ElementAbsent(String, String),
    #[error("formula `{0}`: count or mass out of range")]
    Range(String),
}

/// Element counts of one formula unit, e.g. `Mg3(C6H5O7)2` or `MgCl2·6H2O`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formula {
    text: String,
    atoms: BTreeMap<String, u32>,
}

impl Formula {
    pub fn parse(text: &str) -> Result<Formula, FormulaError> {
        let mut atoms = BTreeMap::new();
        // `·` or `.` separates hydrate parts, each with an optional leading
        // multiplier: `MgCl2·6H2O`.
        for part in text.split(['·', '.']) {
            let bytes = part.as_bytes();
            let (mult, start) = read_number(bytes, 0);
            let mult = mult.unwrap_or(1);
            let (counts, end) = parse_group(text, bytes, start, 0)?;
            if end != bytes.len() {
                return Err(FormulaError::Syntax(text.to_owned(), end));
            }
            for (el, n) in counts {
                let add = n.checked_mul(mult).ok_or_else(|| FormulaError::Range(text.to_owned()))?;
                let slot = atoms.entry(el).or_insert(0u32);
                *slot = slot.checked_add(add).ok_or_else(|| FormulaError::Range(text.to_owned()))?;
            }
        }
        if atoms.is_empty() {
            return Err(FormulaError::Syntax(text.to_owned(), 0));
        }
        Ok(Formula {
            text: text.to_owned(),
            atoms,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn count(&self, element: &str) -> u32 {
        self.atoms.get(element).copied().unwrap_or(0)
    }

    /// Molar mass in mg/mol.
    pub fn molar_mass(&self) -> Result<u64, FormulaError> {
        let mut total: u64 = 0;
        for (el, n) in &self.atoms {
            let w = atomic_weight(el).ok_or_else(|| FormulaError::UnknownElement(self.text.clone(), el.clone()))?;
            total = w
                .checked_mul(u64::from(*n))
                .and_then(|m| total.checked_add(m))
                .ok_or_else(|| FormulaError::Range(self.text.clone()))?;
        }
        Ok(total)
    }

    /// Mass fraction of `element`: `n × A(element) / M(formula)`.
    pub fn elemental_ratio(&self, element: &str) -> Result<Ratio, FormulaError> {
        let n = self.count(element);
        if n == 0 {
            return Err(FormulaError::ElementAbsent(self.text.clone(), element.to_owned()));
        }
        let w = atomic_weight(element).ok_or_else(|| FormulaError::UnknownElement(self.text.clone(), element.to_owned()))?;
        let range = || FormulaError::Range(self.text.clone());
        let num = u32::try_from(w * u64::from(n)).map_err(|_| range())?;
        let den = u32::try_from(self.molar_mass()?).map_err(|_| range())?;
        Ratio::new(num, den).map_err(|_| range())
    }
}

fn read_number(b: &[u8], mut i: usize) -> (Option<u32>, usize) {
    let start = i;
    let mut v: u32 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add(u32::from(b[i] - b'0'));
        i += 1;
    }
    if i == start { (None, i) } else { (Some(v), i) }
}

fn parse_group(text: &str, b: &[u8], mut i: usize, depth: u32) -> Result<(BTreeMap<String, u32>, usize), FormulaError> {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let range = || FormulaError::Range(text.to_owned());
    while i < b.len() {
        match b[i] {
            b'(' => {
                let (inner, after) = parse_group(text, b, i + 1, depth + 1)?;
                if after >= b.len() || b[after] != b')' {
                    return Err(FormulaError::Parentheses(text.to_owned()));
                }
                let (n, next) = read_number(b, after + 1);
                let n = n.unwrap_or(1);
                for (el, c) in inner {
                    let add = c.checked_mul(n).ok_or_else(range)?;
                    let slot = counts.entry(el).or_insert(0);
                    *slot = slot.checked_add(add).ok_or_else(range)?;
                }
                i = next;
            }
            b')' => {
                if depth == 0 {
                    return Err(FormulaError::Parentheses(text.to_owned()));
                }
                return Ok((counts, i));
            }
            c if c.is_ascii_uppercase() => {
                let mut j = i + 1;
                while j < b.len() && b[j].is_ascii_lowercase() {
                    j += 1;
                }
                let symbol = &text_slice(b, i, j);
                if atomic_weight(symbol).is_none() {
                    return Err(FormulaError::UnknownElement(text.to_owned(), symbol.clone()));
                }
                let (n, next) = read_number(b, j);
                let slot = counts.entry(symbol.clone()).or_insert(0);
                *slot = slot.checked_add(n.unwrap_or(1)).ok_or_else(range)?;
                i = next;
            }
            _ => return Err(FormulaError::Syntax(text.to_owned(), i)),
        }
    }
    if depth > 0 {
        return Err(FormulaError::Parentheses(text.to_owned()));
    }
    Ok((counts, i))
}

fn text_slice(b: &[u8], from: usize, to: usize) -> String {
    String::from_utf8_lossy(&b[from..to]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_and_hydrates() {
        let citrate = Formula::parse("Mg3(C6H5O7)2").unwrap();
        assert_eq!(citrate.count("Mg"), 3);
        assert_eq!(citrate.count("C"), 12);
        assert_eq!(citrate.count("O"), 14);
        assert_eq!(citrate.molar_mass().unwrap(), 451_113);

        let chloride = Formula::parse("MgCl2·6H2O").unwrap();
        assert_eq!(chloride.count("H"), 12);
        assert_eq!(chloride.molar_mass().unwrap(), 203_295);
    }

    #[test]
    fn derives_elemental_ratio() {
        let oxide = Formula::parse("MgO").unwrap();
        assert_eq!(oxide.elemental_ratio("Mg").unwrap(), Ratio::new(24_305, 40_304).unwrap());
        assert!(oxide.elemental_ratio("Zn").is_err());
    }

    #[test]
    fn rejects_bad_formulas() {
        assert!(Formula::parse("Mg(OH2").is_err());
        assert!(Formula::parse("MgOH)2").is_err());
        assert!(Formula::parse("Xx2").is_err());
        assert!(Formula::parse("mg").is_err());
        assert!(Formula::parse("").is_err());
    }
}
