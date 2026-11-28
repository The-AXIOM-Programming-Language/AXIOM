//! Dimensional quantities for AXIOM's scientific runtime.
//!
//! Canonical magnitudes use milliseconds, millivolts, and nanoamperes as
//! the independent base units. Derived dimensions are represented algebraically.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dimension {
    /// Exponents of (time, voltage, current), respectively.
    pub exponents: [i8; 3],
}

impl Dimension {
    pub const DIMENSIONLESS: Self = Self { exponents: [0, 0, 0] };
    pub const TIME: Self = Self { exponents: [1, 0, 0] };
    pub const VOLTAGE: Self = Self { exponents: [0, 1, 0] };
    pub const CURRENT: Self = Self { exponents: [0, 0, 1] };

    fn combine(self, other: Self, sign: i8) -> Result<Self, UnitError> {
        let mut exponents = [0; 3];
        for (index, slot) in exponents.iter_mut().enumerate() {
            *slot = self.exponents[index]
                .checked_add(other.exponents[index].checked_mul(sign).ok_or(UnitError::DimensionOverflow)?)
                .ok_or(UnitError::DimensionOverflow)?;
        }
        Ok(Self { exponents })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    /// Magnitude in AXIOM's canonical base units (ms, mV, nA).
    pub magnitude: f64,
    pub dimension: Dimension,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    pub symbol: &'static str,
    pub scale: f64,
    pub dimension: Dimension,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitError {
    IncompatibleDimensions { left: Dimension, right: Dimension },
    DivisionByZero,
    NonFinite,
    DimensionOverflow,
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompatibleDimensions { left, right } =>
                write!(f, "incompatible dimensions {:?} and {:?}", left.exponents, right.exponents),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::NonFinite => write!(f, "quantity is not finite"),
            Self::DimensionOverflow => write!(f, "dimension exponent overflow"),
        }
    }
}

impl std::error::Error for UnitError {}

impl Unit {
    pub fn lookup(name: &str) -> Option<Self> {
        let (symbol, scale, dimension) = match name {
            "ms" => ("ms", 1.0, Dimension::TIME),
            "s" | "sec" | "second" => ("s", 1_000.0, Dimension::TIME),
            "us" => ("us", 0.001, Dimension::TIME),
            "mV" => ("mV", 1.0, Dimension::VOLTAGE),
            "V" | "volt" => ("V", 1_000.0, Dimension::VOLTAGE),
            "nA" => ("nA", 1.0, Dimension::CURRENT),
            "pA" => ("pA", 0.001, Dimension::CURRENT),
            "MOhm" | "megaohm" => ("MOhm", 1.0, Dimension { exponents: [0, 1, -1] }),
            "nF" => ("nF", 1.0, Dimension { exponents: [1, -1, 1] }),
            "Hz" | "hertz" => ("Hz", 0.001, Dimension { exponents: [-1, 0, 0] }),
            _ => return None,
        };
        Some(Self { symbol, scale, dimension })
    }
}

impl Quantity {
    pub fn scalar(value: f64) -> Result<Self, UnitError> {
        Self::new(value, Dimension::DIMENSIONLESS)
    }

    pub fn from_unit(value: f64, unit: Unit) -> Result<Self, UnitError> {
        Self::new(value * unit.scale, unit.dimension)
    }

    pub fn new(magnitude: f64, dimension: Dimension) -> Result<Self, UnitError> {
        if !magnitude.is_finite() {
            return Err(UnitError::NonFinite);
        }
        Ok(Self { magnitude, dimension })
    }

    pub fn add(self, rhs: Self) -> Result<Self, UnitError> {
        self.require_same_dimension(rhs)?;
        Self::new(self.magnitude + rhs.magnitude, self.dimension)
    }

    pub fn sub(self, rhs: Self) -> Result<Self, UnitError> {
        self.require_same_dimension(rhs)?;
        Self::new(self.magnitude - rhs.magnitude, self.dimension)
    }

    pub fn mul(self, rhs: Self) -> Result<Self, UnitError> {
        let dimension = self.dimension.combine(rhs.dimension, 1)?;
        Self::new(self.magnitude * rhs.magnitude, dimension)
    }

    pub fn div(self, rhs: Self) -> Result<Self, UnitError> {
        if rhs.magnitude == 0.0 {
            return Err(UnitError::DivisionByZero);
        }
        let dimension = self.dimension.combine(rhs.dimension, -1)?;
        Self::new(self.magnitude / rhs.magnitude, dimension)
    }

    fn require_same_dimension(self, rhs: Self) -> Result<(), UnitError> {
        if self.dimension != rhs.dimension {
            return Err(UnitError::IncompatibleDimensions {
                left: self.dimension,
                right: rhs.dimension,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(value: f64, unit: &str) -> Quantity {
        Quantity::from_unit(value, Unit::lookup(unit).unwrap()).unwrap()
    }

    #[test]
    fn converts_aliases_to_canonical_magnitudes() {
        assert_eq!(q(1.0, "s").magnitude, 1_000.0);
        assert_eq!(q(1.0, "V").magnitude, 1_000.0);
        assert_eq!(q(1.0, "pA").magnitude, 0.001);
    }

    #[test]
    fn rejects_addition_of_different_dimensions() {
        let error = q(1.0, "ms").add(q(1.0, "mV")).unwrap_err();
        assert!(matches!(error, UnitError::IncompatibleDimensions { .. }));
    }

    #[test]
    fn derived_dimensions_cancel_correctly() {
        let voltage = q(10.0, "MOhm").mul(q(2.0, "nA")).unwrap();
        assert_eq!(voltage.dimension, Dimension::VOLTAGE);
        assert_eq!(voltage.magnitude, 20.0);
    }

    #[test]
    fn frequency_uses_seconds_not_milliseconds_as_its_input_unit() {
        let frequency = q(10.0, "Hz");
        assert_eq!(frequency.magnitude, 0.01);
        assert_eq!(frequency.dimension, Dimension { exponents: [-1, 0, 0] });
    }
}
