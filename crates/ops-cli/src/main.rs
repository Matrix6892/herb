//! `vitrina`: the daily job and its parts (spec §6).
//!
//! ```text
//! vitrina ingest && vitrina build && vitrina verify && vitrina publish
//! ```
//!
//! Exit codes: 0 success, 1 other error, 2 feed unavailable or unusable (no
//! snapshot is written), 3 verification failed (nothing is published),
//! 4 upload failed (the previous version stays live).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use catalog_core::control::BalanceFixture;
use catalog_core::{Date, ProductStatus, RatingBranch, UtcTimestamp};
use catalog_store::files::{read_labels_dir, read_reference_dir};
use catalog_store::{CatalogRead, CatalogWrite, SqliteStore};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use site_gen::verify::{Budgets, verify};
use site_gen::{BuildOptions, Manifest};

#[derive(Parser)]
#[command(name = "vitrina", about = "Daily data pipeline and site generator")]
struct Cli {
    /// Repository root; relative paths in the config resolve against it.
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,
    /// Config file, relative to the root.
    #[arg(long, default_value = "config/vitrina.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import reference and labels, read the feed, record today's prices and
    /// write a dated snapshot.
    Ingest {
        /// Read the feed from a file instead of `VITRINA_FEED_URL`.
        #[arg(long)]
        feed_file: Option<PathBuf>,
        /// Observation date (default: today, UTC).
        #[arg(long)]
        date: Option<String>,
    },
    /// Validate `data/reference` and `data/labels` and load them into the
    /// database.
    ImportLabels {
        /// Label directory (default from the config).
        #[arg(long)]
        labels: Option<PathBuf>,
    },
    /// Generate the site from a snapshot into `out/<date>/`.
    Build {
        /// Snapshot file (default: the newest in the snapshot directory).
        #[arg(long)]
        snapshot: Option<PathBuf>,
    },
    /// Check a build before publication.
    Verify {
        #[arg(long)]
        date: Option<String>,
    },
    /// Upload a verified build and switch `out/current` to it.
    Publish {
        #[arg(long)]
        date: Option<String>,
        /// Only switch `out/current`; skip the upload command.
        #[arg(long)]
        local: bool,
    },
    /// Show the last ingest, the label queue and event counts.
    Status,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    rating_branch: String,
    locale: String,
    site_url: String,
    api_base: String,
    paths: Paths,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paths {
    db: PathBuf,
    snapshots: PathBuf,
    out: PathBuf,
    reference: PathBuf,
    labels: PathBuf,
    site: PathBuf,
    control: PathBuf,
}

struct Env {
    root: PathBuf,
    cfg: Config,
}

impl Env {
    fn path(&self, p: &Path) -> PathBuf {
        if p.is_absolute() { p.to_owned() } else { self.root.join(p) }
    }

    fn branch(&self) -> anyhow::Result<RatingBranch> {
        match self.cfg.rating_branch.as_str() {
            "A" => Ok(RatingBranch::A),
            "B" => Ok(RatingBranch::B),
            other => bail!("rating_branch must be \"A\" or \"B\", not {other:?}"),
        }
    }

    fn open_db(&self) -> anyhow::Result<SqliteStore> {
        let db = self.path(&self.cfg.paths.db);
        if let Some(parent) = db.parent() {
            std::fs::create_dir_all(parent)?;
        }
        SqliteStore::open(&db).with_context(|| format!("opening {}", db.display()))
    }

    fn control(&self) -> anyhow::Result<BalanceFixture> {
        let p = self.path(&self.cfg.paths.control);
        let text = std::fs::read_to_string(&p).with_context(|| p.display().to_string())?;
        toml::from_str(&text).with_context(|| p.display().to_string())
    }

    fn out(&self) -> PathBuf {
        self.path(&self.cfg.paths.out)
    }
}

/// A failure with its exit code.
struct Fail(u8, anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for Fail {
    fn from(e: E) -> Fail {
        Fail(1, e.into())
    }
}

fn code<T>(c: u8, r: anyhow::Result<T>) -> Result<T, Fail> {
    r.map_err(|e| Fail(c, e))
}

fn now() -> UtcTimestamp {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    UtcTimestamp::from_unix(i64::try_from(secs).unwrap_or(i64::MAX))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail(c, e)) => {
            eprintln!("vitrina: {e:#}");
            ExitCode::from(c)
        }
    }
}

fn run(cli: Cli) -> Result<(), Fail> {
    let cfg_path = cli.root.join(&cli.config);
    let text = std::fs::read_to_string(&cfg_path).with_context(|| cfg_path.display().to_string())?;
    let cfg: Config = toml::from_str(&text).with_context(|| cfg_path.display().to_string())?;
    let env = Env { root: cli.root, cfg };
    match cli.command {
        Command::Ingest { feed_file, date } => ingest(&env, feed_file.as_deref(), date.as_deref()),
        Command::ImportLabels { labels } => {
            let mut store = env.open_db()?;
            import(&env, &mut store, labels.as_deref())?;
            Ok(())
        }
        Command::Build { snapshot } => build(&env, snapshot),
        Command::Verify { date } => verify_cmd(&env, date.as_deref()),
        Command::Publish { date, local } => publish(&env, date.as_deref(), local),
        Command::Status => status(&env),
    }
}

fn import(env: &Env, store: &mut SqliteStore, labels: Option<&Path>) -> anyhow::Result<catalog_core::Reference> {
    let (files, reference) = read_reference_dir(&env.path(&env.cfg.paths.reference))?;
    let labels_dir = labels.map_or_else(|| env.path(&env.cfg.paths.labels), Path::to_owned);
    let sets = read_labels_dir(&labels_dir, &reference)?;
    let r = store.import_reference_and_labels(&files, &sets)?;
    println!(
        "import: {} substances, {} forms, {} categories; {} products with labels ({} versions), {} not yet in the feed",
        r.substances, r.forms, r.categories, r.products_with_labels, r.label_versions, r.labels_without_product
    );
    Ok(reference)
}

fn ingest(env: &Env, feed_file: Option<&Path>, date: Option<&str>) -> Result<(), Fail> {
    let branch = env.branch()?;
    let mut store = env.open_db()?;
    // Validate the repository's files before touching the feed; import them
    // after it, so that labels meet today's products.
    let (_, reference) = read_reference_dir(&env.path(&env.cfg.paths.reference))?;

    let (bytes, source) = match feed_file {
        Some(path) => (
            code(2, std::fs::read(path).with_context(|| format!("feed file {}", path.display())))?,
            feed_ingest::describe_source(&path.to_string_lossy()),
        ),
        None => {
            let url = code(
                2,
                std::env::var("VITRINA_FEED_URL").map_err(|_| anyhow!("VITRINA_FEED_URL is not set")),
            )?;
            (
                code(2, feed_ingest::fetch(&url).map_err(anyhow::Error::from))?,
                feed_ingest::describe_source(&url),
            )
        }
    };
    let feed = code(2, feed_ingest::parse_feed(&bytes).map_err(anyhow::Error::from))?;
    let fetched_at = now();
    let date = match date {
        Some(d) => Date::parse(d)?,
        None => fetched_at.date().ok_or_else(|| anyhow!("clock out of range"))?,
    };
    let day = feed_ingest::build_day(&feed, &reference, branch, date, fetched_at, source);
    if day.items.is_empty() {
        return Err(Fail(
            2,
            anyhow!(
                "the feed has {} rows but none in a known category; nothing recorded",
                feed.rows_total
            ),
        ));
    }
    let run = store.record_feed_day(&day)?;
    import(env, &mut store, None)?;
    let snapshots = env.path(&env.cfg.paths.snapshots);
    std::fs::create_dir_all(&snapshots)?;
    let snap = snapshots.join(format!("vitrina-{date}.db"));
    store.snapshot_to(&snap)?;
    println!(
        "ingest {date}: {} rows, {} rejected, {} without price, {} in scope, {} new; snapshot {}",
        run.rows_total,
        run.rows_rejected,
        feed.rows_without_price,
        run.rows_in_scope,
        run.new_products,
        snap.display()
    );
    for r in feed.rejected.iter().take(20) {
        println!("  rejected row {}: {}", r.row, r.reason);
    }
    if feed.rejected.len() > 20 {
        println!("  … and {} more", feed.rejected.len() - 20);
    }
    Ok(())
}

fn dated_dirs(out: &Path) -> anyhow::Result<Vec<Date>> {
    let mut dates = Vec::new();
    if out.exists() {
        for e in std::fs::read_dir(out)? {
            let e = e?;
            if e.file_type()?.is_dir()
                && let Ok(d) = Date::parse(&e.file_name().to_string_lossy())
            {
                dates.push(d);
            }
        }
    }
    dates.sort();
    Ok(dates)
}

fn pick_build(env: &Env, date: Option<&str>) -> anyhow::Result<(Date, PathBuf)> {
    let date = match date {
        Some(d) => Date::parse(d)?,
        None => *dated_dirs(&env.out())?
            .last()
            .ok_or_else(|| anyhow!("no build in {}", env.out().display()))?,
    };
    let dir = env.out().join(date.to_string());
    if !dir.is_dir() {
        bail!("no build at {}", dir.display());
    }
    Ok((date, dir))
}

fn latest_snapshot(dir: &Path) -> anyhow::Result<PathBuf> {
    let mut snaps: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| dir.display().to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("vitrina-") && n.ends_with(".db"))
        })
        .collect();
    snaps.sort();
    snaps.pop().ok_or_else(|| anyhow!("no snapshot in {}", dir.display()))
}

fn build(env: &Env, snapshot: Option<PathBuf>) -> Result<(), Fail> {
    let snapshot = match snapshot {
        Some(p) => p,
        None => latest_snapshot(&env.path(&env.cfg.paths.snapshots))?,
    };
    let store = SqliteStore::open_snapshot(&snapshot).with_context(|| snapshot.display().to_string())?;
    let opts = BuildOptions {
        site_dir: env.path(&env.cfg.paths.site),
        locale: env.cfg.locale.clone(),
        branch: env.branch()?,
        site_url: env.cfg.site_url.trim_end_matches('/').to_owned(),
        api_base: env.cfg.api_base.trim_end_matches('/').to_owned(),
        control: env.control()?,
    };
    let output = site_gen::build(&store, &opts)?;
    let out = env.out();
    std::fs::create_dir_all(&out)?;
    let date = output.manifest.date.clone();
    let tmp = out.join(format!(".tmp-{date}"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    output.write_to(&tmp)?;
    let dest = out.join(&date);
    if dest.exists() {
        let old = out.join(format!(".old-{date}"));
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        std::fs::rename(&dest, &old)?;
        std::fs::rename(&tmp, &dest)?;
        std::fs::remove_dir_all(&old)?;
    } else {
        std::fs::rename(&tmp, &dest)?;
    }
    let m = &output.manifest;
    println!(
        "build {date} from {}: {} files, {} products with a unit price (branch {})",
        snapshot.display(),
        m.files.len(),
        m.products_with_unit_price,
        m.branch
    );
    Ok(())
}

fn read_manifest(dir: &Path) -> anyhow::Result<Manifest> {
    let p = dir.join("manifest.json");
    let bytes = std::fs::read(&p).with_context(|| p.display().to_string())?;
    serde_json::from_slice(&bytes).with_context(|| p.display().to_string())
}

const VERIFIED_MARKER: &str = "verified";

fn manifest_hash(dir: &Path) -> anyhow::Result<String> {
    Ok(site_gen::sha256_hex(&std::fs::read(dir.join("manifest.json"))?))
}

fn verify_cmd(env: &Env, date: Option<&str>) -> Result<(), Fail> {
    let (date, dir) = pick_build(env, date)?;
    let _ = std::fs::remove_file(dir.join(VERIFIED_MARKER));
    let current = env.out().join("current");
    let previous = if current.exists() { Some(read_manifest(&current)?) } else { None };
    let failures = verify(
        &dir,
        previous.as_ref(),
        &env.control()?,
        &env.path(&env.cfg.paths.site).join("static/site.css"),
        Budgets::default(),
    );
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("  [{}] {}", f.check, f.detail);
        }
        return Err(Fail(
            3,
            anyhow!("verification of {date} failed: {} problem(s); not publishable", failures.len()),
        ));
    }
    std::fs::write(dir.join(VERIFIED_MARKER), manifest_hash(&dir)?)?;
    println!("verify {date}: ok");
    Ok(())
}

fn publish(env: &Env, date: Option<&str>, local: bool) -> Result<(), Fail> {
    let (date, dir) = pick_build(env, date)?;
    let marker = std::fs::read_to_string(dir.join(VERIFIED_MARKER)).unwrap_or_default();
    if marker.trim() != manifest_hash(&dir)? {
        return Err(Fail(3, anyhow!("build {date} is not verified; run `vitrina verify` first")));
    }
    if !local {
        let cmd = std::env::var("VITRINA_UPLOAD_CMD").map_err(|_| anyhow!("VITRINA_UPLOAD_CMD is not set (or pass --local)"))?;
        let site = std::fs::canonicalize(dir.join("site"))?;
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .env("VITRINA_SITE_DIR", &site)
            .env("VITRINA_BUILD_DATE", date.to_string())
            .status()
            .context("running VITRINA_UPLOAD_CMD")?;
        if !status.success() {
            return Err(Fail(
                4,
                anyhow!("upload of {date} failed ({status}); the previous version stays live"),
            ));
        }
    }
    switch_current(&env.out(), &date.to_string())?;
    println!(
        "publish {date}: {}",
        if local {
            "out/current switched (no upload)"
        } else {
            "uploaded, out/current switched"
        }
    );
    Ok(())
}

#[cfg(unix)]
fn switch_current(out: &Path, date: &str) -> anyhow::Result<()> {
    let tmp = out.join(".current-tmp");
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(date, &tmp)?;
    std::fs::rename(&tmp, out.join("current"))?;
    Ok(())
}

#[cfg(not(unix))]
fn switch_current(_out: &Path, _date: &str) -> anyhow::Result<()> {
    bail!("publishing needs a Unix host (atomic symlink switch)")
}

fn status(env: &Env) -> Result<(), Fail> {
    let store = env.open_db()?;
    match store.latest_ingest()? {
        Some(r) => println!(
            "last ingest {} (fetched {}, source {}): {} rows, {} rejected, {} in scope",
            r.date, r.fetched_at, r.source, r.rows_total, r.rows_rejected, r.rows_in_scope
        ),
        None => println!("no ingest yet"),
    }
    let products = store.all_products()?;
    for s in [ProductStatus::Active, ProductStatus::PendingLabel, ProductStatus::Delisted] {
        println!("{:>14}: {}", s.key(), products.iter().filter(|p| p.status == s).count());
    }
    let queue = store.pending_labels()?;
    println!("label queue: {}", queue.len());
    for (id, since) in queue.iter().take(50) {
        let name = products
            .iter()
            .find(|p| &p.iherb_id == id)
            .map(|p| format!("{} {}", p.brand, p.title))
            .unwrap_or_default();
        println!("  {id} since {since}  {name}");
    }
    println!(
        "events: {} buy clicks, {} error reports",
        store.count_clicks()?,
        store.count_reports()?
    );
    Ok(())
}
