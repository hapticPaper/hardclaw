//! $HCLAW token amounts with safe arithmetic.
//!
//! Uses 18 decimal places (like ETH) for precision.
//! All arithmetic operations are checked to prevent overflow.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub, Mul, Div};

/// Number of decimal places for HCLAW (10^18 units = 1 HCLAW)
pub const DECIMALS: u32 = 18;

/// One HCLAW in base units
pub const ONE_HCLAW: u128 = 10_u128.pow(DECIMALS);

/// Maximum supply (prevents overflow in calculations)
/// Set to 1 billion HCLAW
pub const MAX_SUPPLY: u128 = 1_000_000_000 * ONE_HCLAW;

/// A token amount in the smallest unit (similar to wei for ETH).
///
/// Internally stores value as u128 to support large amounts without overflow.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct HclawAmount(u128);

impl HclawAmount {
    /// Zero amount
    pub const ZERO: Self = Self(0);

    /// Create from raw base units
    #[must_use]
    pub const fn from_raw(raw: u128) -> Self {
        Self(raw)
    }

    /// Create from whole HCLAW (will be multiplied by 10^18)
    ///
    /// # Panics
    /// Panics if the amount would overflow
    #[must_use]
    pub const fn from_hclaw(hclaw: u64) -> Self {
        Self(hclaw as u128 * ONE_HCLAW)
    }

    /// Create from HCLAW with decimals (e.g., 1.5 HCLAW)
    ///
    /// # Errors
    /// Returns error if the string format is invalid
    pub fn from_decimal_str(s: &str) -> Result<Self, AmountError> {
        let parts: Vec<&str> = s.split('.').collect();

        if parts.len() > 2 {
            return Err(AmountError::InvalidFormat);
        }

        let whole: u128 = parts[0]
            .parse()
            .map_err(|_| AmountError::InvalidFormat)?;

        let fractional = if parts.len() == 2 {
            let frac_str = parts[1];
            if frac_str.len() > DECIMALS as usize {
                return Err(AmountError::TooManyDecimals);
            }

            // Pad with zeros to get the right precision
            let padded = format!("{:0<width$}", frac_str, width = DECIMALS as usize);
            padded[..DECIMALS as usize]
                .parse::<u128>()
                .map_err(|_| AmountError::InvalidFormat)?
        } else {
            0
        };

        let total = whole
            .checked_mul(ONE_HCLAW)
            .and_then(|w| w.checked_add(fractional))
            .ok_or(AmountError::Overflow)?;

        Ok(Self(total))
    }

    /// Get the raw base unit value
    #[must_use]
    pub const fn raw(&self) -> u128 {
        self.0
    }

    /// Get the whole HCLAW part (truncated)
    #[must_use]
    pub const fn whole_hclaw(&self) -> u64 {
        (self.0 / ONE_HCLAW) as u64
    }

    /// Convert to a decimal string representation
    #[must_use]
    pub fn to_decimal_string(&self) -> String {
        let whole = self.0 / ONE_HCLAW;
        let frac = self.0 % ONE_HCLAW;

        if frac == 0 {
            format!("{whole}.0")
        } else {
            let frac_str = format!("{frac:018}");
            let trimmed = frac_str.trim_end_matches('0');
            format!("{whole}.{trimmed}")
        }
    }

    /// Checked addition
    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction
    #[must_use]
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Checked multiplication
    #[must_use]
    pub fn checked_mul(self, factor: u128) -> Option<Self> {
        self.0.checked_mul(factor).map(Self)
    }

    /// Checked division
    #[must_use]
    pub fn checked_div(self, divisor: u128) -> Option<Self> {
        if divisor == 0 {
            None
        } else {
            Some(Self(self.0 / divisor))
        }
    }

    /// Calculate percentage (e.g., 95 = 95%)
    #[must_use]
    pub fn percentage(self, percent: u8) -> Self {
        Self(self.0 * u128::from(percent) / 100)
    }

    /// Saturating addition (caps at MAX_SUPPLY)
    #[must_use]
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0).min(MAX_SUPPLY))
    }

    /// Saturating subtraction (floors at 0)
    #[must_use]
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Check if amount is zero
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for HclawAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HclawAmount({})", self.to_decimal_string())
    }
}

impl fmt::Display for HclawAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} HCLAW", self.to_decimal_string())
    }
}

impl Add for HclawAmount {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.checked_add(other).expect("amount overflow")
    }
}

impl Sub for HclawAmount {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self.checked_sub(other).expect("amount underflow")
    }
}

impl Mul<u128> for HclawAmount {
    type Output = Self;

    fn mul(self, factor: u128) -> Self {
        self.checked_mul(factor).expect("amount overflow")
    }
}

impl Div<u128> for HclawAmount {
    type Output = Self;

    fn div(self, divisor: u128) -> Self {
        self.checked_div(divisor).expect("division by zero")
    }
}

/// Amount parsing/arithmetic errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum AmountError {
    /// Invalid number format
    #[error("invalid amount format")]
    InvalidFormat,
    /// Too many decimal places
    #[error("too many decimal places (max {DECIMALS})")]
    TooManyDecimals,
    /// Arithmetic overflow
    #[error("amount overflow")]
    Overflow,
    /// Insufficient balance
    #[error("insufficient balance: have {have}, need {need}")]
    Insufficient { have: HclawAmount, need: HclawAmount },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_hclaw() {
        let amount = HclawAmount::from_hclaw(100);
        assert_eq!(amount.whole_hclaw(), 100);
        assert_eq!(amount.raw(), 100 * ONE_HCLAW);
    }

    #[test]
    fn test_from_decimal_str() {
        let amount = HclawAmount::from_decimal_str("1.5").unwrap();
        assert_eq!(amount.raw(), ONE_HCLAW + ONE_HCLAW / 2);

        let amount = HclawAmount::from_decimal_str("0.001").unwrap();
        assert_eq!(amount.raw(), ONE_HCLAW / 1000);
    }

    #[test]
    fn test_to_decimal_string() {
        let amount = HclawAmount::from_hclaw(100);
        assert_eq!(amount.to_decimal_string(), "100.0");

        let amount = HclawAmount::from_raw(ONE_HCLAW + ONE_HCLAW / 2);
        assert_eq!(amount.to_decimal_string(), "1.5");
    }

    #[test]
    fn test_percentage() {
        let amount = HclawAmount::from_hclaw(100);

        assert_eq!(amount.percentage(95).whole_hclaw(), 95);
        assert_eq!(amount.percentage(4).whole_hclaw(), 4);
        assert_eq!(amount.percentage(1).whole_hclaw(), 1);
    }

    #[test]
    fn test_arithmetic() {
        let a = HclawAmount::from_hclaw(100);
        let b = HclawAmount::from_hclaw(50);

        assert_eq!((a + b).whole_hclaw(), 150);
        assert_eq!((a - b).whole_hclaw(), 50);
        assert_eq!((a * 2).whole_hclaw(), 200);
        assert_eq!((a / 2).whole_hclaw(), 50);
    }

    #[test]
    fn test_checked_arithmetic() {
        let a = HclawAmount::from_hclaw(100);
        let b = HclawAmount::from_hclaw(200);

        assert!(a.checked_sub(b).is_none());
        assert!(a.checked_add(b).is_some());
    }
}
