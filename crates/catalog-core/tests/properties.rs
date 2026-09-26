//! Property tests of the scoring (spec §10.2).

// Test-only conversions between generated integers and floats.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use catalog_core::scoring::{Candidate, Preset, balance_scores, rank};
use catalog_core::{IherbId, UnitPrice};
use proptest::prelude::*;

/// Unit prices from 0.001 to 99.999 dollars and ratings from 1.00 to 5.00.
fn candidates() -> impl Strategy<Value = Vec<Candidate>> {
    prop::collection::vec((1u32..100_000, 100u32..=500), 1..25).prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(|(i, (milli_dollars, centi_rating))| Candidate {
                id: IherbId::new((i + 1).to_string()).unwrap(),
                unit_price: UnitPrice::from_dollars(&format!("{}.{:03}", milli_dollars / 1000, milli_dollars % 1000)).unwrap(),
                adjusted_rating: Some(f64::from(centi_rating) / 100.0),
                third_party_tested: i % 2 == 0,
            })
            .collect()
    })
}

fn with_price(c: &Candidate, milli_dollars: u32) -> Candidate {
    Candidate {
        unit_price: UnitPrice::from_dollars(&format!("{}.{:03}", milli_dollars / 1000, milli_dollars % 1000)).unwrap(),
        ..c.clone()
    }
}

const EPS: f64 = 1e-9;

proptest! {
    #[test]
    fn higher_price_never_raises_q_price(mut cands in candidates(), pick in any::<prop::sample::Index>(), bump in 1u32..50_000) {
        let i = pick.index(cands.len());
        let before = balance_scores(&cands)[&cands[i].id].q_price;
        let current = (cands[i].unit_price.cents_f64() * 10.0).round() as u32;
        cands[i] = with_price(&cands[i], current + bump);
        let after = balance_scores(&cands)[&cands[i].id].q_price;
        prop_assert!(after <= before + EPS, "q_price rose from {before} to {after}");
    }

    #[test]
    fn higher_rating_never_lowers_q_rating(mut cands in candidates(), pick in any::<prop::sample::Index>(), bump in 1u32..400) {
        let i = pick.index(cands.len());
        let before = balance_scores(&cands)[&cands[i].id].q_rating;
        let r = cands[i].adjusted_rating.unwrap();
        cands[i].adjusted_rating = Some((r + f64::from(bump) / 100.0).min(5.0));
        let after = balance_scores(&cands)[&cands[i].id].q_rating;
        prop_assert!(after + EPS >= before, "q_rating fell from {before} to {after}");
    }

    #[test]
    fn order_does_not_depend_on_input_order(cands in candidates(), seed in any::<u64>()) {
        let mut shuffled = cands.clone();
        // Deterministic Fisher–Yates from the seed.
        let mut state = seed | 1;
        for i in (1..shuffled.len()).rev() {
            state ^= state << 13; state ^= state >> 7; state ^= state << 17;
            let j = (state % (i as u64 + 1)) as usize;
            shuffled.swap(i, j);
        }
        for preset in Preset::ALL {
            prop_assert_eq!(rank(preset, &cands), rank(preset, &shuffled));
        }
    }

    #[test]
    fn scores_stay_in_range(cands in candidates()) {
        for s in balance_scores(&cands).values() {
            prop_assert!((-EPS..=100.0 + EPS).contains(&s.q_price));
            prop_assert!((-EPS..=100.0 + EPS).contains(&s.q_rating));
            prop_assert!((s.s - s.q_price.min(s.q_rating)).abs() < EPS);
        }
    }
}
