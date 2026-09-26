//! Elemental dose and unit price from labels (spec §5.1, §5.2).

mod common;

use catalog_core::schema::LabelFile;
use catalog_core::{Confidence, Declared, DoseError, Label, Mass, SubstanceId, elemental_per_serving};

fn label(toml_text: &str) -> Label {
    let file: LabelFile = toml::from_str(toml_text).unwrap();
    file.into_labels(&common::reference()).unwrap().pop().unwrap()
}

const HEADER: &str = r#"
schema = 1
iherb_id = "1001"
[[label]]
effective_from = "2026-09-01"
serving_size = 2
servings_per_container = 60
verified_by = "test"
verified_at = "2026-09-01"
"#;

fn mg() -> SubstanceId {
    SubstanceId::new("magnesium").unwrap()
}

#[test]
fn salt_mass_is_multiplied_by_the_elemental_ratio() {
    // "Magnesium citrate 500 mg" holds far less magnesium (the spec's example).
    let l = label(&format!(
        "{HEADER}[[label.line]]\nform = \"magnesium_citrate_anhydrous\"\namount = \"500 mg\"\ndeclared_as = \"salt\"\nconfidence = \"verified\"\n"
    ));
    let dose = elemental_per_serving(&l, &mg(), &common::reference()).unwrap();
    // 500 000 µg × 72915 / 451113 = 80 816.78… µg, rounded down once.
    assert_eq!(dose.per_serving, Mass::from_ug(80_816));
    assert_eq!(dose.lines[0].declared, Declared::SaltMass(Mass::from_mg(500).unwrap()));
}

#[test]
fn elemental_mass_is_taken_as_printed_and_lines_add_up_before_rounding() {
    let l = label(&format!(
        "{HEADER}[[label.line]]\nform = \"magnesium_bisglycinate_anhydrous\"\namount = \"100 mg\"\ndeclared_as = \"elemental\"\nconfidence = \"verified\"\n\
         [[label.line]]\nform = \"magnesium_oxide\"\namount = \"1 mg\"\ndeclared_as = \"salt\"\nconfidence = \"verified\"\n"
    ));
    let dose = elemental_per_serving(&l, &mg(), &common::reference()).unwrap();
    // 100 000 + 1 000 × 24305/40304 = 100 603.04… µg
    assert_eq!(dose.per_serving, Mass::from_ug(100_603));
}

#[test]
fn unknown_or_unverified_lines_exclude_the_product() {
    let base = format!("{HEADER}[[label.line]]\nform = \"magnesium_oxide\"\namount = \"100 mg\"\ndeclared_as = \"elemental\"\n");
    let unknown = label(&format!("{base}confidence = \"unknown\"\n"));
    assert!(matches!(
        elemental_per_serving(&unknown, &mg(), &common::reference()),
        Err(DoseError::UnknownLine { line: 1, .. })
    ));
    let recognized = label(&format!("{base}confidence = \"recognized\"\n"));
    assert_eq!(recognized.lines[0].confidence, Confidence::Recognized);
    assert!(matches!(
        elemental_per_serving(&recognized, &mg(), &common::reference()),
        Err(DoseError::NotVerified { .. })
    ));
}

#[test]
fn iu_without_a_conversion_is_refused() {
    let l = label(&format!(
        "{HEADER}[[label.line]]\nform = \"magnesium_oxide\"\namount = \"100 IU\"\ndeclared_as = \"iu\"\nconfidence = \"verified\"\n"
    ));
    assert!(matches!(
        elemental_per_serving(&l, &mg(), &common::reference()),
        Err(DoseError::NoIuConversion { .. })
    ));
}

#[test]
fn label_files_are_validated() {
    let reference = common::reference();
    let bad_form = format!(
        "{HEADER}[[label.line]]\nform = \"magnesium_unobtainium\"\namount = \"1 mg\"\ndeclared_as = \"salt\"\nconfidence = \"verified\"\n"
    );
    let file: LabelFile = toml::from_str(&bad_form).unwrap();
    assert!(file.into_labels(&reference).is_err());

    let no_reviewer = HEADER.replace("verified_by = \"test\"", "verified_by = \" \"");
    let text = format!(
        "{no_reviewer}[[label.line]]\nform = \"magnesium_oxide\"\namount = \"1 mg\"\ndeclared_as = \"salt\"\nconfidence = \"verified\"\n"
    );
    let file: LabelFile = toml::from_str(&text).unwrap();
    assert!(file.into_labels(&reference).is_err());

    let typo =
        format!("{HEADER}[[label.line]]\nform = \"magnesium_oxide\"\namount = \"1 mg\"\ndeclared = \"salt\"\nconfidence = \"verified\"\n");
    assert!(toml::from_str::<LabelFile>(&typo).is_err(), "unknown fields are rejected");
}
