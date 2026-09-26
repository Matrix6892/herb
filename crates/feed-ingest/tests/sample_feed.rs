use std::path::PathBuf;

use catalog_core::schema::{CategoriesFile, FormsFile, SubstancesFile};
use catalog_core::{Date, Money, RatingBranch, Reference, UtcTimestamp};
use feed_ingest::{build_day, parse_feed};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reference() -> Reference {
    let read = |p: &str| std::fs::read_to_string(root().join(p)).unwrap();
    Reference::from_files(
        toml::from_str::<SubstancesFile>(&read("data/reference/substances.toml")).unwrap(),
        toml::from_str::<FormsFile>(&read("data/reference/forms.toml")).unwrap(),
        toml::from_str::<CategoriesFile>(&read("data/reference/categories.toml")).unwrap(),
    )
    .unwrap()
}

fn sample() -> Vec<u8> {
    std::fs::read(root().join("data/fixtures/feed/sample_feed.csv")).unwrap()
}

#[test]
fn sample_feed_parses_with_expected_rejections() {
    let feed = parse_feed(&sample()).unwrap();
    assert_eq!(feed.rows_total, 17);
    let reasons: Vec<String> = feed.rejected.iter().map(|r| format!("{}: {}", r.row, r.reason)).collect();
    assert_eq!(feed.rejected.len(), 3, "{reasons:#?}");
    assert!(reasons[0].contains("not USD"), "{reasons:?}");
    assert!(reasons[1].contains("duplicate id 900001"), "{reasons:?}");
    assert!(reasons[2].contains("availability"), "{reasons:?}");
    assert_eq!(feed.rows_without_price, 1);

    let first = &feed.rows[0];
    assert_eq!(first.iherb_id.as_str(), "900001");
    assert_eq!(first.slug.as_str(), "fixture-labs-magnesium-bisglycinate-200-mg-180-capsules");
    assert_eq!(first.price, Some(Money::from_cents(1899)));
    assert!(first.tracking_url.as_deref().unwrap().starts_with("https://tracking.example/"));
}

#[test]
fn only_category_rows_become_observations() {
    let feed = parse_feed(&sample()).unwrap();
    let date = Date::parse("2026-09-26").unwrap();
    let day = build_day(
        &feed,
        &reference(),
        RatingBranch::B,
        date,
        UtcTimestamp::from_unix(0),
        "sample_feed.csv".into(),
    );
    let ids: Vec<&str> = day.items.iter().map(|i| i.product.iherb_id.as_str()).collect();
    assert_eq!(ids.len(), 12, "{ids:?}");
    assert!(!ids.contains(&"900101"), "vitamin D is outside the category");
    assert!(!ids.contains(&"900102"), "combination products need their own category");
    assert!(day.items.iter().all(|i| i.rating.is_none()), "branch B stores no ratings");

    let day_a = build_day(&feed, &reference(), RatingBranch::A, date, UtcTimestamp::from_unix(0), "x".into());
    assert_eq!(day_a.items.iter().filter(|i| i.rating.is_some()).count(), 11);
}

#[test]
fn gzip_and_tabs_are_read_the_same() {
    use std::io::Write;
    let csv = sample();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&csv).unwrap();
    let from_gz = parse_feed(&gz.finish().unwrap()).unwrap();
    let plain = parse_feed(&csv).unwrap();
    assert_eq!(from_gz.rows, plain.rows);

    let mut rdr = csv::Reader::from_reader(csv.as_slice());
    let mut w = csv::WriterBuilder::new().delimiter(b'\t').from_writer(Vec::new());
    w.write_record(rdr.headers().unwrap()).unwrap();
    for r in rdr.records() {
        w.write_record(&r.unwrap()).unwrap();
    }
    let tsv = parse_feed(&w.into_inner().unwrap()).unwrap();
    assert_eq!(tsv.rows.len(), plain.rows.len());
}

#[test]
fn a_feed_without_required_columns_is_an_error() {
    let err = parse_feed(b"Name,Price\nA,1.00\n").unwrap_err();
    assert!(err.to_string().contains("missing required columns"), "{err}");
    assert!(parse_feed(b"").is_err());
}

#[test]
fn fetch_refuses_the_store() {
    let err = feed_ingest::fetch("https://www.iherb.com/feed.csv").unwrap_err();
    assert!(err.to_string().contains("never reads iherb.com"), "{err}");
}
