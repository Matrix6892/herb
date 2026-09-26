// A catalog built from the repository's fixtures: the real reference, the
// synthetic labels and the synthetic sample feed, ingested on 2026-09-26.
#![allow(dead_code)]

use std::path::PathBuf;

use catalog_core::control::BalanceFixture;
use catalog_core::{Date, RatingBranch, UtcTimestamp};
use catalog_store::files::{read_labels_dir, read_reference_dir};
use catalog_store::{CatalogWrite, SqliteStore};
use site_gen::{BuildOptions, BuildOutput};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub const DATE: &str = "2026-09-26";

pub fn fixture_store(branch: RatingBranch) -> SqliteStore {
    store_from_feed(branch, &std::fs::read(root().join("data/fixtures/feed/sample_feed.csv")).unwrap())
}

/// The fixture reference and labels with another feed, for edge cases.
pub fn store_from_feed(branch: RatingBranch, feed_bytes: &[u8]) -> SqliteStore {
    let mut store = SqliteStore::open_in_memory().unwrap();
    let (files, reference) = read_reference_dir(&root().join("data/reference")).unwrap();
    let labels = read_labels_dir(&root().join("data/fixtures/labels"), &reference).unwrap();
    let feed = feed_ingest::parse_feed(feed_bytes).unwrap();
    let date = Date::parse(DATE).unwrap();
    let fetched_at = UtcTimestamp::from_unix(date.days_since_epoch() * 86_400 + 6 * 3600 + 2 * 60);
    let day = feed_ingest::build_day(&feed, &reference, branch, date, fetched_at, "sample_feed.csv".into());
    store.record_feed_day(&day).unwrap();
    store.import_reference_and_labels(&files, &labels).unwrap();
    store
}

pub fn control() -> BalanceFixture {
    toml::from_str(&std::fs::read_to_string(root().join("data/fixtures/balance_abcdef.toml")).unwrap()).unwrap()
}

pub fn options(branch: RatingBranch) -> BuildOptions {
    BuildOptions {
        site_dir: root().join("site"),
        locale: "en".into(),
        branch,
        site_url: "https://vitrina.example".into(),
        api_base: "/api".into(),
        control: control(),
    }
}

pub fn build(branch: RatingBranch) -> BuildOutput {
    site_gen::build(&fixture_store(branch), &options(branch)).unwrap_or_else(|e| panic!("{e}"))
}

/// The sample feed cut to `ids`, each row edited by `edit` (column name to
/// new value). Rows keep their order.
pub fn edited_feed(ids: &[&str], edit: &dyn Fn(&str) -> Vec<(&'static str, String)>) -> Vec<u8> {
    let bytes = std::fs::read(root().join("data/fixtures/feed/sample_feed.csv")).unwrap();
    let mut reader = csv::Reader::from_reader(bytes.as_slice());
    let headers = reader.headers().unwrap().clone();
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(&headers).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for row in reader.records() {
        let row = row.unwrap();
        let id = row.get(0).unwrap().to_owned();
        if !ids.contains(&id.as_str()) || !seen.insert(id.clone()) {
            continue;
        }
        let mut fields: Vec<String> = row.iter().map(ToOwned::to_owned).collect();
        for (column, value) in edit(&id) {
            let i = headers
                .iter()
                .position(|h| h == column)
                .unwrap_or_else(|| panic!("no column {column}"));
            fields[i] = value;
        }
        writer.write_record(&fields).unwrap();
    }
    writer.into_inner().unwrap()
}

pub fn build_from_feed(branch: RatingBranch, feed: &[u8]) -> BuildOutput {
    site_gen::build(&store_from_feed(branch, feed), &options(branch)).unwrap_or_else(|e| panic!("{e}"))
}

pub fn page(out: &BuildOutput, rel: &str) -> String {
    String::from_utf8(out.files.get(rel).unwrap_or_else(|| panic!("{rel} not built")).clone()).unwrap()
}
