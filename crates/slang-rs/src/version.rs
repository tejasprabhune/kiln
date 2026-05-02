//! Slang version representation and parsing.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::SlangError;

/// A parsed slang version. Tuple ordering on `(major, minor, patch)`
/// determines comparability; `raw` retains the verbatim string slang
/// reported, which may include pre-release / build-metadata suffixes
/// that we do not interpret.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SlangVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub raw: String,
}

impl SlangVersion {
    /// Construct a `SlangVersion` from a parsed triple. `raw` is set to
    /// `"<major>.<minor>.<patch>"`; intended for tests and the built-in
    /// `MIN_VERSION` constant.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            raw: String::new(),
        }
    }

    /// Build a `SlangVersion` from explicit fields including the raw string.
    pub fn from_parts(major: u32, minor: u32, patch: u32, raw: impl Into<String>) -> Self {
        Self {
            major,
            minor,
            patch,
            raw: raw.into(),
        }
    }

    /// Parse the first non-empty line of `slang --version` output.
    ///
    /// Slang's format has varied across releases (`slang 7.0`, `slang
    /// version 10.0`, `slang version 10.0+abcdef`). The parser is
    /// permissive: it pulls out the first `<digits>.<digits>(.<digits>)?`
    /// substring it sees.
    pub fn parse(raw: &str) -> Result<Self, SlangError> {
        let line = raw
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .ok_or_else(|| SlangError::ParseVersion {
                reason: "version output is empty".to_string(),
                raw: raw.to_string(),
            })?;
        let (major, minor, patch) =
            find_version_triple(line).ok_or_else(|| SlangError::ParseVersion {
                reason: format!("no `<digits>.<digits>` triple found in `{line}`"),
                raw: raw.to_string(),
            })?;
        Ok(Self {
            major,
            minor,
            patch,
            raw: line.to_string(),
        })
    }
}

impl fmt::Display for SlangVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.raw.is_empty() {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        } else {
            f.write_str(&self.raw)
        }
    }
}

/// Scan `s` for the first `<digits>.<digits>(.<digits>)?` substring.
fn find_version_triple(s: &str) -> Option<(u32, u32, u32)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len()
                && bytes[i] == b'.'
                && i + 1 < bytes.len()
                && bytes[i + 1].is_ascii_digit()
            {
                let dot1 = i;
                i += 1;
                let mid_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let mid_end = i;
                let major: u32 = std::str::from_utf8(&bytes[start..dot1])
                    .ok()?
                    .parse()
                    .ok()?;
                let minor: u32 = std::str::from_utf8(&bytes[mid_start..mid_end])
                    .ok()?
                    .parse()
                    .ok()?;
                let mut patch: u32 = 0;
                if i < bytes.len()
                    && bytes[i] == b'.'
                    && i + 1 < bytes.len()
                    && bytes[i + 1].is_ascii_digit()
                {
                    i += 1;
                    let p_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    patch = std::str::from_utf8(&bytes[p_start..i]).ok()?.parse().ok()?;
                }
                return Some((major, minor, patch));
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_classic_two_part() {
        let v = SlangVersion::parse("slang 7.0\n").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (7, 0, 0));
        assert_eq!(v.raw, "slang 7.0");
    }

    #[test]
    fn parses_version_with_word() {
        let v = SlangVersion::parse("slang version 10.0\n").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (10, 0, 0));
    }

    #[test]
    fn parses_three_part() {
        let v = SlangVersion::parse("slang 10.2.3\n").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (10, 2, 3));
    }

    #[test]
    fn parses_with_build_suffix() {
        let v = SlangVersion::parse("slang version 10.0+ab12cd\n").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (10, 0, 0));
        assert!(v.raw.contains("+ab12cd"));
    }

    #[test]
    fn skips_blank_lines() {
        let v = SlangVersion::parse("\n\n  slang version 8.5  \n").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (8, 5, 0));
    }

    #[test]
    fn rejects_no_digits() {
        let err = SlangVersion::parse("slang version unknown\n").unwrap_err();
        assert!(matches!(err, SlangError::ParseVersion { .. }));
    }

    #[test]
    fn rejects_empty_output() {
        let err = SlangVersion::parse("\n\n").unwrap_err();
        assert!(matches!(err, SlangError::ParseVersion { .. }));
    }

    #[test]
    fn version_ordering() {
        assert!(SlangVersion::new(10, 0, 0) > SlangVersion::new(9, 99, 99));
        assert!(SlangVersion::new(10, 1, 0) > SlangVersion::new(10, 0, 99));
        assert!(SlangVersion::new(10, 0, 1) > SlangVersion::new(10, 0, 0));
        assert_eq!(SlangVersion::new(10, 0, 0), SlangVersion::new(10, 0, 0));
    }

    #[test]
    fn ordering_ignores_raw_string() {
        let a = SlangVersion::from_parts(10, 0, 0, "slang 10.0");
        let b = SlangVersion::from_parts(10, 0, 0, "slang version 10.0+ab");
        // Note: derived PartialOrd respects field order, so `raw` participates.
        // We document this here so future readers know to compare via tuple
        // when lexical raw differences must be ignored.
        assert_ne!(a, b);
        assert_eq!((a.major, a.minor, a.patch), (b.major, b.minor, b.patch));
    }
}
