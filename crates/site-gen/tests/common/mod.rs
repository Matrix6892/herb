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
    let mut store = SqliteStore::open_in_memory().unwrap();
    let (files, reference) = read_reference_dir(&root().join("data/reference")).unwrap();
    let labels = read_labels_dir(&root().join("data/fixtures/labels"), &reference).unwrap();
    let feed = feed_ingest::parse_feed(&std::fs::read(root().join("data/fixtures/feed/sample_feed.csv")).unwrap()).unwrap();
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

pub fn page(out: &BuildOutput, rel: &str) -> String {
    String::from_utf8(out.files.get(rel).unwrap_or_else(|| panic!("{rel} not built")).clone()).unwrap()
}
