//! Units of measure. Everything here is integer or rational; floating point
//! never appears in this module (spec §4.1).

use core::cmp::Ordering;
use core::fmt;

use crate::ids::SubstanceId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnitError {
    #[error("empty value")]
    Empty,
    #[error("`{0}` is not a non-negative decimal number")]
    BadNumber(String),
    #[error("unknown unit `{0}`")]
    UnknownUnit(String),
    #[error("expected `<number> <unit>`, got `{0}`")]
    BadShape(String),
    #[error("`{0}` is more precise than the storage unit allows")]
    TooPrecise(String),
    #[error("`{0}` is out of range")]
    OutOfRange(String),
    #[error("ratio denominator must be positive")]
    ZeroDenominator,
    #[error("`{0}` is not a ratio `num/den`")]
    BadRatio(String),
}

/// A non-negative decimal as an integer mantissa and the number of fractional
/// digits: `"1.25"` is `(125, 2)`.
fn parse_decimal(s: &str) -> Result<(u128, u32), UnitError> {
    if s.is_empty() {
        return Err(UnitError::Empty);
    }
    let bad = || UnitError::BadNumber(s.to_owned());
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    if s.contains('.') && (frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit())) {
        return Err(bad());
    }
    if int.len() + frac.len() > 30 {
        return Err(UnitError::OutOfRange(s.to_owned()));
    }
    let mut mantissa: u128 = 0;
    for b in int.bytes().chain(frac.bytes()) {
        mantissa = mantissa * 10 + u128::from(b - b'0');
    }
    let digits = u32::try_from(frac.len()).map_err(|_| bad())?;
    Ok((mantissa, digits))
}

/// `mantissa / 10^digits * factor`, exactly, or an error if a remainder is left.
fn scale_exact(original: &str, mantissa: u128, digits: u32, factor: u128) -> Result<u128, UnitError> {
    let pow = 10u128.pow(digits);
    let scaled = mantissa
        .checked_mul(factor)
        .ok_or_else(|| UnitError::OutOfRange(original.to_owned()))?;
    if scaled % pow != 0 {
        return Err(UnitError::TooPrecise(original.to_owned()));
    }
    Ok(scaled / pow)
}

fn split_value_unit(s: &str) -> Result<(&str, &str), UnitError> {
    let s = s.trim();
    let mut parts = s.split_whitespace();
    match (parts.next(), parts.next(), parts.next()) {
        (Some(n), Some(u), None) => Ok((n, u)),
        (None, _, _) => Err(UnitError::Empty),
        _ => Err(UnitError::BadShape(s.to_owned())),
    }
}

/// Mass in whole micrograms. 1 µg (vitamin B12) to kilograms of powder fit
/// without loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Mass(u64);

impl Mass {
    pub const ZERO: Mass = Mass(0);

    pub const fn from_ug(ug: u64) -> Mass {
        Mass(ug)
    }

    pub const fn from_mg(mg: u64) -> Option<Mass> {
        match mg.checked_mul(1_000) {
            Some(ug) => Some(Mass(ug)),
            None => None,
        }
    }

    pub const fn ug(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, other: Mass) -> Option<Mass> {
        self.0.checked_add(other.0).map(Mass)
    }

    pub fn checked_mul(self, n: u32) -> Option<Mass> {
        self.0.checked_mul(u64::from(n)).map(Mass)
    }

    /// Parses `"500 mg"`, `"1.5 g"`, `"25 mcg"`. Values finer than 1 µg are
    /// rejected rather than rounded.
    pub fn parse(s: &str) -> Result<Mass, UnitError> {
        let (num, unit) = split_value_unit(s)?;
        let factor: u128 = match unit {
            "mcg" | "ug" | "µg" | "μg" => 1,
            "mg" => 1_000,
            "g" => 1_000_000,
            other => return Err(UnitError::UnknownUnit(other.to_owned())),
        };
        let (mantissa, digits) = parse_decimal(num)?;
        let ug = scale_exact(s, mantissa, digits, factor)?;
        u64::try_from(ug).map(Mass).map_err(|_| UnitError::OutOfRange(s.to_owned()))
    }
}

impl fmt::Display for Mass {
    /// Canonical, locale-free form used in logs and tests: `140.96 mg`,
    /// `25 mcg`, `1.5 g`. Pages format through the locale instead.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ug = self.0;
        let (div, unit) = if ug >= 1_000_000 && ug.is_multiple_of(1_000) {
            (1_000_000, "g")
        } else if ug >= 1_000 {
            (1_000, "mg")
        } else {
            (1, "mcg")
        };
        write_decimal(f, u128::from(ug), div)?;
        write!(f, " {unit}")
    }
}

fn write_decimal(f: &mut fmt::Formatter<'_>, value: u128, div: u128) -> fmt::Result {
    let int = value / div;
    let rem = value % div;
    if rem == 0 {
        return write!(f, "{int}");
    }
    let width = div.ilog10() as usize;
    let frac = format!("{rem:0width$}");
    write!(f, "{int}.{}", frac.trim_end_matches('0'))
}

/// Money in US cents. Absence of a price is `Option<Money>`, never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i64);

impl Money {
    pub const fn from_cents(cents: i64) -> Money {
        Money(cents)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    /// Parses a dollar amount such as `"12.34"` or `"9"`. Sub-cent values are
    /// rejected.
    pub fn parse_dollars(s: &str) -> Result<Money, UnitError> {
        let s = s.trim();
        let (mantissa, digits) = parse_decimal(s)?;
        let cents = scale_exact(s, mantissa, digits, 100)?;
        i64::try_from(cents).map(Money).map_err(|_| UnitError::OutOfRange(s.to_owned()))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}${}.{:02}", abs / 100, abs % 100)
    }
}

/// International units. Deliberately has no conversion to `Mass`: the only
/// way to a mass is [`IuConversion`], which names its substance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Iu(u64);

impl Iu {
    pub const fn new(iu: u64) -> Iu {
        Iu(iu)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    /// Parses `"1000 IU"`. Whole units only.
    pub fn parse(s: &str) -> Result<Iu, UnitError> {
        let (num, unit) = split_value_unit(s)?;
        if unit != "IU" {
            return Err(UnitError::UnknownUnit(unit.to_owned()));
        }
        let (mantissa, digits) = parse_decimal(num)?;
        let v = scale_exact(s, mantissa, digits, 1)?;
        u64::try_from(v).map(Iu).map_err(|_| UnitError::OutOfRange(s.to_owned()))
    }
}

impl fmt::Display for Iu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} IU", self.0)
    }
}

/// A non-negative rational number, kept reduced so that equal values compare
/// equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ratio {
    num: u32,
    den: u32,
}

const fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

impl Ratio {
    pub const ONE: Ratio = Ratio { num: 1, den: 1 };

    pub fn new(num: u32, den: u32) -> Result<Ratio, UnitError> {
        if den == 0 {
            return Err(UnitError::ZeroDenominator);
        }
        let g = gcd_u128(u128::from(num), u128::from(den));
        let g = u32::try_from(g).expect("gcd of u32 values fits in u32");
        Ok(Ratio {
            num: num / g,
            den: den / g,
        })
    }

    /// Parses `"24305/40304"` or a whole number `"1"`.
    pub fn parse(s: &str) -> Result<Ratio, UnitError> {
        let bad = || UnitError::BadRatio(s.to_owned());
        let (n, d) = s.split_once('/').unwrap_or((s, "1"));
        let n: u32 = n.trim().parse().map_err(|_| bad())?;
        let d: u32 = d.trim().parse().map_err(|_| bad())?;
        Ratio::new(n, d)
    }

    pub const fn num(self) -> u32 {
        self.num
    }

    pub const fn den(self) -> u32 {
        self.den
    }

    pub fn is_at_most_one(self) -> bool {
        self.num <= self.den
    }

    /// For display only.
    pub fn to_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}

impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// An exact, possibly fractional mass in micrograms: `num / den` µg. Used
/// while summing label lines so that rounding happens once, at the end
/// (spec §5.1).
#[derive(Debug, Clone, Copy)]
pub struct ExactMass {
    num: u128,
    den: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("arithmetic overflow in exact mass")]
pub struct Overflow;

impl ExactMass {
    pub const ZERO: ExactMass = ExactMass { num: 0, den: 1 };

    pub fn from_mass(m: Mass) -> ExactMass {
        ExactMass {
            num: u128::from(m.ug()),
            den: 1,
        }
    }

    fn reduced(num: u128, den: u128) -> ExactMass {
        let g = gcd_u128(num, den).max(1);
        ExactMass {
            num: num / g,
            den: den / g,
        }
    }

    pub fn times(self, r: Ratio) -> Result<ExactMass, Overflow> {
        let num = self.num.checked_mul(u128::from(r.num())).ok_or(Overflow)?;
        let den = self.den.checked_mul(u128::from(r.den())).ok_or(Overflow)?;
        Ok(ExactMass::reduced(num, den))
    }

    pub fn plus(self, other: ExactMass) -> Result<ExactMass, Overflow> {
        let g = gcd_u128(self.den, other.den);
        let lcm = (self.den / g).checked_mul(other.den).ok_or(Overflow)?;
        let a = self.num.checked_mul(lcm / self.den).ok_or(Overflow)?;
        let b = other.num.checked_mul(lcm / other.den).ok_or(Overflow)?;
        Ok(ExactMass::reduced(a.checked_add(b).ok_or(Overflow)?, lcm))
    }

    /// Rounds down to a whole microgram.
    pub fn floor(self) -> Result<Mass, Overflow> {
        u64::try_from(self.num / self.den).map(Mass::from_ug).map_err(|_| Overflow)
    }

    pub fn is_exact(self) -> bool {
        self.den == 1
    }
}

impl PartialEq for ExactMass {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ExactMass {}

impl PartialOrd for ExactMass {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExactMass {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_fractions(self.num, self.den, other.num, other.den)
    }
}

/// Compares `a/b` with `c/d` exactly, without multiplying (so without
/// overflow), by walking the continued-fraction expansions.
pub(crate) fn cmp_fractions(mut a: u128, mut b: u128, mut c: u128, mut d: u128) -> Ordering {
    debug_assert!(b != 0 && d != 0);
    let mut flipped = false;
    loop {
        let (q1, r1) = (a / b, a % b);
        let (q2, r2) = (c / d, c % d);
        let ord = if q1 != q2 {
            Some(q1.cmp(&q2))
        } else {
            match (r1 == 0, r2 == 0) {
                (true, true) => Some(Ordering::Equal),
                (true, false) => Some(Ordering::Less),
                (false, true) => Some(Ordering::Greater),
                (false, false) => None,
            }
        };
        if let Some(o) = ord {
            return if flipped { o.reverse() } else { o };
        }
        // r1/b vs r2/d has the opposite order of b/r1 vs d/r2.
        (a, b, c, d) = (b, r1, d, r2);
        flipped = !flipped;
    }
}

/// Converts international units to mass for one substance (vitamin D:
/// 40 IU = 1 µg). It cannot be built without naming the substance, and dose
/// calculation refuses a conversion that belongs to another substance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IuConversion {
    substance: SubstanceId,
    ug_per_iu: Ratio,
}

impl IuConversion {
    pub fn new(substance: SubstanceId, ug_per_iu: Ratio) -> IuConversion {
        IuConversion { substance, ug_per_iu }
    }

    pub fn substance(&self) -> &SubstanceId {
        &self.substance
    }

    pub fn ug_per_iu(&self) -> Ratio {
        self.ug_per_iu
    }

    pub fn to_mass(&self, iu: Iu) -> Result<ExactMass, Overflow> {
        ExactMass {
            num: u128::from(iu.value()),
            den: 1,
        }
        .times(self.ug_per_iu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_masses_exactly() {
        assert_eq!(Mass::parse("500 mg"), Ok(Mass::from_ug(500_000)));
        assert_eq!(Mass::parse("1.5 g"), Ok(Mass::from_ug(1_500_000)));
        assert_eq!(Mass::parse("25 mcg"), Ok(Mass::from_ug(25)));
        assert_eq!(Mass::parse("2.4 µg").unwrap_err(), UnitError::TooPrecise("2.4 µg".into()));
        assert_eq!(Mass::parse("0.001 mg"), Ok(Mass::from_ug(1)));
        assert!(matches!(Mass::parse("10 kg"), Err(UnitError::UnknownUnit(_))));
        assert!(matches!(Mass::parse("1,000 mg"), Err(UnitError::BadNumber(_))));
        assert!(matches!(Mass::parse("-1 mg"), Err(UnitError::BadNumber(_))));
        assert!(matches!(Mass::parse("1. mg"), Err(UnitError::BadNumber(_))));
        assert!(matches!(Mass::parse("mg"), Err(UnitError::BadShape(_))));
    }

    #[test]
    fn displays_masses() {
        assert_eq!(Mass::from_ug(140_960).to_string(), "140.96 mg");
        assert_eq!(Mass::from_ug(25).to_string(), "25 mcg");
        assert_eq!(Mass::from_ug(1_500_000).to_string(), "1.5 g");
        assert_eq!(Mass::from_ug(1_500_001).to_string(), "1500.001 mg");
    }

    #[test]
    fn parses_money() {
        assert_eq!(Money::parse_dollars("12.34"), Ok(Money::from_cents(1234)));
        assert_eq!(Money::parse_dollars("9"), Ok(Money::from_cents(900)));
        assert_eq!(Money::parse_dollars("9.5"), Ok(Money::from_cents(950)));
        assert!(Money::parse_dollars("9.999").is_err());
        assert_eq!(Money::from_cents(1234).to_string(), "$12.34");
    }

    #[test]
    fn ratios_reduce() {
        assert_eq!(Ratio::parse("72915/451113").unwrap(), Ratio::new(24305, 150371).unwrap());
        assert_eq!(Ratio::parse("1").unwrap(), Ratio::ONE);
        assert!(Ratio::parse("1/0").is_err());
    }

    #[test]
    fn exact_mass_sums_then_floors_once() {
        // 1 µg × 1/3 three times is exactly 1 µg; rounding each term would give 0.
        let third = Ratio::new(1, 3).unwrap();
        let one = ExactMass::from_mass(Mass::from_ug(1)).times(third).unwrap();
        let sum = one.plus(one).unwrap().plus(one).unwrap();
        assert_eq!(sum.floor().unwrap(), Mass::from_ug(1));
        assert!(sum.is_exact());
    }

    #[test]
    fn compares_fractions_without_overflow() {
        let big = u128::MAX / 3;
        assert_eq!(cmp_fractions(big, big - 1, big - 1, big - 2), Ordering::Less);
        assert_eq!(cmp_fractions(2, 4, 1, 2), Ordering::Equal);
        assert_eq!(cmp_fractions(7, 3, 5, 2), Ordering::Less);
        assert_eq!(cmp_fractions(0, 3, 0, 5), Ordering::Equal);
    }

    #[test]
    fn iu_needs_a_substance() {
        let d = SubstanceId::new("vitamin_d").unwrap();
        let conv = IuConversion::new(d, Ratio::new(1, 40).unwrap());
        let m = conv.to_mass(Iu::new(1000)).unwrap();
        assert_eq!(m.floor().unwrap(), Mass::from_ug(25));
    }
}
