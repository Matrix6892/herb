//! Interface strings and locale formatting (spec §7.4).
//!
//! Strings come from `site/i18n/<locale>.toml`, "why here" phrases from
//! `site/templates/why/<key>.txt`. A key or placeholder that a page asks for
//! but the files lack is recorded, and the build fails on it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use catalog_core::{Date, ExactMass, Mass, Money, UnitPrice, UtcTimestamp};
use serde::Deserialize;

use crate::BuildError;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleSettings {
    pub code: String,
    pub lang: String,
    /// `ltr` or `rtl`.
    pub dir: String,
    pub decimal: String,
    pub group: String,
    pub currency_symbol: String,
    /// `prefix` or `suffix`.
    pub currency_position: String,
}

pub struct Tr {
    pub locale: LocaleSettings,
    strings: BTreeMap<String, String>,
    why: BTreeMap<String, String>,
    missing: RefCell<BTreeSet<String>>,
}

fn flatten(prefix: &str, table: &toml::Table, out: &mut BTreeMap<String, String>) -> Result<(), String> {
    for (k, v) in table {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            toml::Value::Table(t) => flatten(&key, t, out)?,
            _ => return Err(format!("`{key}` must be a string or a table")),
        }
    }
    Ok(())
}

impl Tr {
    pub fn load(site_dir: &Path, locale: &str) -> Result<Tr, BuildError> {
        let path = site_dir.join("i18n").join(format!("{locale}.toml"));
        let text = std::fs::read_to_string(&path).map_err(|e| BuildError::Input(format!("{}: {e}", path.display())))?;
        let mut table: toml::Table = toml::from_str(&text).map_err(|e| BuildError::Input(format!("{}: {e}", path.display())))?;
        let locale_value = table
            .remove("locale")
            .ok_or_else(|| BuildError::Input(format!("{}: missing [locale]", path.display())))?;
        let locale: LocaleSettings = locale_value
            .try_into()
            .map_err(|e| BuildError::Input(format!("{}: [locale]: {e}", path.display())))?;
        let mut strings = BTreeMap::new();
        flatten("", &table, &mut strings).map_err(|e| BuildError::Input(format!("{}: {e}", path.display())))?;

        let why_dir = site_dir.join("templates").join("why");
        let mut why = BTreeMap::new();
        let entries = std::fs::read_dir(&why_dir).map_err(|e| BuildError::Input(format!("{}: {e}", why_dir.display())))?;
        for entry in entries {
            let p = entry.map_err(|e| BuildError::Input(e.to_string()))?.path();
            if p.extension().is_some_and(|e| e == "txt") {
                let key = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let text = std::fs::read_to_string(&p).map_err(|e| BuildError::Input(format!("{}: {e}", p.display())))?;
                why.insert(key, text.trim().to_owned());
            }
        }
        Ok(Tr {
            locale,
            strings,
            why,
            missing: RefCell::new(BTreeSet::new()),
        })
    }

    /// The string for `key`, or the key itself (recorded as missing).
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        match self.strings.get(key) {
            Some(s) => s,
            None => {
                self.missing.borrow_mut().insert(key.to_owned());
                key
            }
        }
    }

    fn fill(&self, key: &str, template: &str, args: &[(&str, &str)]) -> String {
        let mut out = template.to_owned();
        for (name, value) in args {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        if let Some(start) = out.find('{')
            && let Some(len) = out[start..].find('}')
        {
            let placeholder = &out[start..=start + len];
            if placeholder[1..placeholder.len() - 1]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                self.missing.borrow_mut().insert(format!("{key} {placeholder}"));
            }
        }
        out
    }

    /// `key` with `{name}` placeholders filled.
    pub fn f(&self, key: &str, args: &[(&str, &str)]) -> String {
        let template = self.get(key).to_owned();
        self.fill(key, &template, args)
    }

    /// A "why here" phrase from `templates/why/<key>.txt`.
    pub fn why(&self, key: &str, args: &[(&str, &str)]) -> String {
        match self.why.get(key) {
            Some(t) => self.fill(key, t, args),
            None => {
                self.missing.borrow_mut().insert(format!("why/{key}.txt"));
                key.to_owned()
            }
        }
    }

    pub fn take_missing(&self) -> Vec<String> {
        std::mem::take(&mut *self.missing.borrow_mut()).into_iter().collect()
    }

    // ---- numbers -------------------------------------------------------

    fn group_digits(&self, digits: &str) -> String {
        let n = digits.len();
        let mut out = String::with_capacity(n + n / 3);
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (n - i).is_multiple_of(3) {
                out.push_str(&self.locale.group);
            }
            out.push(c);
        }
        out
    }

    /// Whole part grouped, fraction joined with the locale's separator.
    fn number(&self, whole: u128, frac: &str) -> String {
        let mut s = self.group_digits(&whole.to_string());
        if !frac.is_empty() {
            s.push_str(&self.locale.decimal);
            s.push_str(frac);
        }
        s
    }

    pub fn int(&self, v: u64) -> String {
        self.number(u128::from(v), "")
    }

    /// Fixed number of decimals. Rust's formatting rounds correctly and
    /// deterministically, which keeps builds byte-for-byte reproducible.
    pub fn decimal(&self, v: f64, digits: usize) -> String {
        let s = format!("{:.*}", digits, v.abs());
        let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
        let sign = if v < 0.0 && s.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
            "-"
        } else {
            ""
        };
        format!("{sign}{}", self.number(whole.parse().unwrap_or(0), frac))
    }

    fn currency(&self, amount: String) -> String {
        if self.locale.currency_position == "suffix" {
            format!("{amount} {}", self.locale.currency_symbol)
        } else {
            format!("{}{amount}", self.locale.currency_symbol)
        }
    }

    pub fn money(&self, m: Money) -> String {
        let abs = m.cents().unsigned_abs();
        let sign = if m.cents() < 0 { "-" } else { "" };
        let body = self.number(u128::from(abs / 100), &format!("{:02}", abs % 100));
        format!("{sign}{}", self.currency(body))
    }

    /// Dollars with three decimals: `$0.083`.
    pub fn unit_price(&self, p: UnitPrice) -> String {
        match p.round_dollars(3) {
            Some(v) => self.currency(self.number(v / 1000, &format!("{:03}", v % 1000))),
            None => "∞".to_owned(),
        }
    }

    /// `25 mcg`, `202.037 mg`, `24.245 g`: exact to the microgram below
    /// 10 g, then grams rounded to three decimals.
    pub fn mass(&self, m: Mass) -> String {
        let ug = u128::from(m.ug());
        if ug < 1_000 {
            return format!("{} mcg", self.number(ug, ""));
        }
        if ug < 10_000_000 {
            let frac = format!("{:03}", ug % 1_000);
            return format!("{} mg", self.number(ug / 1_000, frac.trim_end_matches('0')));
        }
        let milli_g = (ug + 500) / 1_000;
        let frac = format!("{:03}", milli_g % 1_000);
        format!("{} g", self.number(milli_g / 1_000, frac.trim_end_matches('0')))
    }

    /// An exact amount shown to the microgram; `≈` marks a fraction dropped.
    pub fn exact_mass(&self, m: ExactMass) -> String {
        match m.floor() {
            Ok(f) if m.is_exact() => self.mass(f),
            Ok(f) => format!("≈ {}", self.mass(f)),
            Err(_) => "∞".to_owned(),
        }
    }

    pub fn percent(&self, fraction: f64, digits: usize) -> String {
        format!("{}%", self.decimal(fraction * 100.0, digits))
    }

    pub fn date(&self, d: Date) -> String {
        d.to_string()
    }

    pub fn timestamp(&self, t: UtcTimestamp) -> String {
        t.to_string()
    }
}
