//! Conversion tests over every line of `data/reference/` (spec §10.2). Adding
//! a form adds its checks; nothing here names a particular form.

mod common;

use catalog_core::chem::{ATOMIC_WEIGHTS_SOURCE, atomic_weight};
use catalog_core::units::ExactMass;
use catalog_core::{Mass, Ratio};

#[test]
fn every_form_converts_consistently_with_its_formula() {
    let reference = common::reference();
    assert!(!reference.forms.is_empty(), "reference has no forms");
    let mut failures = Vec::new();
    for form in reference.forms.values() {
        let substance = &reference.substances[&form.substance];
        let ratio = form.elemental_ratio;
        if ratio.num() == 0 || !ratio.is_at_most_one() {
            failures.push(format!("{}: ratio {ratio} outside (0, 1]", form.id));
        }
        if !form.source_url.starts_with("https://") {
            failures.push(format!("{}: source_url is not https", form.id));
        }
        // Independent re-derivation with plain u128 arithmetic: 1 g of the
        // compound holds floor(10^6 × n × A / M) µg of the element.
        if let (Some(formula), Some(element)) = (&form.formula, &substance.element) {
            let n = u128::from(formula.count(element));
            let a = u128::from(atomic_weight(element).expect("known element"));
            let m = u128::from(formula.molar_mass().expect("known elements"));
            let expected_ug = 1_000_000 * n * a / m;
            let got = ExactMass::from_mass(Mass::from_ug(1_000_000))
                .times(ratio)
                .unwrap()
                .floor()
                .unwrap();
            if u128::from(got.ug()) != expected_ug {
                failures.push(format!("{}: 1 g → {got}, expected {expected_ug} µg", form.id));
            }
        } else if substance.element.is_some() {
            failures.push(format!("{}: form of an element without a formula", form.id));
        }
        // A form of an element must yield less of it than the compound weighs,
        // except the element itself.
        if ratio == Ratio::ONE
            && form
                .formula
                .as_ref()
                .is_some_and(|f| f.text() != substance.element.as_deref().unwrap_or(""))
        {
            failures.push(format!("{}: ratio of one for a compound", form.id));
        }
    }
    assert!(
        failures.is_empty(),
        "reference check failed (weights: {ATOMIC_WEIGHTS_SOURCE}):\n{}",
        failures.join("\n")
    );
}

#[test]
fn every_substance_and_category_is_sourced_and_used() {
    let reference = common::reference();
    for s in reference.substances.values() {
        assert!(s.source_url.starts_with("https://"), "{}: source", s.id);
    }
    for c in reference.categories.values() {
        assert!(reference.substances.contains_key(&c.substance), "{}: substance", c.id);
        assert!(
            reference.forms.values().any(|f| f.substance == c.substance),
            "{}: category substance has no forms",
            c.id
        );
        assert!(c.unit > Mass::ZERO);
    }
}

#[test]
fn stored_ratios_match_the_documented_percentages() {
    // The comments in forms.toml print each ratio as a percentage for the
    // reviewer; keep them honest.
    let text = common::read("data/reference/forms.toml");
    let reference = common::reference();
    for line in text.lines().filter(|l| l.starts_with("elemental_ratio")) {
        let ratio_text = line.split('"').nth(1).expect("quoted ratio");
        let pct_text = line
            .split('#')
            .nth(1)
            .expect("percentage comment")
            .trim()
            .trim_end_matches('%')
            .trim();
        let ratio = Ratio::parse(ratio_text).unwrap();
        let pct: f64 = pct_text.parse().unwrap();
        assert!((ratio.to_f64() * 100.0 - pct).abs() < 0.005 + 1e-9, "{line}");
        assert!(reference.forms.values().any(|f| f.elemental_ratio == ratio), "{line}: not loaded");
    }
}
