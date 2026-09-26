//! Static site from a dated snapshot (spec §7).
//!
//! `build` reads only the snapshot (through [`CatalogRead`]) and the files
//! under `site/`; it takes no clock and no network, so the same inputs give
//! byte-for-byte the same output. Every number on a page comes from
//! `catalog-core`; this crate only formats.

pub mod evaluation;
pub mod i18n;
pub mod pages;
pub mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use askama::Template;
use catalog_core::chem::ATOMIC_WEIGHTS_SOURCE;
use catalog_core::control::{BalanceFixture, ControlError};
use catalog_core::{
    Category, CategoryView, Date, Declared, Evaluation, ExactMass, Exclusion, IherbId, Label, Mass, Preset, PriceObs, Product,
    ProductInput, ProductStatus, RatingBranch, RatingObs, Reference, UnitPrice, Why, evaluate, why,
};
use catalog_store::{CatalogRead, IngestRun, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::i18n::Tr;
use crate::pages::{
    BuyButton, CardView, CategoryPage, ControlRowView, ControlView, DisclosurePage, HomeCategory, HowPage, IndexPage, LabelView, Layout,
    LineView, NeighbourView, NotFoundPage, PendingPage, PriceView, PrivacyPage, ProductPage, TabView, UnrankedView,
};

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("input: {0}")]
    Input(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("template: {0}")]
    Template(#[from] askama::Error),
    #[error("missing translations or placeholders: {0}")]
    Missing(String),
    #[error("control set: {0}")]
    Control(#[from] ControlError),
    #[error("the snapshot has no ingest run")]
    NoIngest,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub struct BuildOptions {
    pub site_dir: PathBuf,
    /// File stem in `site/i18n/`, e.g. `en`.
    pub locale: String,
    pub branch: RatingBranch,
    /// Canonical origin without a trailing slash.
    pub site_url: String,
    /// Where edge-api is mounted, e.g. `/api`.
    pub api_base: String,
    pub control: BalanceFixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStat {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryStats {
    pub slug: String,
    /// Site path of the category page's file.
    pub file: String,
    pub products: usize,
    pub ranked: usize,
    pub pending_label: usize,
}

/// Written next to the site as `manifest.json`; `verify` compares it with
/// the previous day's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub date: String,
    pub feed_fetched_at: String,
    pub branch: String,
    pub locale: String,
    pub products_with_unit_price: usize,
    pub categories: Vec<CategoryStats>,
    pub files: BTreeMap<String, FileStat>,
}

pub struct BuildOutput {
    pub manifest: Manifest,
    /// Site-relative path to content.
    pub files: BTreeMap<String, Vec<u8>>,
}

impl BuildOutput {
    /// Writes `site/…` and `manifest.json` under `dir`, which must be empty
    /// or absent.
    pub fn write_to(&self, dir: &Path) -> Result<(), BuildError> {
        let site = dir.join("site");
        for (rel, bytes) in &self.files {
            let path = site.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, bytes)?;
        }
        let manifest = serde_json::to_vec_pretty(&self.manifest).map_err(|e| BuildError::Input(e.to_string()))?;
        std::fs::write(dir.join("manifest.json"), manifest)?;
        Ok(())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn product_path(p: &Product) -> String {
    format!("/pr/{}/{}", p.slug, p.iherb_id)
}

pub fn category_path(c: &Category) -> String {
    format!("/c/{}", c.slug)
}

/// Site path to file: `/c/magnesium` is served from `c/magnesium.html`.
pub fn file_for_path(path: &str) -> String {
    if path == "/" {
        "index.html".to_owned()
    } else {
        format!("{}.html", path.trim_start_matches('/'))
    }
}

struct Ctx<'a> {
    t: &'a Tr,
    opts: &'a BuildOptions,
    reference: &'a Reference,
    run: &'a IngestRun,
    css: String,
    js: String,
    updated: String,
    nav: Vec<(String, String)>,
}

impl Ctx<'_> {
    fn layout(&self, title: &str, description: &str, path: &str, noindex: bool) -> Layout {
        let site = self.t.get("site.name");
        Layout {
            lang: self.t.locale.lang.clone(),
            dir: self.t.locale.dir.clone(),
            title: if path == "/" {
                site.to_owned()
            } else {
                format!("{title} · {site}")
            },
            description: description.to_owned(),
            canonical: format!("{}{}", self.opts.site_url, path),
            noindex,
            css: self.css.clone(),
            js: self.js.clone(),
            api: self.opts.api_base.clone(),
            updated: Some(self.updated.clone()),
            nav: self.nav.clone(),
        }
    }

    fn substance_name(&self, c: &Category) -> String {
        self.reference
            .substances
            .get(&c.substance)
            .map_or_else(|| c.substance.to_string(), |s| s.name.to_lowercase())
    }

    /// The network's tracking link; a product missing from today's feed has
    /// no current link.
    fn buy_href(&self, p: &Product) -> Option<String> {
        (p.status != ProductStatus::Delisted).then(|| p.tracking_url.clone()).flatten()
    }

    fn buy(&self, p: &Product) -> Result<String, BuildError> {
        Ok(BuyButton {
            t: self.t,
            id: p.iherb_id.to_string(),
            href: self.buy_href(p),
        }
        .render()?)
    }

    fn preset_label(&self, preset: Preset, c: &Category) -> String {
        self.t
            .f(&format!("preset.{}.label", preset.key()), &[("unit", &self.t.mass(c.unit))])
    }

    fn preset_note(&self, preset: Preset, c: &Category) -> String {
        self.t.f(
            &format!("preset.{}.note", preset.key()),
            &[("unit", &self.t.mass(c.unit)), ("substance", &self.substance_name(c))],
        )
    }

    /// Why `preset` ranks nothing today.
    fn preset_empty(&self, preset: Preset, c: &Category) -> String {
        self.t.f(
            &format!("preset.{}.empty", preset.key()),
            &[("unit", &self.t.mass(c.unit)), ("substance", &self.substance_name(c))],
        )
    }

    fn exclusion(&self, e: &Exclusion, c: &Category) -> String {
        self.t
            .f(&format!("exclusion.{}", e.key()), &[("substance", &self.substance_name(c))])
    }

    /// `$14.50 for 120 servings of 202.037 mg`, for ranked products.
    fn container_line(&self, e: &Evaluation) -> Option<String> {
        let (label, dose, price) = (e.label.as_ref()?, e.dose.as_ref()?, e.price.as_ref()?.price?);
        Some(self.t.f(
            "card.container",
            &[
                ("price", &self.t.money(price)),
                ("servings", &self.t.int(u64::from(label.servings_per_container))),
                ("dose", &self.t.mass(dose.per_serving)),
            ],
        ))
    }

    fn price_formula(&self, e: &Evaluation, c: &Category, up: UnitPrice) -> Option<String> {
        let (label, dose, price) = (e.label.as_ref()?, e.dose.as_ref()?, e.price.as_ref()?.price?);
        Some(self.t.f(
            "product.price_line",
            &[
                ("price", &self.t.money(price)),
                ("servings", &self.t.int(u64::from(label.servings_per_container))),
                ("dose", &self.t.mass(dose.per_serving)),
                ("unit", &self.t.mass(c.unit)),
                ("unit_price", &self.t.unit_price(up)),
            ],
        ))
    }

    fn why_text(&self, w: &Why, c: &Category) -> String {
        let t = self.t;
        match w {
            Why::CheaperThanMedian { percent } | Why::PricierThanMedian { percent } => {
                t.why(w.key(), &[("percent", &percent.to_string()), ("unit", &t.mass(c.unit))])
            }
            Why::Rating { avg, count } => t.why(
                w.key(),
                &[("avg", &t.decimal(f64::from(*avg), 1)), ("count", &t.int(u64::from(*count)))],
            ),
            Why::FormulaChanged { year } => t.why(w.key(), &[("year", &year.to_string())]),
            Why::ThirdPartyTest { name } => t.why(w.key(), &[("name", name)]),
        }
    }
}

struct Built {
    category: Category,
    products: Vec<Product>,
    view: CategoryView,
}

/// Drops comments and collapses whitespace. The stylesheet has no strings or
/// `url()`s where that would matter; the budget applies to the source file.
pub fn minify_css(css: &str) -> String {
    let mut no_comments = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        no_comments.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    no_comments.push_str(rest);
    let collapsed = no_comments.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    for c in collapsed.chars() {
        if matches!(c, '{' | '}' | ';' | ',' | '>') && out.ends_with(' ') {
            out.pop();
        }
        if c == ' ' && (out.ends_with(['{', '}', ';', ',', '>'])) {
            continue;
        }
        if c == '}' && out.ends_with(';') {
            out.pop();
        }
        out.push(c);
    }
    out
}

fn read_site_file(site_dir: &Path, rel: &str) -> Result<String, BuildError> {
    let p = site_dir.join(rel);
    std::fs::read_to_string(&p).map_err(|e| BuildError::Input(format!("{}: {e}", p.display())))
}

fn load_category(
    store: &impl CatalogRead,
    reference: &Reference,
    category: &Category,
    branch: RatingBranch,
    as_of: Date,
) -> Result<Built, BuildError> {
    let products = store.products_in_category(&category.id)?;
    let mut inputs = Vec::with_capacity(products.len());
    for p in &products {
        let price = store.price_on(&p.iherb_id, as_of)?.map(|s| PriceObs {
            date: s.date,
            price: s.price,
            in_stock: s.in_stock,
            hidden_until_cart: s.hidden_until_cart,
        });
        let rating = if branch.uses_ratings() {
            store.rating_on(&p.iherb_id, as_of)?.and_then(|r| RatingObs::new(r.avg?, r.count?))
        } else {
            None
        };
        inputs.push(ProductInput {
            id: p.iherb_id.clone(),
            labels: store.labels(&p.iherb_id)?,
            price,
            rating,
        });
    }
    let view = evaluate(category, reference, branch, as_of, &inputs);
    Ok(Built {
        category: category.clone(),
        products,
        view,
    })
}

pub fn build(store: &impl CatalogRead, opts: &BuildOptions) -> Result<BuildOutput, BuildError> {
    let t = Tr::load(&opts.site_dir, &opts.locale)?;
    let run = store.latest_ingest()?.ok_or(BuildError::NoIngest)?;
    let reference = store.reference()?;
    let css = minify_css(&read_site_file(&opts.site_dir, "static/site.css")?);
    let js_source = read_site_file(&opts.site_dir, "static/app.js")?;
    let js = format!("/static/app.js?v={}", &sha256_hex(js_source.as_bytes())[..10]);
    let nav = reference.categories.values().map(|c| (category_path(c), c.name.clone())).collect();
    let ctx = Ctx {
        t: &t,
        opts,
        reference: &reference,
        run: &run,
        css,
        js,
        updated: t.f("footer.updated", &[("time", &t.timestamp(run.fetched_at))]),
        nav,
    };

    let queued: BTreeMap<IherbId, Date> = store.pending_labels()?.into_iter().collect();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut stats = Vec::new();
    let mut home = Vec::new();
    let mut built_all = Vec::new();
    for category in reference.categories.values() {
        let built = load_category(store, &reference, category, opts.branch, run.date)?;
        let path = category_path(category);
        files.insert(file_for_path(&path), category_page(&ctx, &built)?.into_bytes());
        let evaluation = evaluation::evaluation_file_for(&ctx, &built);
        files.insert(
            evaluation::evaluation_file(&file_for_path(&path)),
            serde_json::to_vec(&evaluation).map_err(|e| BuildError::Input(e.to_string()))?,
        );
        for p in &built.products {
            let html = product_page(&ctx, &built, p, queued.get(&p.iherb_id).copied())?;
            files.insert(file_for_path(&product_path(p)), html.into_bytes());
        }
        let ranked = built.view.evaluations.values().filter(|e| e.unit_price.is_ok()).count();
        let pending = built
            .view
            .evaluations
            .values()
            .filter(|e| matches!(&e.unit_price, Err(x) if x.is_pending_label()))
            .count();
        home.push(HomeCategory {
            href: path.clone(),
            name: category.name.clone(),
            counts: t.f(
                "home.products",
                &[("count", &t.int(built.products.len() as u64)), ("ranked", &t.int(ranked as u64))],
            ),
        });
        stats.push(CategoryStats {
            slug: category.slug.to_string(),
            file: file_for_path(&path),
            products: built.products.len(),
            ranked,
            pending_label: pending,
        });
        built_all.push(built);
    }

    files.insert("how.html".into(), how_page(&ctx, &built_all)?.into_bytes());
    let home_page = IndexPage {
        t: &t,
        l: ctx.layout(t.get("home.title"), t.get("site.tagline"), "/", false),
        categories: home,
    };
    files.insert("index.html".into(), home_page.render()?.into_bytes());
    let disclosure = DisclosurePage {
        t: &t,
        l: ctx.layout(t.get("disclosure.title"), t.get("disclosure.title"), "/disclosure", false),
    };
    files.insert("disclosure.html".into(), disclosure.render()?.into_bytes());
    let privacy = PrivacyPage {
        t: &t,
        l: ctx.layout(t.get("privacy.title"), t.get("privacy.body_1"), "/privacy", false),
    };
    files.insert("privacy.html".into(), privacy.render()?.into_bytes());
    let not_found = NotFoundPage {
        t: &t,
        l: ctx.layout(t.get("notfound.title"), t.get("notfound.body"), "/404", true),
    };
    files.insert("404.html".into(), not_found.render()?.into_bytes());
    files.insert("static/app.js".into(), js_source.into_bytes());
    files.insert(
        "static/icon.svg".into(),
        read_site_file(&opts.site_dir, "static/icon.svg")?.into_bytes(),
    );
    files.insert("robots.txt".into(), b"User-agent: *\nAllow: /\n".to_vec());

    let missing = t.take_missing();
    if !missing.is_empty() {
        return Err(BuildError::Missing(missing.join(", ")));
    }

    let manifest = Manifest {
        date: run.date.to_string(),
        feed_fetched_at: run.fetched_at.to_string(),
        branch: opts.branch.key().to_owned(),
        locale: t.locale.code.clone(),
        products_with_unit_price: stats.iter().map(|s| s.ranked).sum(),
        categories: stats,
        files: files
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    FileStat {
                        bytes: v.len() as u64,
                        sha256: sha256_hex(v),
                    },
                )
            })
            .collect(),
    };
    Ok(BuildOutput { manifest, files })
}

fn category_page(ctx: &Ctx<'_>, b: &Built) -> Result<String, BuildError> {
    let (t, c, view) = (ctx.t, &b.category, &b.view);
    let branch = ctx.opts.branch;
    let default = branch.default_preset();
    let by_id: BTreeMap<&IherbId, &Product> = b.products.iter().map(|p| (&p.iherb_id, p)).collect();
    let default_order = view.rankings.get(&default).cloned().unwrap_or_default();

    let mut presets: Vec<Preset> = vec![default];
    presets.extend(branch.presets().iter().copied().filter(|p| *p != default));
    let tabs = presets
        .iter()
        .map(|p| TabView {
            key: p.key(),
            label: ctx.preset_label(*p, c),
            note: ctx.preset_note(*p, c),
            href: format!("?preset={}", p.key()),
            current: *p == default,
            // An empty ranking says why, instead of showing an empty list (AC22).
            empty: view.rankings.get(p).is_none_or(Vec::is_empty).then(|| ctx.preset_empty(*p, c)),
        })
        .collect();
    let orders = presets
        .iter()
        .map(|p| {
            let ids = view
                .rankings
                .get(p)
                .map(|ids| ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            (p.key(), ids)
        })
        .collect();

    // Cards: the default order first, then candidates that only other
    // presets include, hidden until a script switches to them.
    let mut ordered: Vec<(usize, &IherbId)> = default_order.iter().enumerate().map(|(i, id)| (i + 1, id)).collect();
    let in_default: BTreeSet<&IherbId> = default_order.iter().collect();
    for e in view.evaluations.values() {
        if e.unit_price.is_ok() && !in_default.contains(&e.id) {
            ordered.push((0, &e.id));
        }
    }
    let mut cards = Vec::with_capacity(ordered.len());
    for (rank, id) in ordered {
        let (Some(p), Some(e)) = (by_id.get(id), view.evaluations.get(id)) else {
            continue;
        };
        let Ok(up) = e.unit_price else { continue };
        cards.push(CardView {
            id: id.to_string(),
            rank,
            hidden: rank == 0,
            href: product_path(p),
            title: p.title.clone(),
            brand: p.brand.clone(),
            unit_price: t.unit_price(up),
            per_unit: c.unit_label.clone(),
            container: ctx.container_line(e).unwrap_or_default(),
            rating: branch.uses_ratings().then(|| match e.adjusted_rating {
                Some(r) => t.f("card.rating", &[("value", &t.decimal(r, 2))]),
                None => t.get("card.no_rating").to_owned(),
            }),
            why: why(view, id).iter().map(|w| ctx.why_text(w, c)).collect(),
            out_of_stock: e.price.as_ref().is_some_and(|p| !p.in_stock),
            buy: ctx.buy(p)?,
        });
    }

    let unranked = view
        .evaluations
        .values()
        .filter_map(|e| {
            let Err(x) = &e.unit_price else { return None };
            let p = by_id.get(&e.id)?;
            Some(UnrankedView {
                href: product_path(p),
                name: format!("{} {}", p.brand, p.title),
                reason: ctx.exclusion(x, c),
            })
        })
        .collect();

    let substance = ctx.substance_name(c);
    let unit = t.mass(c.unit);
    let title = t.f("category.title", &[("name", &c.name), ("unit", &unit)]);
    let lede = t.f("category.lede", &[("unit", &unit), ("substance", &substance)]);
    let page = CategoryPage {
        t,
        l: ctx.layout(&title, &lede, &category_path(c), false),
        counts: t.f(
            "category.counts",
            &[
                ("count", &t.int(b.products.len() as u64)),
                (
                    "ranked",
                    &t.int(view.evaluations.values().filter(|e| e.unit_price.is_ok()).count() as u64),
                ),
            ],
        ),
        title,
        lede,
        tabs,
        default_key: default.key(),
        orders,
        cards,
        unranked,
    };
    Ok(page.render()?)
}

fn label_view(ctx: &Ctx<'_>, c: &Category, e: &Evaluation, label: &Label) -> LabelView {
    let t = ctx.t;
    let substance = ctx.substance_name(c);
    let mut sources: Vec<(String, String)> = Vec::new();
    let mut uses_formula = false;
    let lines = label
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let n = i + 1;
            let form = ctx.reference.forms.get(&line.form);
            let form_name = form.map_or_else(|| line.form.to_string(), |f| f.name.clone());
            if let Some(f) = form
                && !sources.iter().any(|(name, _)| *name == f.name)
            {
                sources.push((f.name.clone(), f.source_url.clone()));
            }
            let amount = match line.declared {
                Declared::SaltMass(m) | Declared::ElementalMass(m) => t.mass(m),
                Declared::Iu(iu) => format!("{} IU", t.int(iu.value())),
            };
            let same_substance = form.is_some_and(|f| f.substance == c.substance);
            let mut counted_as = if !same_substance {
                t.f("product.other_substance", &[("substance", &substance)])
            } else {
                match line.declared {
                    Declared::SaltMass(_) => t.get("product.declared_salt").to_owned(),
                    Declared::ElementalMass(_) => t.f("product.declared_elemental", &[("substance", &substance)]),
                    Declared::Iu(_) => t.get("product.declared_iu").to_owned(),
                }
            };
            match line.confidence {
                catalog_core::Confidence::Verified => {}
                catalog_core::Confidence::Recognized => counted_as = format!("{counted_as} ({})", t.get("product.confidence_recognized")),
                catalog_core::Confidence::Unknown => counted_as = format!("{counted_as} ({})", t.get("product.confidence_unknown")),
            }
            let computed = e.dose.as_ref().and_then(|d| d.lines.iter().find(|l| l.line == n));
            let factor = match (same_substance, line.declared, form) {
                (false, _, _) | (_, _, None) => "—".to_owned(),
                (true, Declared::ElementalMass(_), _) => t.get("product.factor_one").to_owned(),
                (true, Declared::SaltMass(_), Some(f)) => {
                    uses_formula |= f.formula.is_some();
                    let formula = f
                        .formula
                        .as_ref()
                        .map_or_else(|| f.elemental_ratio.to_string(), |x| x.text().to_owned());
                    t.f(
                        "product.factor_ratio",
                        &[("percent", &t.percent(f.elemental_ratio.to_f64(), 2)), ("formula", &formula)],
                    )
                }
                (true, Declared::Iu(_), Some(_)) => computed.map_or_else(
                    || "—".to_owned(),
                    |l| t.f("product.factor_iu", &[("ug", &t.decimal(l.factor.to_f64(), 3))]),
                ),
            };
            LineView {
                n,
                printed: format!("{amount} {form_name}"),
                counted_as,
                factor,
                result: computed.map_or_else(|| "—".to_owned(), |l| t.exact_mass(l.elemental)),
            }
        })
        .collect();
    if uses_formula {
        sources.push((t.get("product.source_atomic_weights").to_owned(), ATOMIC_WEIGHTS_SOURCE.to_owned()));
    }
    LabelView {
        result_heading: t.f("product.label_col_result", &[("substance", &c.name)]),
        lines,
        per_serving: e.dose.as_ref().map(|d| {
            t.f(
                "product.per_serving",
                &[
                    ("dose", &t.mass(d.per_serving)),
                    ("substance", &substance),
                    ("size", &t.int(u64::from(label.serving_size))),
                    ("servings", &t.int(u64::from(label.servings_per_container))),
                ],
            )
        }),
        verified: t.f(
            "product.verified",
            &[("who", &label.verified_by), ("date", &t.date(label.verified_at))],
        ),
        formula_changed: e.formula_changed.map(|d| t.f("product.formula_changed", &[("date", &t.date(d))])),
        third_party: label.third_party_test.as_deref().map_or_else(
            || t.get("product.third_party_none").to_owned(),
            |name| t.f("product.third_party", &[("name", name)]),
        ),
        sources,
    }
}

fn product_page(ctx: &Ctx<'_>, b: &Built, p: &Product, queued: Option<Date>) -> Result<String, BuildError> {
    let (t, c, view) = (ctx.t, &b.category, &b.view);
    let path = product_path(p);
    let e = view
        .evaluations
        .get(&p.iherb_id)
        .ok_or_else(|| BuildError::Input(format!("product {} not evaluated", p.iherb_id)))?;
    let name = format!("{} {}", p.brand, p.title);

    if matches!(&e.unit_price, Err(x) if x.is_pending_label()) {
        let heading = t.f("pending.title", &[("title", &name)]);
        let page = PendingPage {
            t,
            l: ctx.layout(&heading, t.get("pending.lede"), &path, true),
            crumb_href: category_path(c),
            crumb_name: c.name.clone(),
            heading,
            brand: p.brand.clone(),
            queued: queued.map(|d| t.f("pending.queued", &[("date", &t.date(d))])),
            buy: ctx.buy(p)?,
        };
        return Ok(page.render()?);
    }

    let branch = ctx.opts.branch;
    let price = e.unit_price.as_ref().ok().map(|up| PriceView {
        unit_price: t.unit_price(*up),
        per_unit: c.unit_label.clone(),
        container: ctx.container_line(e).unwrap_or_default(),
        formula: ctx.price_formula(e, c, *up).unwrap_or_default(),
        date: t.f("product.price_date", &[("date", &t.timestamp(ctx.run.fetched_at))]),
    });
    let positions = if e.unit_price.is_ok() {
        branch
            .presets()
            .iter()
            .map(|preset| {
                let label = ctx.preset_label(*preset, c);
                let total = view.rankings.get(preset).map_or(0, Vec::len);
                match view.position(*preset, &p.iherb_id) {
                    Some(pos) => t.f(
                        "product.position",
                        &[("preset", &label), ("pos", &pos.to_string()), ("total", &total.to_string())],
                    ),
                    None => t.f("product.position_none", &[("preset", &label)]),
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let default = branch.default_preset();
    let order = view.rankings.get(&default).cloned().unwrap_or_default();
    let by_id: BTreeMap<&IherbId, &Product> = b.products.iter().map(|x| (&x.iherb_id, x)).collect();
    let neighbours = match order.iter().position(|id| id == &p.iherb_id) {
        None => Vec::new(),
        Some(i) => {
            // Up to five others around the product: two above, three below,
            // shifted at the ends of the list.
            let start = i.saturating_sub(2).min(order.len().saturating_sub(6));
            let end = (start + 6).min(order.len());
            order[start..end]
                .iter()
                .enumerate()
                .filter_map(|(k, id)| {
                    let q = by_id.get(id)?;
                    let up = view.evaluations.get(id)?.unit_price.as_ref().ok()?;
                    Some(NeighbourView {
                        rank: start + k + 1,
                        href: product_path(q),
                        name: format!("{} {}", q.brand, q.title),
                        unit_price: t.unit_price(*up),
                        is_self: *id == p.iherb_id,
                    })
                })
                .collect()
        }
    };

    let description = price
        .as_ref()
        .map_or_else(|| name.clone(), |pv| format!("{name}: {} {}", pv.unit_price, pv.per_unit));
    let page = ProductPage {
        t,
        l: ctx.layout(&name, &description, &path, false),
        id: p.iherb_id.to_string(),
        path: path.clone(),
        crumb_href: category_path(c),
        crumb_name: c.name.clone(),
        name: p.title.clone(),
        brand: p.brand.clone(),
        price,
        not_compared: e
            .unit_price
            .as_ref()
            .err()
            .map(|x| t.f("product.not_compared", &[("reason", &ctx.exclusion(x, c))])),
        out_of_stock: e.price.as_ref().is_some_and(|x| !x.in_stock),
        buy: ctx.buy(p)?,
        positions,
        label: e.label.as_ref().map(|l| label_view(ctx, c, e, l)),
        neighbours_title: t.f("product.neighbours", &[("preset", &ctx.preset_label(default, c))]),
        neighbours,
        show_rating_field: branch.uses_ratings(),
    };
    Ok(page.render()?)
}

fn how_page(ctx: &Ctx<'_>, built: &[Built]) -> Result<String, BuildError> {
    let t = ctx.t;
    let branch = ctx.opts.branch;
    let first = built.first();

    // Worked example of §5.1 with the reference's own factor: 500 mg of a
    // citrate if there is one (the spec's example), else the first form.
    let dose_example = first.and_then(|b| {
        let c = &b.category;
        let forms: Vec<_> = ctx.reference.forms.values().filter(|f| f.substance == c.substance).collect();
        let form = forms.iter().find(|f| f.id.as_str().contains("citrate")).or(forms.first())?;
        let printed = Mass::from_mg(500)?;
        let result = ExactMass::from_mass(printed).times(form.elemental_ratio).ok()?.floor().ok()?;
        Some(t.f(
            "how.dose_example",
            &[
                ("printed", &t.mass(printed)),
                ("form", &form.name.to_lowercase()),
                ("ratio", &form.elemental_ratio.to_string()),
                ("percent", &t.percent(form.elemental_ratio.to_f64(), 2)),
                ("result", &t.mass(result)),
                ("substance", &ctx.substance_name(c)),
            ],
        ))
    });

    // Worked example of §5.2 on today's first-ranked product.
    let unit_example = first.and_then(|b| {
        let id = b.view.rankings.get(&branch.default_preset())?.first()?;
        let e = b.view.evaluations.get(id)?;
        let up = *e.unit_price.as_ref().ok()?;
        let (label, dose, price) = (e.label.as_ref()?, e.dose.as_ref()?, e.price.as_ref()?.price?);
        Some(t.f(
            "how.unit_example",
            &[
                ("price", &t.money(price)),
                ("servings", &t.int(u64::from(label.servings_per_container))),
                ("dose", &t.mass(dose.per_serving)),
                ("unit", &t.mass(b.category.unit)),
                ("unit_price", &t.unit_price(up)),
            ],
        ))
    });

    let control = if branch.uses_ratings() {
        let result = ctx.opts.control.check()?;
        Some(ControlView {
            rows: result
                .rows
                .iter()
                .map(|r| ControlRowView {
                    name: r.name.clone(),
                    p_unit: t.decimal(r.p_unit.cents_f64() / 100.0, 2),
                    r_b: t.decimal(r.r_b, 2),
                    q_price: t.decimal(r.score.q_price, 1),
                    q_rating: t.decimal(r.score.q_rating, 1),
                    s: t.decimal(r.score.s, 1),
                })
                .collect(),
            order: t.f("how.order", &[("order", &result.order.join(", "))]),
        })
    } else {
        None
    };

    let (unit, substance) = first.map_or_else(
        || (String::new(), String::new()),
        |b| (t.mass(b.category.unit), ctx.substance_name(&b.category)),
    );
    let presets = first.map_or_else(Vec::new, |b| {
        branch
            .presets()
            .iter()
            .map(|p| (ctx.preset_label(*p, &b.category), ctx.preset_note(*p, &b.category)))
            .collect()
    });
    let page = HowPage {
        t,
        l: ctx.layout(t.get("how.title"), t.get("how.lede"), "/how", false),
        dose_example,
        unit_text: t.f("how.unit_text", &[("unit", &unit), ("substance", &substance)]),
        unit_example,
        control,
        presets,
        ratings_shown: branch.uses_ratings(),
    };
    Ok(page.render()?)
}

#[cfg(test)]
mod tests {
    use super::minify_css;

    #[test]
    fn minifies_css() {
        let css = "/* c */\n:root { --a: #fff; }\n.a > b,\n.c { margin: 0 auto;  }\n@media (x: y) { .d { e: f; } }";
        assert_eq!(minify_css(css), ":root{--a: #fff}.a>b,.c{margin: 0 auto}@media (x: y){.d{e: f}}");
    }
}
