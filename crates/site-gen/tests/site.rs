mod common;

use catalog_core::RatingBranch;
use site_gen::verify::{Budgets, verify};

#[test]
fn builds_both_branches_and_passes_verify() {
    for branch in [RatingBranch::A, RatingBranch::B] {
        let out = common::build(branch);
        let dir = tempfile::tempdir().unwrap();
        out.write_to(dir.path()).unwrap();
        let failures = verify(
            dir.path(),
            None,
            &common::control(),
            &common::root().join("site/static/site.css"),
            Budgets::default(),
        );
        assert!(failures.is_empty(), "{branch:?}: {failures:#?}");
        assert_eq!(out.manifest.date, common::DATE);
        assert_eq!(out.manifest.categories[0].products, 12);
        assert_eq!(out.manifest.products_with_unit_price, 8);
        assert_eq!(out.manifest.categories[0].pending_label, 1);
    }
}

#[test]
fn build_is_reproducible() {
    let a = common::build(RatingBranch::A);
    let b = common::build(RatingBranch::A);
    assert_eq!(a.manifest, b.manifest);
    assert_eq!(a.files, b.files);
}

#[test]
fn every_page_expected_exists() {
    let out = common::build(RatingBranch::B);
    for f in [
        "index.html",
        "c/magnesium.html",
        "how.html",
        "disclosure.html",
        "privacy.html",
        "404.html",
        "robots.txt",
        "static/app.js",
    ] {
        assert!(out.files.contains_key(f), "{f}");
    }
    let products = out.files.keys().filter(|k| k.starts_with("pr/")).count();
    assert_eq!(products, 12);
}

#[test]
fn pending_products_get_the_queue_page_with_noindex() {
    let out = common::build(RatingBranch::B);
    let html = common::page(&out, "pr/example-naturals-magnesium-aspartate-100-capsules/900010.html");
    assert!(html.contains("added to the queue"));
    assert!(html.contains(r#"<meta name="robots" content="noindex">"#));
    let ranked = common::page(&out, "pr/example-naturals-magnesium-citrate-240-tablets/900002.html");
    assert!(!ranked.contains("noindex"));
}

#[test]
fn unknown_is_shown_as_unknown() {
    let out = common::build(RatingBranch::B);
    let category = common::page(&out, "c/magnesium.html");
    // No price, cart-only price and an unreadable label line: each named, none zeroed.
    assert!(category.contains("No price in today&#39;s network feed."), "no-price reason");
    assert!(category.contains("only in the cart"));
    assert!(category.contains("could not be read with certainty"));
    assert!(!category.contains("$0.000"));
}

#[test]
fn branch_b_shows_no_ratings_anywhere() {
    let out = common::build(RatingBranch::B);
    for (path, bytes) in &out.files {
        let html = String::from_utf8_lossy(bytes);
        assert!(!html.contains("reviews"), "{path} mentions reviews in branch B");
        assert!(!html.contains("data-preset=\"balance\""), "{path}");
    }
    let category = common::page(&out, "c/magnesium.html");
    assert!(category.contains("data-preset=\"cheapest_verified\""));
}

#[test]
fn buy_buttons_use_only_tracking_links() {
    let out = common::build(RatingBranch::A);
    for (path, bytes) in &out.files {
        let html = String::from_utf8_lossy(bytes);
        for chunk in html.split("class=\"btn\" href=\"").skip(1) {
            let href = chunk.split('"').next().unwrap();
            assert!(href.starts_with("https://tracking.example/"), "{path}: {href}");
        }
    }
}
