//! Spec §5, §10.2: the control numbers on /how and in the code are the same.

mod common;

use catalog_core::RatingBranch;

fn fmt1(v: f64) -> String {
    format!("{v:.1}")
}

#[test]
fn how_page_matches_code() {
    let fixture = common::control();
    let computed = fixture.check().expect("code matches the fixture");
    let out = common::build(RatingBranch::A);
    let html = common::page(&out, "how.html");
    let table = html
        .split("<table id=\"control\">")
        .nth(1)
        .expect("control table")
        .split("</table>")
        .next()
        .unwrap();

    for (row, expected) in computed.rows.iter().zip(computed.order.iter()) {
        assert_eq!(&row.name, expected);
        let fx = fixture.product.iter().find(|p| p.name == row.name).unwrap();
        // The page shows the code's numbers, and they equal the fixture's
        // expected values within its tolerance.
        let cells = format!(
            "<td>{}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>",
            row.name,
            row.p_unit.cents_f64() / 100.0,
            row.r_b,
            fmt1(row.score.q_price),
            fmt1(row.score.q_rating),
            fmt1(row.score.s)
        );
        assert!(table.contains(&cells), "row {} not on the page as {cells}", row.name);
        for (shown, want) in [
            (row.score.q_price, fx.q_price),
            (row.score.q_rating, fx.q_rating),
            (row.score.s, fx.s),
        ] {
            let on_page: f64 = fmt1(shown).parse().unwrap();
            assert!(
                (on_page - want).abs() <= fixture.tolerance,
                "{}: page {on_page}, spec {want}",
                row.name
            );
        }
    }
    assert!(html.contains(&format!("Order: {}.", fixture.expected_order.join(", "))));
}

#[test]
fn how_page_worked_examples_match_code() {
    for branch in [RatingBranch::A, RatingBranch::B] {
        let out = common::build(branch);
        let html = common::page(&out, "how.html");
        // 500 mg of anhydrous magnesium citrate × 72915/451113, rounded down
        // to a microgram once: 80 816 µg.
        assert!(html.contains("Example: 500 mg of magnesium citrate (trimagnesium dicitrate, anhydrous) × 24305/150371 (16.16%) = 80.816 mg of magnesium, rounded down to a microgram."), "{branch:?}");
        assert!(html.contains("P_unit = price ÷ (servings per container × dose per serving × k_form)"));
        assert_eq!(html.contains("<table id=\"control\">"), branch == RatingBranch::A);
    }
}
