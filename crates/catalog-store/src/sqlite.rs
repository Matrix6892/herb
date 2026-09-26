use std::path::Path;
use std::time::Duration;

use catalog_core::schema::{CategoriesFile, CategoryEntry, FormEntry, FormsFile, SubstanceEntry, SubstancesFile};
use catalog_core::{
    CategoryId, Confidence, Date, Declared, FormId, IherbId, Iu, Label, LabelLine, Mass, Money, Product, ProductStatus, Reference, Slug,
    UtcTimestamp,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};

use crate::migrate::{LATEST_VERSION, migrate, user_version};
use crate::records::{ClickEvent, ErrorReport, FeedDay, ImportReport, IngestRun, LabelSet, PriceSnapshot, RatingSnapshot, ReferenceFiles};
use crate::{CatalogRead, CatalogWrite, EventSink, StoreError};

pub struct SqliteStore {
    conn: Connection,
}

fn corrupt(table: &'static str, detail: impl ToString) -> StoreError {
    StoreError::Corrupt {
        table,
        detail: detail.to_string(),
    }
}

fn to_i64(v: u64) -> Result<i64, StoreError> {
    i64::try_from(v).map_err(|_| corrupt("value", v))
}

fn json_vec(v: &[String]) -> String {
    serde_json::to_string(v).expect("strings serialise")
}

fn parse_json_vec(table: &'static str, s: &str) -> Result<Vec<String>, StoreError> {
    serde_json::from_str(s).map_err(|e| corrupt(table, e))
}

fn date(table: &'static str, s: &str) -> Result<Date, StoreError> {
    Date::parse(s).map_err(|e| corrupt(table, e))
}

impl SqliteStore {
    /// Opens (creating if needed) the working database and applies pending
    /// migrations.
    pub fn open(path: &Path) -> Result<SqliteStore, StoreError> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(Duration::from_secs(10))?;
        migrate(&mut conn)?;
        Ok(SqliteStore { conn })
    }

    pub fn open_in_memory() -> Result<SqliteStore, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(SqliteStore { conn })
    }

    /// Opens a dated snapshot read-only. Snapshots never change, so SQLite is
    /// told the file is immutable and needs no lock or WAL files.
    pub fn open_snapshot(path: &Path) -> Result<SqliteStore, StoreError> {
        let mut uri = String::from("file:");
        for c in path.to_string_lossy().chars() {
            match c {
                '?' => uri.push_str("%3f"),
                '#' => uri.push_str("%23"),
                '%' => uri.push_str("%25"),
                c => uri.push(c),
            }
        }
        uri.push_str("?immutable=1");
        let conn = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)?;
        let found = user_version(&conn)?;
        if found != LATEST_VERSION {
            return Err(StoreError::SnapshotVersion {
                found,
                supported: LATEST_VERSION,
            });
        }
        Ok(SqliteStore { conn })
    }

    /// Writes a consistent copy of the database to `dest` (via a temporary
    /// file and a rename, so `dest` is either complete or absent).
    pub fn snapshot_to(&self, dest: &Path) -> Result<(), StoreError> {
        let tmp = dest.with_extension("pending-snap");
        if tmp.exists() {
            std::fs::remove_file(&tmp)?;
        }
        self.conn.execute("VACUUM INTO ?1", params![tmp.to_string_lossy()])?;
        // The copy is read through `immutable=1`; rollback journal mode keeps
        // it a single self-contained file.
        {
            let copy = Connection::open(&tmp)?;
            copy.pragma_update(None, "journal_mode", "DELETE")?;
        }
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn refresh_status(tx: &Transaction<'_>) -> Result<(), StoreError> {
        tx.execute_batch(
            "UPDATE product SET status = CASE
                 WHEN last_seen < (SELECT max(date) FROM ingest_run) THEN 'delisted'
                 WHEN EXISTS (SELECT 1 FROM label WHERE label.product_id = product.iherb_id) THEN 'active'
                 ELSE 'pending_label'
             END;
             DELETE FROM pending_labels WHERE product_id IN (SELECT product_id FROM label);",
        )?;
        Ok(())
    }

    fn read_reference_files(&self) -> Result<ReferenceFiles, StoreError> {
        let mut substances = Vec::new();
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, synonyms_json, default_unit, element, ug_per_iu, source_url FROM substance ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            substances.push(SubstanceEntry {
                id: r.get(0)?,
                name: r.get(1)?,
                synonyms: parse_json_vec("substance", &r.get::<_, String>(2)?)?,
                default_unit: r.get(3)?,
                element: r.get(4)?,
                ug_per_iu: r.get(5)?,
                source_url: r.get(6)?,
            });
        }
        let mut forms = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, substance_id, name, synonyms_json, elemental_ratio, formula, bioavailability_k, bioavailability_note, ug_per_iu, source_url
             FROM form ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            forms.push(FormEntry {
                id: r.get(0)?,
                substance: r.get(1)?,
                name: r.get(2)?,
                synonyms: parse_json_vec("form", &r.get::<_, String>(3)?)?,
                elemental_ratio: r.get(4)?,
                formula: r.get(5)?,
                bioavailability_k: r.get(6)?,
                bioavailability_note: r.get(7)?,
                ug_per_iu: r.get(8)?,
                source_url: r.get(9)?,
            });
        }
        let mut categories = Vec::new();
        let mut stmt = self
            .conn
            .prepare("SELECT id, slug, name, substance_id, unit, unit_label, feed_match_json FROM category ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            categories.push(CategoryEntry {
                id: r.get(0)?,
                slug: r.get(1)?,
                name: r.get(2)?,
                substance: r.get(3)?,
                unit: r.get(4)?,
                unit_label: r.get(5)?,
                feed_match: parse_json_vec("category", &r.get::<_, String>(6)?)?,
            });
        }
        let v = catalog_core::reference::SCHEMA_VERSION;
        Ok(ReferenceFiles {
            substances: SubstancesFile {
                schema: v,
                substance: substances,
            },
            forms: FormsFile { schema: v, form: forms },
            categories: CategoriesFile {
                schema: v,
                category: categories,
            },
        })
    }

    fn product_from_row(r: &rusqlite::Row<'_>) -> Result<Product, StoreError> {
        let id: String = r.get(0)?;
        let status: String = r.get(5)?;
        Ok(Product {
            iherb_id: IherbId::new(id).map_err(|e| corrupt("product", e))?,
            slug: Slug::new(r.get::<_, String>(1)?).map_err(|e| corrupt("product", e))?,
            brand: r.get(2)?,
            title: r.get(3)?,
            category: CategoryId::new(r.get::<_, String>(4)?).map_err(|e| corrupt("product", e))?,
            status: ProductStatus::from_key(&status).ok_or_else(|| corrupt("product", status))?,
            tracking_url: r.get(6)?,
            label_ref: r.get(7)?,
            first_seen: date("product", &r.get::<_, String>(8)?)?,
            last_seen: date("product", &r.get::<_, String>(9)?)?,
        })
    }

    /// Products of every status, for checks and the CLI.
    pub fn all_products(&self) -> Result<Vec<Product>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT iherb_id, slug, brand, title, category_id, status, tracking_url, label_ref, first_seen, last_seen
             FROM product ORDER BY length(iherb_id), iherb_id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(Self::product_from_row(r)?);
        }
        Ok(out)
    }

    pub fn count_clicks(&self) -> Result<u64, StoreError> {
        let n: i64 = self.conn.query_row("SELECT count(*) FROM click_event", [], |r| r.get(0))?;
        u64::try_from(n).map_err(|_| corrupt("click_event", n))
    }

    pub fn count_reports(&self) -> Result<u64, StoreError> {
        let n: i64 = self.conn.query_row("SELECT count(*) FROM error_report", [], |r| r.get(0))?;
        u64::try_from(n).map_err(|_| corrupt("error_report", n))
    }
}

fn declared_columns(d: Declared) -> (&'static str, u64) {
    match d {
        Declared::SaltMass(m) => ("salt", m.ug()),
        Declared::ElementalMass(m) => ("elemental", m.ug()),
        Declared::Iu(iu) => ("iu", iu.value()),
    }
}

impl CatalogRead for SqliteStore {
    fn reference(&self) -> Result<Reference, StoreError> {
        let f = self.read_reference_files()?;
        Reference::from_files(f.substances, f.forms, f.categories).map_err(|e| StoreError::Reference(e.to_string()))
    }

    fn latest_ingest(&self) -> Result<Option<IngestRun>, StoreError> {
        self.conn
            .query_row(
                "SELECT date, fetched_at, source, feed_sha256, rows_total, rows_rejected, rows_in_scope, new_products
                 FROM ingest_run ORDER BY date DESC LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        [r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?],
                    ))
                },
            )
            .optional()?
            .map(|(d, fetched_at, source, feed_sha256, n)| {
                let n = n.map(|v| u32::try_from(v).map_err(|_| corrupt("ingest_run", v)));
                let [rows_total, rows_rejected, rows_in_scope, new_products] = n;
                Ok(IngestRun {
                    date: date("ingest_run", &d)?,
                    fetched_at: UtcTimestamp::from_unix(fetched_at),
                    source,
                    feed_sha256,
                    rows_total: rows_total?,
                    rows_rejected: rows_rejected?,
                    rows_in_scope: rows_in_scope?,
                    new_products: new_products?,
                })
            })
            .transpose()
    }

    fn products_in_category(&self, category: &CategoryId) -> Result<Vec<Product>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT iherb_id, slug, brand, title, category_id, status, tracking_url, label_ref, first_seen, last_seen
             FROM product WHERE category_id = ?1 ORDER BY length(iherb_id), iherb_id",
        )?;
        let mut rows = stmt.query(params![category.as_str()])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(Self::product_from_row(r)?);
        }
        Ok(out)
    }

    fn labels(&self, product: &IherbId) -> Result<Vec<Label>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, effective_from, serving_size, servings_per_container, verified_by, verified_at, label_photo_ref, third_party_test
             FROM label WHERE product_id = ?1 ORDER BY effective_from",
        )?;
        let mut line_stmt = self
            .conn
            .prepare("SELECT form_id, declared_as, amount, confidence FROM label_line WHERE label_id = ?1 ORDER BY position")?;
        let mut rows = stmt.query(params![product.as_str()])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let label_id: i64 = r.get(0)?;
            let mut lines = Vec::new();
            let mut lrows = line_stmt.query(params![label_id])?;
            while let Some(l) = lrows.next()? {
                let kind: String = l.get(1)?;
                let amount = u64::try_from(l.get::<_, i64>(2)?).map_err(|e| corrupt("label_line", e))?;
                let declared = match kind.as_str() {
                    "salt" => Declared::SaltMass(Mass::from_ug(amount)),
                    "elemental" => Declared::ElementalMass(Mass::from_ug(amount)),
                    "iu" => Declared::Iu(Iu::new(amount)),
                    other => return Err(corrupt("label_line", other)),
                };
                let conf: String = l.get(3)?;
                let confidence = match conf.as_str() {
                    "verified" => Confidence::Verified,
                    "recognized" => Confidence::Recognized,
                    "unknown" => Confidence::Unknown,
                    other => return Err(corrupt("label_line", other)),
                };
                lines.push(LabelLine {
                    form: FormId::new(l.get::<_, String>(0)?).map_err(|e| corrupt("label_line", e))?,
                    declared,
                    confidence,
                });
            }
            let count = |i: usize| -> Result<u32, StoreError> { u32::try_from(r.get::<_, i64>(i)?).map_err(|e| corrupt("label", e)) };
            out.push(Label {
                product: product.clone(),
                effective_from: date("label", &r.get::<_, String>(1)?)?,
                serving_size: count(2)?,
                servings_per_container: count(3)?,
                lines,
                verified_by: r.get(4)?,
                verified_at: date("label", &r.get::<_, String>(5)?)?,
                label_photo_ref: r.get(6)?,
                third_party_test: r.get(7)?,
            });
        }
        Ok(out)
    }

    fn price_on(&self, product: &IherbId, day: Date) -> Result<Option<PriceSnapshot>, StoreError> {
        self.conn
            .query_row(
                "SELECT price_cents, in_stock, hidden_until_cart, feed_row_hash FROM price_snapshot WHERE product_id = ?1 AND date = ?2",
                params![product.as_str(), day.to_string()],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|(price, in_stock, hidden_until_cart, feed_row_hash)| {
                Ok(PriceSnapshot {
                    product: product.clone(),
                    date: day,
                    price: price.map(Money::from_cents),
                    in_stock,
                    hidden_until_cart,
                    feed_row_hash,
                })
            })
            .transpose()
    }

    fn rating_on(&self, product: &IherbId, day: Date) -> Result<Option<RatingSnapshot>, StoreError> {
        self.conn
            .query_row(
                "SELECT avg, count FROM rating_snapshot WHERE product_id = ?1 AND date = ?2",
                params![product.as_str(), day.to_string()],
                |r| Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .map(|(avg, count)| {
                #[allow(clippy::cast_possible_truncation)]
                let avg = avg.map(|a| a as f32);
                let count = count
                    .map(|c| u32::try_from(c).map_err(|e| corrupt("rating_snapshot", e)))
                    .transpose()?;
                Ok(RatingSnapshot {
                    product: product.clone(),
                    date: day,
                    avg,
                    count,
                })
            })
            .transpose()
    }

    fn pending_labels(&self) -> Result<Vec<(IherbId, Date)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT product_id, queued_on FROM pending_labels ORDER BY queued_on, length(product_id), product_id")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push((
                IherbId::new(r.get::<_, String>(0)?).map_err(|e| corrupt("pending_labels", e))?,
                date("pending_labels", &r.get::<_, String>(1)?)?,
            ));
        }
        Ok(out)
    }
}

impl CatalogWrite for SqliteStore {
    fn import_reference_and_labels(&mut self, files: &ReferenceFiles, labels: &[LabelSet]) -> Result<ImportReport, StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM label_line; DELETE FROM label; DELETE FROM category; DELETE FROM form; DELETE FROM substance;")?;
        for s in &files.substances.substance {
            tx.execute(
                "INSERT INTO substance (id, name, synonyms_json, default_unit, element, ug_per_iu, source_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![s.id, s.name, json_vec(&s.synonyms), s.default_unit, s.element, s.ug_per_iu, s.source_url],
            )?;
        }
        for f in &files.forms.form {
            tx.execute(
                "INSERT INTO form (id, substance_id, name, synonyms_json, elemental_ratio, formula, bioavailability_k, bioavailability_note, ug_per_iu, source_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    f.id,
                    f.substance,
                    f.name,
                    json_vec(&f.synonyms),
                    f.elemental_ratio,
                    f.formula,
                    f.bioavailability_k,
                    f.bioavailability_note,
                    f.ug_per_iu,
                    f.source_url
                ],
            )?;
        }
        for c in &files.categories.category {
            tx.execute(
                "INSERT INTO category (id, slug, name, substance_id, unit, unit_label, feed_match_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![c.id, c.slug, c.name, c.substance, c.unit, c.unit_label, json_vec(&c.feed_match)],
            )?;
        }
        let mut report = ImportReport {
            substances: files.substances.substance.len(),
            forms: files.forms.form.len(),
            categories: files.categories.category.len(),
            ..ImportReport::default()
        };
        tx.execute("UPDATE product SET label_ref = NULL", [])?;
        for set in labels {
            report.products_with_labels += 1;
            let known = tx.execute(
                "UPDATE product SET label_ref = ?2 WHERE iherb_id = ?1",
                params![set.product.as_str(), set.label_ref],
            )?;
            if known == 0 {
                report.labels_without_product += 1;
            }
            for label in &set.labels {
                report.label_versions += 1;
                tx.execute(
                    "INSERT INTO label (product_id, effective_from, serving_size, servings_per_container, verified_by, verified_at, label_photo_ref, third_party_test)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        set.product.as_str(),
                        label.effective_from.to_string(),
                        label.serving_size,
                        label.servings_per_container,
                        label.verified_by,
                        label.verified_at.to_string(),
                        label.label_photo_ref,
                        label.third_party_test
                    ],
                )?;
                let label_id = tx.last_insert_rowid();
                for (pos, line) in label.lines.iter().enumerate() {
                    let (kind, amount) = declared_columns(line.declared);
                    tx.execute(
                        "INSERT INTO label_line (label_id, position, form_id, declared_as, amount, confidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![label_id, to_i64(pos as u64)?, line.form.as_str(), kind, to_i64(amount)?, line.confidence.key()],
                    )?;
                }
            }
        }
        Self::refresh_status(&tx)?;
        tx.commit()?;
        Ok(report)
    }

    fn record_feed_day(&mut self, day: &FeedDay) -> Result<IngestRun, StoreError> {
        let tx = self.conn.transaction()?;
        let d = day.date.to_string();
        let mut new_products: u32 = 0;
        for item in &day.items {
            let p = &item.product;
            let inserted = tx.execute(
                "INSERT INTO product (iherb_id, slug, brand, title, category_id, status, tracking_url, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending_label', ?6, ?7, ?7)
                 ON CONFLICT (iherb_id) DO NOTHING",
                params![
                    p.iherb_id.as_str(),
                    p.slug.as_str(),
                    p.brand,
                    p.title,
                    p.category.as_str(),
                    p.tracking_url,
                    d
                ],
            )?;
            if inserted == 1 {
                new_products += 1;
                tx.execute(
                    "INSERT INTO pending_labels (product_id, queued_on)
                     SELECT ?1, ?2 WHERE NOT EXISTS (SELECT 1 FROM label WHERE product_id = ?1)
                     ON CONFLICT (product_id) DO NOTHING",
                    params![p.iherb_id.as_str(), d],
                )?;
            } else {
                tx.execute(
                    "UPDATE product SET slug = ?2, brand = ?3, title = ?4, category_id = ?5, tracking_url = ?6,
                         last_seen = max(last_seen, ?7)
                     WHERE iherb_id = ?1",
                    params![
                        p.iherb_id.as_str(),
                        p.slug.as_str(),
                        p.brand,
                        p.title,
                        p.category.as_str(),
                        p.tracking_url,
                        d
                    ],
                )?;
            }
            let s = &item.price;
            tx.execute(
                "INSERT INTO price_snapshot (product_id, date, price_cents, in_stock, hidden_until_cart, feed_row_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (product_id, date) DO UPDATE SET price_cents = excluded.price_cents, in_stock = excluded.in_stock,
                     hidden_until_cart = excluded.hidden_until_cart, feed_row_hash = excluded.feed_row_hash",
                params![
                    s.product.as_str(),
                    s.date.to_string(),
                    s.price.map(Money::cents),
                    s.in_stock,
                    s.hidden_until_cart,
                    s.feed_row_hash
                ],
            )?;
            if let Some(r) = &item.rating {
                tx.execute(
                    "INSERT INTO rating_snapshot (product_id, date, avg, count) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (product_id, date) DO UPDATE SET avg = excluded.avg, count = excluded.count",
                    params![r.product.as_str(), r.date.to_string(), r.avg.map(f64::from), r.count],
                )?;
            }
        }
        let rows_in_scope = u32::try_from(day.items.len()).map_err(|e| corrupt("ingest_run", e))?;
        tx.execute(
            "INSERT INTO ingest_run (date, fetched_at, source, feed_sha256, rows_total, rows_rejected, rows_in_scope, new_products)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (date) DO UPDATE SET fetched_at = excluded.fetched_at, source = excluded.source, feed_sha256 = excluded.feed_sha256,
                 rows_total = excluded.rows_total, rows_rejected = excluded.rows_rejected, rows_in_scope = excluded.rows_in_scope,
                 new_products = excluded.new_products",
            params![d, day.fetched_at.unix(), day.source, day.feed_sha256, day.rows_total, day.rows_rejected, rows_in_scope, new_products],
        )?;
        Self::refresh_status(&tx)?;
        tx.commit()?;
        Ok(IngestRun {
            date: day.date,
            fetched_at: day.fetched_at,
            source: day.source.clone(),
            feed_sha256: day.feed_sha256.clone(),
            rows_total: day.rows_total,
            rows_rejected: day.rows_rejected,
            rows_in_scope,
            new_products,
        })
    }
}

impl EventSink for SqliteStore {
    fn insert_click(&self, e: &ClickEvent) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO click_event (product_id, preset, page, ts, session_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![e.product.as_str(), e.preset, e.page, e.ts.unix(), e.session_hash],
        )?;
        Ok(())
    }

    fn insert_report(&self, r: &ErrorReport) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO error_report (product_id, field, text, created_at, page_url) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                r.product.as_ref().map(IherbId::as_str),
                r.field,
                r.text,
                r.created_at.unix(),
                r.page_url
            ],
        )?;
        Ok(())
    }
}
