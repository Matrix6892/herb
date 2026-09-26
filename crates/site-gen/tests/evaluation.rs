//! `c/<slug>.json` (ADR 0020): one computation for the page, its script and
//! the design prototype, including the edge cases of AC20–AC23.
//!
//! The cases are also written to `design/prototypes/cases/`, where the
//! prototype and its browser test read them. Run with
//! `UPDATE_PROTOTYPE_CASES=1` to rewrite them after a deliberate change.

mod common;

use catalog_core::RatingBranch;
use serde_json::Value;
use site_gen::BuildOutput;

const CATEGORY_PAGE: &str = "c/magnesium.html";
const EVALUATION: &str = "c/magnesium.json";
const MAGNESIUM: &[&str] = &[
    "900001", "900002", "900003", "900004", "900005", "900006", "900007", "900008", "900009", "900010", "900011", "900012",
];

struct Case {
    name: &'static str,
    branch: RatingBranch,
    out: BuildOutput,
}

fn no_edit(_: &str) -> Vec<(&'static str, String)> {
    Vec::new()
}

fn cases() -> Vec<Case> {
    use RatingBranch::{A, B};
    let feed = common::edited_feed;
    vec![
        Case {
            name: "fixture-a",
            branch: A,
            out: common::build(A),
        },
        Case {
            name: "fixture-b",
            branch: B,
            out: common::build(B),
        },
        // AC20: nothing to compare. Hidden price, no price, no label.
        Case {
            name: "zero-compared",
            branch: A,
            out: common::build_from_feed(A, &feed(&["900008", "900009", "900010"], &no_edit)),
        },
        // AC20, AC17: one candidate, no #2 to be ahead of.
        Case {
            name: "one-compared",
            branch: A,
            out: common::build_from_feed(A, &feed(&["900002"], &no_edit)),
        },
        // AC22 in branch A: the default ranking needs a rating and has none.
        Case {
            name: "one-unrated",
            branch: A,
            out: common::build_from_feed(A, &feed(&["900007"], &no_edit)),
        },
        // AC21: two rated products with exactly the same price per unit.
        Case {
            name: "equal-prices",
            branch: A,
            out: common::build_from_feed(
                A,
                &feed(&["900001", "900007"], &|id| match id {
                    // 25.32 / (120 × 200 mg) = 18.99 / (90 × 200 mg)
                    "900007" => vec![
                        ("CurrentPrice", "25.32".into()),
                        ("Rating", "4.5".into()),
                        ("ReviewCount", "3400".into()),
                    ],
                    _ => Vec::new(),
                }),
            ),
        },
        // AC21: two products with the same average and review count.
        Case {
            name: "equal-ratings",
            branch: A,
            out: common::build_from_feed(
                A,
                &feed(&["900001", "900002"], &|id| match id {
                    "900002" => vec![("Rating", "4.7".into()), ("ReviewCount", "12000".into())],
                    _ => Vec::new(),
                }),
            ),
        },
        // AC22: branch B with no testing mark on any compared label.
        Case {
            name: "b-no-marks",
            branch: B,
            out: common::build_from_feed(
                B,
                &feed(
                    &MAGNESIUM
                        .iter()
                        .copied()
                        .filter(|id| !["900002", "900004"].contains(id))
                        .collect::<Vec<_>>(),
                    &no_edit,
                ),
            ),
        },
    ]
}

fn json(out: &BuildOutput) -> Value {
    serde_json::from_slice(out.files.get(EVALUATION).expect("evaluation file built")).unwrap()
}

fn number(v: &Value, path: &str) -> f64 {
    v.pointer(path)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("{path} is not a number in {v}"))
}

/// `data-o-<key>` of the category list, split into ids.
fn html_order(html: &str, key: &str) -> Vec<String> {
    let attr = format!("data-o-{key}=\"");
    let Some(i) = html.find(&attr) else { return Vec::new() };
    let rest = &html[i + attr.len()..];
    rest[..rest.find('"').unwrap()].split_whitespace().map(ToOwned::to_owned).collect()
}

#[test]
fn every_case_is_finite_consistent_and_explained() {
    for case in cases() {
        let name = case.name;
        for (rel, bytes) in &case.out.files {
            let text = String::from_utf8_lossy(bytes);
            for bad in ["NaN", "Infinity", "undefined"] {
                assert!(!text.contains(bad), "{name}: {rel} contains {bad}");
            }
        }
        let v = json(&case.out);
        assert_eq!(v["schema"], "vitrina.category-evaluation/1", "{name}");
        assert!(v["rules"].as_str().is_some_and(|s| !s.is_empty()), "{name}");
        let html = common::page(&case.out, CATEGORY_PAGE);
        let compared = v["category"]["compared"].as_u64().unwrap();

        for preset in v["presets"].as_array().unwrap() {
            let key = preset["key"].as_str().unwrap();
            let order: Vec<String> = preset["order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_owned())
                .collect();
            // AC23: the page and the file carry one ranking.
            if compared > 0 {
                assert_eq!(order, html_order(&html, key), "{name}: {key} differs between page and file");
            }
            // AC22: an empty ranking says why; a full one does not.
            assert_eq!(order.is_empty(), preset.get("empty").is_some(), "{name}: {key}");
            if let Some(why) = preset["empty"].as_str()
                && compared > 0
            {
                // askama escapes the apostrophe of "today's".
                let escaped = why.replace('\'', "&#39;");
                assert!(html.contains(&escaped), "{name}: the page does not explain the empty {key}");
            }
            // AC17: no lead without a runner-up.
            assert_eq!(order.len() >= 2, preset.get("lead").is_some(), "{name}: {key} lead");
            if let Some(lead) = preset.get("lead") {
                assert_eq!(lead["runner_up"], order[1].as_str(), "{name}: {key}");
                assert!(lead["value"].as_f64().is_some(), "{name}: {key} lead is not finite");
            }
        }

        for p in v["products"].as_array().unwrap() {
            let id = p["id"].as_str().unwrap();
            if p["status"] == "compared" {
                number(p, "/unit_price/value");
                number(p, "/vs_median");
                assert!(p["positions"]["cheapest_per_unit"].as_u64().is_some(), "{name}: {id}");
            } else {
                // И7: no price, no made-up number.
                assert!(p.get("unit_price").is_none() && p.get("vs_median").is_none(), "{name}: {id}");
                assert!(p["reason"].as_str().is_some_and(|r| !r.is_empty()), "{name}: {id}");
            }
            if let Some(r) = p.get("rating") {
                // The average is shown as written in the feed, one decimal.
                let avg = number(p, "/rating/avg");
                assert!((avg * 10.0 - (avg * 10.0).round()).abs() < 1e-9, "{name}: {id} average {avg}");
                assert_eq!(r["avg_text"].as_str().unwrap().len(), 3, "{name}: {id}");
            }
            if let Some(b) = p.get("balance") {
                for field in ["/balance/q_price", "/balance/q_rating", "/balance/s"] {
                    let x = number(p, field);
                    assert!((0.0..=100.0).contains(&x), "{name}: {id} {field} = {x}");
                }
                let norm = &v["balance_norm"];
                // A threshold exists exactly when that side spreads.
                assert_eq!(b["price_at_cents"].is_number(), norm["price_spreads"] == true, "{name}: {id}");
                assert_eq!(b["rating_at"].is_number(), norm["rating_spreads"] == true, "{name}: {id}");
            }
        }

        if compared == 0 {
            assert!(
                html.contains("No product in this category has a verified label and a price today."),
                "{name}"
            );
        }
        if case.branch == RatingBranch::B {
            let text = String::from_utf8(case.out.files[EVALUATION].clone()).unwrap();
            for key in ["\"rating\"", "\"balance\"", "adjusted", "balance_norm"] {
                assert!(!text.contains(key), "{name}: branch B file carries {key}");
            }
        }
    }
}

#[test]
fn edge_cases_compute_as_expected() {
    let by_name = |n: &str| cases().into_iter().find(|c| c.name == n).unwrap();

    let v = json(&by_name("equal-prices").out);
    assert_eq!(v["balance_norm"]["price_spreads"], false);
    for p in v["products"].as_array().unwrap().iter().filter(|p| p.get("balance").is_some()) {
        assert!((number(p, "/balance/q_price") - 100.0).abs() < 1e-9);
    }

    let v = json(&by_name("equal-ratings").out);
    assert_eq!(v["balance_norm"]["rating_spreads"], false);
    for p in v["products"].as_array().unwrap().iter().filter(|p| p.get("balance").is_some()) {
        assert!((number(p, "/balance/q_rating") - 100.0).abs() < 1e-9);
    }

    let v = json(&by_name("one-unrated").out);
    let preset = |k: &str| v["presets"].as_array().unwrap().iter().find(|p| p["key"] == k).unwrap().clone();
    assert_eq!(preset("balance")["order"].as_array().unwrap().len(), 0);
    assert_eq!(preset("cheapest_per_unit")["order"].as_array().unwrap().len(), 1);
    let html = common::page(&by_name("one-unrated").out, CATEGORY_PAGE);
    // The default ranking is empty: its reason is on the page without script.
    assert!(html.contains("class=\"pnote empty\" data-empty-for=\"balance\">"));

    let v = json(&by_name("b-no-marks").out);
    let verified = v["presets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "cheapest_verified")
        .unwrap();
    assert!(verified["order"].as_array().unwrap().is_empty());
    assert!(verified["empty"].as_str().unwrap().contains("testing mark"));
}

#[test]
fn prototype_cases_match_the_generator() {
    let dir = common::root().join("design/prototypes/cases");
    let update = std::env::var_os("UPDATE_PROTOTYPE_CASES").is_some();
    for case in cases() {
        let pretty = serde_json::to_string_pretty(&json(&case.out)).unwrap() + "\n";
        let path = dir.join(format!("{}.json", case.name));
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &pretty).unwrap();
            continue;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == pretty,
            "{} is out of date; run UPDATE_PROTOTYPE_CASES=1 cargo test -p site-gen --test evaluation",
            path.display()
        );
    }
}
