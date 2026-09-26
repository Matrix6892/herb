// Shared helpers for integration tests: load the repository's data files.
#![allow(dead_code)]

use std::path::PathBuf;

use catalog_core::Reference;
use catalog_core::schema::{CategoriesFile, FormsFile, SubstancesFile};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn parse<T: serde::de::DeserializeOwned>(rel: &str) -> T {
    toml::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

pub fn reference() -> Reference {
    let substances: SubstancesFile = parse("data/reference/substances.toml");
    let forms: FormsFile = parse("data/reference/forms.toml");
    let categories: CategoriesFile = parse("data/reference/categories.toml");
    Reference::from_files(substances, forms, categories).expect("reference is valid")
}
