//! Register-only wide arithmetic and exact runtime-compatible conversions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arithmetic64 {
    SignedToDouble,
    UnsignedToDouble,
    DoubleToSigned,
    DoubleToUnsigned,
    SignedDivide,
    UnsignedDivide,
    SignedRemainder,
    UnsignedRemainder,
}

impl Arithmetic64 {
    pub fn unary(self) -> bool {
        matches!(
            self,
            Self::SignedToDouble
                | Self::UnsignedToDouble
                | Self::DoubleToSigned
                | Self::DoubleToUnsigned
        )
    }
    /// Match the runtime handlers, including divide-by-zero and MIN / -1.
    pub fn evaluate(self, dividend: u64, divisor: u64) -> u64 {
        match self {
            Self::SignedToDouble => return ((dividend as i64) as f64).to_bits(),
            Self::UnsignedToDouble => return (dividend as f64).to_bits(),
            Self::DoubleToSigned => return (f64::from_bits(dividend) as i64) as u64,
            Self::DoubleToUnsigned => {
                let value = f64::from_bits(dividend);
                return if value < 0.0 || !value.is_finite() {
                    0
                } else {
                    value as u64
                };
            }
            _ => {}
        }
        if divisor == 0 {
            return 0;
        }
        match self {
            Self::SignedDivide => (dividend as i64).wrapping_div(divisor as i64) as u64,
            Self::UnsignedDivide => dividend / divisor,
            Self::SignedRemainder => (dividend as i64).wrapping_rem(divisor as i64) as u64,
            Self::UnsignedRemainder => dividend % divisor,
            _ => unreachable!("conversions handled above"),
        }
    }
}
