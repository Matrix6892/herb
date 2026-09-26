//! HTML snapshots of the category and product pages on the fixture (spec §10.2).
//! Review changes with `cargo insta review`, or rerun with INSTA_UPDATE=always.

mod common;

use catalog_core::RatingBranch;

/// The inlined stylesheet is the same on every page and has its own budget
/// check; leaving it out keeps the snapshots about the page.
fn without_css(html: &str) -> String {
    match (html.find("<style>"), html.find("</style>")) {
        (Some(a), Some(b)) => format!("{}<style>…</style>{}", &html[..a], &html[b + "</style>".len()..]),
        _ => html.to_owned(),
    }
}

#[test]
fn category_page_branch_b() {
    let out = common::build(RatingBranch::B);
    insta::assert_snapshot!(without_css(&common::page(&out, "c/magnesium.html")));
}

#[test]
fn category_page_branch_a() {
    let out = common::build(RatingBranch::A);
    insta::assert_snapshot!(without_css(&common::page(&out, "c/magnesium.html")));
}

#[test]
fn product_page_salt_declaration() {
    let out = common::build(RatingBranch::B);
    insta::assert_snapshot!(without_css(&common::page(
        &out,
        "pr/example-naturals-magnesium-citrate-240-tablets/900002.html"
    )));
}

#[test]
fn product_page_formula_changed_branch_a() {
    let out = common::build(RatingBranch::A);
    insta::assert_snapshot!(without_css(&common::page(
        &out,
        "pr/example-naturals-magnesium-taurate-120-capsules/900006.html"
    )));
}

#[test]
fn product_page_excluded() {
    let out = common::build(RatingBranch::B);
    insta::assert_snapshot!(without_css(&common::page(
        &out,
        "pr/placeholder-nutrition-magnesium-glycinate-90-capsules/900011.html"
    )));
}
