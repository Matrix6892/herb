//! Validated identifiers. Slugs and ids from the feed become file paths in the
//! generated site, so they are restricted to path-safe characters here.

use core::cmp::Ordering;
use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind}: `{value}` ({rule})")]
pub struct IdError {
    pub kind: &'static str,
    pub value: String,
    pub rule: &'static str,
}

fn check(kind: &'static str, value: &str, max: usize, ok: impl Fn(u8) -> bool, rule: &'static str) -> Result<(), IdError> {
    if value.is_empty() || value.len() > max || !value.bytes().all(ok) {
        return Err(IdError {
            kind,
            value: value.to_owned(),
            rule,
        });
    }
    Ok(())
}

macro_rules! reference_id {
    ($(#[$doc:meta])* $name:ident, $kind:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                check($kind, &value, 64, |b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_', "lowercase ascii, digits and `_`")?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

reference_id!(
    /// `magnesium`, `omega_3`.
    SubstanceId,
    "substance id"
);
reference_id!(
    /// `magnesium_bisglycinate_anhydrous`.
    FormId,
    "form id"
);
reference_id!(
    /// `magnesium`.
    CategoryId,
    "category id"
);

/// The product id from the network feed, identical to the number in the
/// iHerb product path. Ordered numerically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IherbId(String);

impl IherbId {
    pub fn new(value: impl Into<String>) -> Result<IherbId, IdError> {
        let value = value.into();
        check("iherb id", &value, 20, |b| b.is_ascii_digit(), "digits only")?;
        if value.len() > 1 && value.starts_with('0') {
            return Err(IdError {
                kind: "iherb id",
                value,
                rule: "no leading zeros",
            });
        }
        Ok(IherbId(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Ord for IherbId {
    fn cmp(&self, other: &Self) -> Ordering {
        // Without leading zeros, a shorter digit string is a smaller number.
        self.0.len().cmp(&other.0.len()).then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for IherbId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for IherbId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A URL path segment copied unchanged from the feed so that swapping the
/// domain in an iHerb link lands on the same page here (spec §7.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

impl Slug {
    pub fn new(value: impl Into<String>) -> Result<Slug, IdError> {
        let value = value.into();
        check(
            "slug",
            &value,
            200,
            |b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_',
            "ascii letters, digits, `-` and `_`",
        )?;
        Ok(Slug(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iherb_ids_order_numerically() {
        let nine = IherbId::new("9").unwrap();
        let ten = IherbId::new("10").unwrap();
        assert!(nine < ten);
        assert!(IherbId::new("012").is_err());
        assert!(IherbId::new("12a").is_err());
        assert!(IherbId::new("").is_err());
    }

    #[test]
    fn slugs_are_path_safe() {
        assert!(Slug::new("now-foods-magnesium-glycinate-180-tablets").is_ok());
        assert!(Slug::new("../etc").is_err());
        assert!(Slug::new("a/b").is_err());
        assert!(Slug::new("a b").is_err());
    }
}
