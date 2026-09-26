use std::path::PathBuf;

use catalog_core::schema::{CategoriesFile, FormsFile, LabelFile, SubstancesFile};
use catalog_core::{CategoryId, Date, IherbId, Money, ProductStatus, Reference, Slug, UtcTimestamp};
use catalog_store::{
    CatalogRead, CatalogWrite, ClickEvent, EventSink, FeedDay, FeedItem, FeedProduct, LATEST_VERSION, LabelSet, MIGRATIONS, PriceSnapshot,
    ReferenceFiles, SqliteStore,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse<T: serde::de::DeserializeOwned>(rel: &str) -> T {
    toml::from_str(&std::fs::read_to_string(root().join(rel)).unwrap()).unwrap()
}

fn files() -> ReferenceFiles {
    ReferenceFiles {
        substances: parse::<SubstancesFile>("data/reference/substances.toml"),
        forms: parse::<FormsFile>("data/reference/forms.toml"),
        categories: parse::<CategoriesFile>("data/reference/categories.toml"),
    }
}

fn reference(f: &ReferenceFiles) -> Reference {
    Reference::from_files(f.substances.clone(), f.forms.clone(), f.categories.clone()).unwrap()
}

const LABEL: &str = r#"
schema = 1
iherb_id = "1001"
[[label]]
effective_from = "2025-01-01"
serving_size = 2
servings_per_container = 60
verified_by = "tester"
verified_at = "2025-01-02"
[[label.line]]
form = "magnesium_citrate_anhydrous"
amount = "500 mg"
declared_as = "salt"
confidence = "verified"
[[label]]
effective_from = "2026-03-01"
serving_size = 2
servings_per_container = 60
verified_by = "tester"
verified_at = "2026-03-02"
third_party_test = "USP Verified"
[[label.line]]
form = "magnesium_bisglycinate_anhydrous"
amount = "200 mg"
declared_as = "elemental"
confidence = "verified"
"#;

fn item(id: &str, day: Date, cents: Option<i64>) -> FeedItem {
    let iherb_id = IherbId::new(id).unwrap();
    FeedItem {
        product: FeedProduct {
            iherb_id: iherb_id.clone(),
            slug: Slug::new(format!("brand-magnesium-{id}")).unwrap(),
            brand: "Brand".into(),
            title: format!("Magnesium {id}"),
            category: CategoryId::new("magnesium").unwrap(),
            tracking_url: Some(format!("https://tracking.example/c/1?u={id}")),
        },
        price: PriceSnapshot {
            product: iherb_id,
            date: day,
            price: cents.map(Money::from_cents),
            in_stock: true,
            hidden_until_cart: false,
            feed_row_hash: format!("hash-{id}"),
        },
        rating: None,
    }
}

fn feed_day(day: Date, items: Vec<FeedItem>) -> FeedDay {
    FeedDay {
        date: day,
        fetched_at: UtcTimestamp::from_unix(day.days_since_epoch() * 86_400 + 6 * 3600),
        source: "feed.example".into(),
        feed_sha256: "00".into(),
        rows_total: 10,
        rows_rejected: 1,
        categories: vec![CategoryId::new("magnesium").unwrap()],
        items,
    }
}

#[test]
fn migrations_are_numbered_forward_only_and_match_the_directory() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut on_disk: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let listed: Vec<String> = MIGRATIONS.iter().map(|(_, n, _)| (*n).to_owned()).collect();
    assert_eq!(on_disk, listed, "every file in migrations/ is listed, in order");
    for (i, (v, name, _)) in MIGRATIONS.iter().enumerate() {
        assert_eq!(*v as usize, i + 1);
        assert!(name.starts_with(&format!("{v:03}_")), "{name}");
        assert!(!name.contains("down"), "no down migrations");
    }
    assert_eq!(LATEST_VERSION as usize, MIGRATIONS.len());
}

#[test]
fn reference_and_labels_round_trip() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    let f = files();
    let r = reference(&f);
    let labels = toml::from_str::<LabelFile>(LABEL).unwrap().into_labels(&r).unwrap();
    let report = store
        .import_reference_and_labels(
            &f,
            &[LabelSet {
                product: IherbId::new("1001").unwrap(),
                label_ref: "data/labels/1001.toml".into(),
                labels: labels.clone(),
            }],
        )
        .unwrap();
    assert_eq!(report.label_versions, 2);
    assert_eq!(report.labels_without_product, 1);
    assert_eq!(store.reference().unwrap(), r);
    assert_eq!(store.labels(&IherbId::new("1001").unwrap()).unwrap(), labels);
    // Importing twice replaces, not duplicates.
    store.import_reference_and_labels(&f, &[]).unwrap();
    assert!(store.labels(&IherbId::new("1001").unwrap()).unwrap().is_empty());
}

#[test]
fn feed_days_create_products_queue_and_delist() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    let f = files();
    let r = reference(&f);
    let labels = toml::from_str::<LabelFile>(LABEL).unwrap().into_labels(&r).unwrap();
    store
        .import_reference_and_labels(
            &f,
            &[LabelSet {
                product: IherbId::new("1001").unwrap(),
                label_ref: "data/labels/1001.toml".into(),
                labels,
            }],
        )
        .unwrap();

    let d1 = Date::parse("2026-09-25").unwrap();
    let run = store
        .record_feed_day(&feed_day(d1, vec![item("1001", d1, Some(1500)), item("1002", d1, None)]))
        .unwrap();
    assert_eq!(run.new_products, 2);
    let cat = CategoryId::new("magnesium").unwrap();
    let products = store.products_in_category(&cat).unwrap();
    let status = |id: &str| products.iter().find(|p| p.iherb_id.as_str() == id).unwrap().status;
    assert_eq!(status("1001"), ProductStatus::Active);
    assert_eq!(status("1002"), ProductStatus::PendingLabel);
    assert_eq!(store.pending_labels().unwrap(), vec![(IherbId::new("1002").unwrap(), d1)]);
    // A missing price stays missing.
    assert_eq!(store.price_on(&IherbId::new("1002").unwrap(), d1).unwrap().unwrap().price, None);

    let d2 = Date::parse("2026-09-26").unwrap();
    store.record_feed_day(&feed_day(d2, vec![item("1001", d2, Some(1400))])).unwrap();
    let products = store.products_in_category(&cat).unwrap();
    let status = |id: &str| products.iter().find(|p| p.iherb_id.as_str() == id).unwrap().status;
    assert_eq!(status("1002"), ProductStatus::Delisted);
    assert_eq!(store.latest_ingest().unwrap().unwrap().date, d2);
    // History is kept.
    assert_eq!(
        store.price_on(&IherbId::new("1001").unwrap(), d1).unwrap().unwrap().price,
        Some(Money::from_cents(1500))
    );
    assert_eq!(
        store.price_on(&IherbId::new("1001").unwrap(), d2).unwrap().unwrap().price,
        Some(Money::from_cents(1400))
    );
}

#[test]
fn snapshots_are_complete_read_only_copies() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("vitrina.db")).unwrap();
    let f = files();
    store.import_reference_and_labels(&f, &[]).unwrap();
    let d = Date::parse("2026-09-26").unwrap();
    store.record_feed_day(&feed_day(d, vec![item("7", d, Some(999))])).unwrap();
    store
        .insert_click(&ClickEvent {
            product: IherbId::new("7").unwrap(),
            preset: "balance".into(),
            page: "/c/magnesium".into(),
            ts: UtcTimestamp::from_unix(1),
            session_hash: "abc".into(),
        })
        .unwrap();
    assert_eq!(store.count_clicks().unwrap(), 1);

    let snap = dir.path().join("vitrina-2026-09-26.db");
    store.snapshot_to(&snap).unwrap();
    assert!(!dir.path().join("vitrina-2026-09-26.pending-snap").exists());
    let ro = SqliteStore::open_snapshot(&snap).unwrap();
    assert_eq!(ro.latest_ingest().unwrap().unwrap().date, d);
    assert_eq!(ro.reference().unwrap(), reference(&f));
    assert!(
        ro.insert_click(&ClickEvent {
            product: IherbId::new("7").unwrap(),
            preset: "balance".into(),
            page: "/".into(),
            ts: UtcTimestamp::from_unix(2),
            session_hash: "x".into(),
        })
        .is_err(),
        "snapshots are read-only"
    );
}

#[test]
fn fixture_label_files_load() {
    let (_, r) = catalog_store::files::read_reference_dir(&root().join("data/reference")).unwrap();
    let sets = catalog_store::files::read_labels_dir(&root().join("data/fixtures/labels"), &r).unwrap();
    assert_eq!(sets.len(), 11);
    assert!(sets.windows(2).all(|w| w[0].product < w[1].product));
    let taurate = sets.iter().find(|s| s.product.as_str() == "900006").unwrap();
    assert_eq!(taurate.labels.len(), 2);
    assert!(taurate.label_ref.ends_with("data/fixtures/labels/900006.toml"));
}
