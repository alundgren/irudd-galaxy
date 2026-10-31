//! Engine and module versions: `MAJOR.MINOR.PATCH`, compared number by number.

use std::fmt;
use std::str::FromStr;

/// A version. `MAJOR.MINOR` is accepted and means `MAJOR.MINOR.0`; pre-release and build
/// suffixes are not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// This engine's version: the engine workspace's package version.
pub fn engine() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the workspace version is MAJOR.MINOR.PATCH")
}

impl FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let bad = || format!("`{text}` is not a version: write MAJOR.MINOR.PATCH, such as 0.1.0");
        let parts: Vec<&str> = text.split('.').collect();
        if !(2..=3).contains(&parts.len()) {
            return Err(bad());
        }
        let mut numbers = [0u32; 3];
        for (number, part) in numbers.iter_mut().zip(&parts) {
            let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            if !digits || (part.len() > 1 && part.starts_with('0')) {
                return Err(bad());
            }
            *number = part.parse().map_err(|_| bad())?;
        }
        Ok(Version {
            major: numbers[0],
            minor: numbers[1],
            patch: numbers[2],
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn parses_and_orders_numerically() {
        assert_eq!(v("0.4"), v("0.4.0"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.1.1") > v("0.1.0"));
    }

    #[test]
    fn rejects_what_is_not_a_plain_version() {
        for bad in [
            "",
            "1",
            "1.2.3.4",
            "1.x.0",
            "01.2.3",
            "1.2.3-dev",
            " 1.2.3",
            "1..3",
            "v1.2.3",
        ] {
            assert!(
                bad.parse::<Version>().is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn the_engine_version_parses() {
        let _ = super::engine();
    }
}
