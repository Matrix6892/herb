//! Reading the hand-maintained files in `data/` for `vitrina import-labels`.

use std::path::{Path, PathBuf};

use catalog_core::schema::{CategoriesFile, FormsFile, LabelFile, SubstancesFile};
use catalog_core::{IherbId, Reference};

use crate::records::{LabelSet, ReferenceFiles};

#[derive(Debug, thiserror::Error)]
pub enum FilesError {
    #[error("{path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("{path}: {source}")]
    Toml { path: PathBuf, source: toml::de::Error },
    #[error("{path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, FilesError> {
    let text = std::fs::read_to_string(path).map_err(|source| FilesError::Read {
        path: path.to_owned(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| FilesError::Toml {
        path: path.to_owned(),
        source,
    })
}

/// Reads and validates `substances.toml`, `forms.toml` and
/// `categories.toml` from `dir`.
pub fn read_reference_dir(dir: &Path) -> Result<(ReferenceFiles, Reference), FilesError> {
    let files = ReferenceFiles {
        substances: read_toml::<SubstancesFile>(&dir.join("substances.toml"))?,
        forms: read_toml::<FormsFile>(&dir.join("forms.toml"))?,
        categories: read_toml::<CategoriesFile>(&dir.join("categories.toml"))?,
    };
    let reference = Reference::from_files(files.substances.clone(), files.forms.clone(), files.categories.clone()).map_err(|e| {
        FilesError::Invalid {
            path: dir.to_owned(),
            message: e.to_string(),
        }
    })?;
    Ok((files, reference))
}

/// Reads every `<iherb_id>.toml` in `dir`, in id order. The file name must
/// match the id inside. `label_ref` is the path as given, joined with the
/// file name, so that it reads `data/labels/<id>.toml` when `dir` is
/// `data/labels`.
pub fn read_labels_dir(dir: &Path, reference: &Reference) -> Result<Vec<LabelSet>, FilesError> {
    let entries = std::fs::read_dir(dir).map_err(|source| FilesError::Read {
        path: dir.to_owned(),
        source,
    })?;
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| FilesError::Read {
            path: dir.to_owned(),
            source,
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            paths.push(path);
        }
    }
    let mut sets = Vec::new();
    for path in paths {
        let invalid = |message: String| FilesError::Invalid {
            path: path.clone(),
            message,
        };
        let file: LabelFile = read_toml(&path)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if stem != file.iherb_id {
            return Err(invalid(format!("file name must be {}.toml", file.iherb_id)));
        }
        let product = IherbId::new(file.iherb_id.clone()).map_err(|e| invalid(e.to_string()))?;
        let labels = file.into_labels(reference).map_err(|e| invalid(e.to_string()))?;
        let label_ref = dir.join(format!("{product}.toml")).to_string_lossy().replace('\\', "/");
        sets.push(LabelSet {
            product,
            label_ref,
            labels,
        });
    }
    sets.sort_by(|a, b| a.product.cmp(&b.product));
    Ok(sets)
}
