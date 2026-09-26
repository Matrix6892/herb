//! Price per effective unit (spec §5.2):
//!
//! ```text
//! P_unit = price / (servings_per_container × D_elemental × k_form)
//! ```
//!
//! expressed per category unit (for example per 100 mg of magnesium) and kept
//! as an exact fraction of cents.

use core::cmp::Ordering;
use core::fmt;

use crate::units::{Mass, Money, Ratio, UnitError, cmp_fractions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnitPriceError {
    #[error("price is not positive")]
    NonPositivePrice,
    #[error("no servings in the container")]
    NoServings,
    #[error("dose per serving is zero")]
    ZeroDose,
    #[error("k_form is zero")]
    ZeroK,
    #[error("arithmetic overflow")]
    Overflow,
}

/// Cents per category unit, as the exact fraction `num / den`.
#[derive(Debug, Clone, Copy)]
pub struct UnitPrice {
    num: u128,
    den: u128,
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl UnitPrice {
    fn reduced(num: u128, den: u128) -> UnitPrice {
        let g = gcd(num, den).max(1);
        UnitPrice {
            num: num / g,
            den: den / g,
        }
    }

    pub fn compute(
        price: Money,
        servings_per_container: u32,
        dose_per_serving: Mass,
        k_form: Ratio,
        unit: Mass,
    ) -> Result<UnitPrice, UnitPriceError> {
        if price.cents() <= 0 {
            return Err(UnitPriceError::NonPositivePrice);
        }
        if servings_per_container == 0 {
            return Err(UnitPriceError::NoServings);
        }
        if dose_per_serving == Mass::ZERO {
            return Err(UnitPriceError::ZeroDose);
        }
        if k_form.num() == 0 {
            return Err(UnitPriceError::ZeroK);
        }
        let cents = u128::try_from(price.cents()).map_err(|_| UnitPriceError::Overflow)?;
        // price × unit × k.den / (servings × dose × k.num)
        let num = cents
            .checked_mul(u128::from(unit.ug()))
            .and_then(|v| v.checked_mul(u128::from(k_form.den())))
            .ok_or(UnitPriceError::Overflow)?;
        let den = u128::from(servings_per_container)
            .checked_mul(u128::from(dose_per_serving.ug()))
            .and_then(|v| v.checked_mul(u128::from(k_form.num())))
            .ok_or(UnitPriceError::Overflow)?;
        Ok(UnitPrice::reduced(num, den))
    }

    /// Parses a dollar amount per unit such as `"0.06"`, for fixtures.
    pub fn from_dollars(s: &str) -> Result<UnitPrice, UnitError> {
        let (int, frac) = s.split_once('.').unwrap_or((s, ""));
        let digits = u32::try_from(frac.len()).map_err(|_| UnitError::BadNumber(s.into()))?;
        let mantissa: u128 = format!("{int}{frac}").parse().map_err(|_| UnitError::BadNumber(s.into()))?;
        // dollars = mantissa / 10^digits, cents = mantissa × 100 / 10^digits
        if mantissa == 0 {
            return Err(UnitError::OutOfRange(s.into()));
        }
        Ok(UnitPrice::reduced(mantissa * 100, 10u128.pow(digits)))
    }

    /// Cents per unit, for scoring and display only.
    #[allow(clippy::cast_precision_loss)]
    pub fn cents_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Value in units of `10^-decimals` dollars, rounded half up. `decimals =
    /// 3` gives tenths of a cent.
    pub fn round_dollars(self, decimals: u32) -> Option<u128> {
        // dollars × 10^d = cents × 10^d / 100
        let scale = 10u128.checked_pow(decimals)?;
        let n = self.num.checked_mul(scale)?;
        let d = self.den.checked_mul(100)?;
        let twice_d = d.checked_mul(2)?;
        n.checked_mul(2)?.checked_add(d).map(|v| v / twice_d)
    }
}

impl PartialEq for UnitPrice {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for UnitPrice {}

impl PartialOrd for UnitPrice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UnitPrice {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_fractions(self.num, self.den, other.num, other.den)
    }
}

impl fmt::Display for UnitPrice {
    /// `$0.083` (three decimals), for logs and tests.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.round_dollars(3) {
            Some(v) => write!(f, "${}.{:03}", v / 1000, v % 1000),
            None => f.write_str("$∞"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_per_100_mg() {
        // $15.00 for 90 servings of 200 mg: 18 g in total, $0.0833 per 100 mg.
        let p = UnitPrice::compute(
            Money::from_cents(1500),
            90,
            Mass::from_mg(200).unwrap(),
            Ratio::ONE,
            Mass::from_mg(100).unwrap(),
        )
        .unwrap();
        assert_eq!(p.to_string(), "$0.083");
        assert!((p.cents_f64() - 8.333_333).abs() < 1e-5);
    }

    #[test]
    fn orders_exactly() {
        let a = UnitPrice::from_dollars("0.06").unwrap();
        let b = UnitPrice::from_dollars("0.060").unwrap();
        let c = UnitPrice::from_dollars("0.061").unwrap();
        assert_eq!(a, b);
        assert!(a < c);
    }

    #[test]
    fn refuses_missing_inputs() {
        let unit = Mass::from_mg(100).unwrap();
        let dose = Mass::from_mg(200).unwrap();
        assert_eq!(
            UnitPrice::compute(Money::from_cents(0), 90, dose, Ratio::ONE, unit),
            Err(UnitPriceError::NonPositivePrice)
        );
        assert_eq!(
            UnitPrice::compute(Money::from_cents(100), 0, dose, Ratio::ONE, unit),
            Err(UnitPriceError::NoServings)
        );
        assert_eq!(
            UnitPrice::compute(Money::from_cents(100), 90, Mass::ZERO, Ratio::ONE, unit),
            Err(UnitPriceError::ZeroDose)
        );
    }
}
