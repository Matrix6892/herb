//! The daily job end to end on the fixtures, through the real binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new(branch: &str) -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let r = repo();
        let config = format!(
            r#"rating_branch = "{branch}"
locale = "en"
site_url = "https://vitrina.example"
api_base = "/api"
[paths]
db = "var/vitrina.db"
snapshots = "var/snapshots"
out = "out"
reference = "{r}/data/reference"
labels = "{r}/data/fixtures/labels"
site = "{r}/site"
control = "{r}/data/fixtures/balance_abcdef.toml"
"#,
            r = r.display()
        );
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/vitrina.toml"), config).unwrap();
        Sandbox { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vitrina"));
        cmd.arg("--root")
            .arg(self.root())
            .args(args)
            .env_remove("VITRINA_FEED_URL")
            .env_remove("VITRINA_UPLOAD_CMD");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args, &[]);
        assert!(
            out.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn feed() -> String {
    repo().join("data/fixtures/feed/sample_feed.csv").display().to_string()
}

#[test]
fn two_days_ingest_build_verify_publish() {
    let s = Sandbox::new("A");
    for date in ["2026-09-25", "2026-09-26"] {
        let out = s.ok(&["ingest", "--feed-file", &feed(), "--date", date]);
        assert!(out.contains("17 rows, 3 rejected, 1 without price, 12 in scope"), "{out}");
        assert!(s.root().join(format!("var/snapshots/vitrina-{date}.db")).exists());
        s.ok(&["build"]);
        s.ok(&["verify"]);
        s.ok(&["publish", "--local"]);
        let current = std::fs::read_link(s.root().join("out/current")).unwrap();
        assert_eq!(current, PathBuf::from(date));
    }
    assert!(s.root().join("out/current/site/c/magnesium.html").exists());
    let status = s.ok(&["status"]);
    assert!(status.contains("label queue: 1"), "{status}");
}

#[test]
fn builds_from_the_same_snapshot_are_identical() {
    let s = Sandbox::new("B");
    s.ok(&["ingest", "--feed-file", &feed(), "--date", "2026-09-26"]);
    s.ok(&["build"]);
    let first = std::fs::read(s.root().join("out/2026-09-26/manifest.json")).unwrap();
    s.ok(&["build"]);
    let second = std::fs::read(s.root().join("out/2026-09-26/manifest.json")).unwrap();
    assert_eq!(first, second, "manifest (with every file's hash) must not change");
}

#[test]
fn feed_failures_exit_2_and_write_no_snapshot() {
    let s = Sandbox::new("B");
    let out = s.run(&["ingest", "--feed-file", "/nonexistent/feed.csv", "--date", "2026-09-26"], &[]);
    assert_eq!(out.status.code(), Some(2));
    let out = s.run(
        &["ingest", "--date", "2026-09-26"],
        &[("VITRINA_FEED_URL", "https://www.iherb.com/feed.csv")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("never reads iherb.com"));
    assert!(!s.root().join("var/snapshots/vitrina-2026-09-26.db").exists());
}

#[test]
fn unverified_or_broken_builds_are_not_published() {
    let s = Sandbox::new("B");
    s.ok(&["ingest", "--feed-file", &feed(), "--date", "2026-09-26"]);
    s.ok(&["build"]);
    let out = s.run(&["publish", "--local"], &[]);
    assert_eq!(out.status.code(), Some(3), "publish before verify");

    s.ok(&["verify"]);
    // Tamper with a page after verification: the manifest no longer matches.
    let page = s.root().join("out/2026-09-26/site/how.html");
    std::fs::write(&page, "<a href=\"/nowhere\">x</a>").unwrap();
    let out = s.run(&["verify"], &[]);
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("[files]") && err.contains("[links]"), "{err}");
    assert_eq!(s.run(&["publish", "--local"], &[]).status.code(), Some(3));
    assert!(!s.root().join("out/current").exists());
}

#[test]
fn failed_upload_keeps_the_previous_version() {
    let s = Sandbox::new("B");
    s.ok(&["ingest", "--feed-file", &feed(), "--date", "2026-09-25"]);
    s.ok(&["build"]);
    s.ok(&["verify"]);
    s.ok(&["publish", "--local"]);
    s.ok(&["ingest", "--feed-file", &feed(), "--date", "2026-09-26"]);
    s.ok(&["build"]);
    s.ok(&["verify"]);
    let out = s.run(&["publish"], &[("VITRINA_UPLOAD_CMD", "test -d \"$VITRINA_SITE_DIR\" && exit 7")]);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(
        std::fs::read_link(s.root().join("out/current")).unwrap(),
        PathBuf::from("2026-09-25")
    );
    let out = s.run(
        &["publish"],
        &[("VITRINA_UPLOAD_CMD", "test -f \"$VITRINA_SITE_DIR/c/magnesium.html\"")],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        std::fs::read_link(s.root().join("out/current")).unwrap(),
        PathBuf::from("2026-09-26")
    );
}
