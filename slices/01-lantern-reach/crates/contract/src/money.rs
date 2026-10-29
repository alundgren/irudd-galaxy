//! Money. The sim keeps credits and every price as whole **centicredits** (1 cr = 100),
//! so totals are exact and hash the same everywhere (`content/08-contract.md`,
//! "Numbers"). Data files write ordinary decimal credits; the loader converts them.

/// Centicredits per credit.
pub const CENTS_PER_CREDIT: i64 = 100;

/// Decimal credits from a data file as centicredits: `None` unless the value is finite
/// and a whole number of centicredits (`14`, `0.15`, not `0.155`).
pub fn cents_from_credits(credits: f64) -> Option<i64> {
    if !credits.is_finite() {
        return None;
    }
    let cents = credits * CENTS_PER_CREDIT as f64;
    let whole = cents.round();
    ((cents - whole).abs() < 1e-6 && whole.abs() < 9.0e15).then_some(whole as i64)
}

/// Centicredits as credits for summaries and messages: `40`, `9.4`, `-0.05`.
pub fn show_credits(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let (whole, frac) = (abs / 100, abs % 100);
    if frac == 0 {
        format!("{sign}{whole}")
    } else if frac % 10 == 0 {
        format!("{sign}{whole}.{}", frac / 10)
    } else {
        format!("{sign}{whole}.{frac:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_and_shows() {
        assert_eq!(cents_from_credits(14.0), Some(1400));
        assert_eq!(cents_from_credits(14.3), Some(1430));
        assert_eq!(cents_from_credits(0.155), None);
        assert_eq!(cents_from_credits(f64::NAN), None);
        assert_eq!(show_credits(4000), "40");
        assert_eq!(show_credits(94067), "940.67");
        assert_eq!(show_credits(1430), "14.3");
        assert_eq!(show_credits(-5), "-0.05");
    }
}
