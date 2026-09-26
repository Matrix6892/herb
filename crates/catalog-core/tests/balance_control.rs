//! Control set A–F from spec §5.5.

mod common;

use catalog_core::control::BalanceFixture;

#[test]
fn balance_matches_the_control_set() {
    let fixture: BalanceFixture = common::parse("data/fixtures/balance_abcdef.toml");
    let result = fixture.check().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(result.order, ["B", "C", "E", "D", "A", "F"]);
}

#[test]
fn the_check_catches_a_wrong_expectation() {
    let mut fixture: BalanceFixture = common::parse("data/fixtures/balance_abcdef.toml");
    fixture.product[1].q_price += 0.1;
    assert!(fixture.check().is_err());
    let mut fixture: BalanceFixture = common::parse("data/fixtures/balance_abcdef.toml");
    fixture.expected_order.swap(0, 1);
    assert!(fixture.check().is_err());
}
