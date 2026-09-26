//! Checks run by `vitrina verify` before publication (spec §6). Any failure
//! blocks `publish`.

use std::io::Write;
use std::path::Path;

use catalog_core::control::BalanceFixture;

use crate::Manifest;

/// Size budgets of spec §7.2, in bytes. HTML is measured compressed, as it
/// travels; see docs/adr/0016-performance-budget-measured.md.
#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    pub category_html_gzip: u64,
    pub css: u64,
    pub js: u64,
    /// Largest allowed fall in products with a unit price, in percent.
    pub max_drop_percent: u64,
}

impl Default for Budgets {
    fn default() -> Budgets {
        Budgets {
            category_html_gzip: 60_000,
            css: 30_000,
            js: 10_000,
            max_drop_percent: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub check: &'static str,
    pub detail: String,
}

fn fail(check: &'static str, detail: impl Into<String>) -> Failure {
    Failure {
        check,
        detail: detail.into(),
    }
}

fn gzip_len(bytes: &[u8]) -> u64 {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    enc.write_all(bytes).expect("in-memory write");
    enc.finish().expect("in-memory write").len() as u64
}

/// Values of `href`, `src` and `action` attributes.
fn attribute_urls(html: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for attr in ["href=\"", "src=\"", "action=\""] {
        let mut rest = html;
        while let Some(i) = rest.find(attr) {
            let from = i + attr.len();
            let Some(len) = rest[from..].find('"') else { break };
            out.push(&rest[from..from + len]);
            rest = &rest[from + len..];
        }
    }
    out
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Runs every check against a written build `out/<date>/` and returns the
/// failures; an empty list means publishable.
pub fn verify(
    build_dir: &Path,
    previous: Option<&Manifest>,
    control: &BalanceFixture,
    css_source: &Path,
    budgets: Budgets,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let manifest: Manifest = match std::fs::read(build_dir.join("manifest.json"))
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(e) => return vec![fail("manifest", e)],
    };
    let site = build_dir.join("site");

    // 1. The feed did not silently lose products.
    if let Some(prev) = previous {
        let (before, now) = (prev.products_with_unit_price as u64, manifest.products_with_unit_price as u64);
        if before > 0 && now * 100 < before * (100 - budgets.max_drop_percent) {
            failures.push(fail(
                "drop",
                format!(
                    "products with a unit price fell from {before} ({}) to {now} ({})",
                    prev.date, manifest.date
                ),
            ));
        }
    }

    // 2. The control set still computes to the published numbers.
    if let Err(e) = control.check() {
        failures.push(fail("control", e.to_string()));
    }

    // 3. Files match the manifest, internal links resolve, store links go
    //    through the network, and every buy button carries its disclosure.
    for (rel, stat) in &manifest.files {
        let path = site.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            failures.push(fail("files", format!("{rel} is in the manifest but missing")));
            continue;
        };
        if bytes.len() as u64 != stat.bytes || crate::sha256_hex(&bytes) != stat.sha256 {
            failures.push(fail("files", format!("{rel} differs from the manifest")));
        }
        if !rel.ends_with(".html") {
            continue;
        }
        let html = String::from_utf8_lossy(&bytes);
        for url in attribute_urls(&html) {
            if let Some(host) = host_of(url) {
                let host = host.to_ascii_lowercase();
                if host == "iherb.com" || host.ends_with(".iherb.com") {
                    failures.push(fail(
                        "store_links",
                        format!("{rel}: direct store link {url}; use the tracking link (И3)"),
                    ));
                }
                continue;
            }
            if !url.starts_with('/') || url.starts_with("//") || url.starts_with("/api/") {
                continue;
            }
            let target = url.split(['?', '#']).next().unwrap_or(url);
            let file = if target.starts_with("/static/") {
                target.trim_start_matches('/').to_owned()
            } else {
                crate::file_for_path(target)
            };
            if !manifest.files.contains_key(&file) {
                failures.push(fail("links", format!("{rel}: {url} → {file} does not exist")));
            }
        }
        let buttons = html.matches("data-buy=").count();
        let disclosures = html.matches("class=\"disc\"").count();
        if buttons != disclosures {
            failures.push(fail(
                "disclosure",
                format!("{rel}: {buttons} buy buttons but {disclosures} disclosures (И4)"),
            ));
        }
        if html.contains("data-buy=") && !html.contains("rel=\"sponsored nofollow\"") {
            failures.push(fail("disclosure", format!("{rel}: buy link without rel=sponsored")));
        }
    }

    // 4. Size budgets (spec §7.2).
    for c in &manifest.categories {
        match std::fs::read(site.join(&c.file)) {
            Ok(bytes) => {
                let gz = gzip_len(&bytes);
                if gz > budgets.category_html_gzip {
                    failures.push(fail(
                        "budget",
                        format!("{}: {gz} bytes gzipped, budget {}", c.file, budgets.category_html_gzip),
                    ));
                }
            }
            Err(e) => failures.push(fail("budget", format!("{}: {e}", c.file))),
        }
    }
    match std::fs::metadata(css_source) {
        Ok(m) if m.len() > budgets.css => failures.push(fail("budget", format!("CSS is {} bytes, budget {}", m.len(), budgets.css))),
        Ok(_) => {}
        Err(e) => failures.push(fail("budget", format!("{}: {e}", css_source.display()))),
    }
    let js: u64 = manifest
        .files
        .iter()
        .filter(|(k, _)| k.ends_with(".js"))
        .map(|(_, s)| s.bytes)
        .sum();
    if js > budgets.js {
        failures.push(fail("budget", format!("JavaScript is {js} bytes, budget {}", budgets.js)));
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_attribute_urls() {
        let html = r#"<a href="/c/x">x</a><script src="/static/app.js?v=1"></script><form action="/api/report">"#;
        assert_eq!(attribute_urls(html), ["/c/x", "/static/app.js?v=1", "/api/report"]);
        assert_eq!(host_of("https://www.iherb.com/pr/a/1"), Some("www.iherb.com"));
        assert_eq!(host_of("/c/x"), None);
    }
}
