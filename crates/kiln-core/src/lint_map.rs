//! Canonical lint rule table mapping kiln rule names to per-tool identifiers.

use crate::manifest::LintSeverity;
use strsim::jaro_winkler;

pub struct LintMapping {
    pub canonical: &'static str,
    pub slang_option: Option<&'static str>,
    pub verilator_code: Option<&'static str>,
    pub description: &'static str,
    pub default_severity: LintSeverity,
}

pub static CANONICAL_RULES: &[LintMapping] = &[
    LintMapping {
        canonical: "width-trunc",
        slang_option: Some("width-trunc"),
        verilator_code: Some("WIDTHTRUNC"),
        description: "Implicit truncation on assignment",
        default_severity: LintSeverity::Warn,
    },
    LintMapping {
        canonical: "case-incomplete",
        slang_option: Some("case-incomplete"),
        verilator_code: Some("CASEINCOMPLETE"),
        description: "case/casez/casex missing values",
        default_severity: LintSeverity::Warn,
    },
    LintMapping {
        canonical: "unused",
        slang_option: Some("unused"),
        verilator_code: Some("UNUSED"),
        description: "Unused variables or signals",
        default_severity: LintSeverity::Warn,
    },
    LintMapping {
        canonical: "implicit-net",
        slang_option: Some("implicit-net"),
        verilator_code: None,
        description: "Implicit net declaration",
        default_severity: LintSeverity::Warn,
    },
    LintMapping {
        canonical: "port-coercion",
        slang_option: Some("port-coercion"),
        verilator_code: None,
        description: "Port direction or type coercion",
        default_severity: LintSeverity::Warn,
    },
];

/// Returns the mapping for a canonical rule name, or None.
pub fn lookup(name: &str) -> Option<&'static LintMapping> {
    CANONICAL_RULES.iter().find(|m| m.canonical == name)
}

/// Returns a fuzzy-match suggestion for an unknown lint name.
/// Threshold: normalized Jaro-Winkler similarity >= 0.6.
pub fn suggest(unknown: &str) -> Option<&'static str> {
    CANONICAL_RULES
        .iter()
        .map(|m| (m.canonical, jaro_winkler(unknown, m.canonical)))
        .filter(|(_, score)| *score >= 0.6)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_rule() {
        let m = lookup("width-trunc").unwrap();
        assert_eq!(m.slang_option, Some("width-trunc"));
        assert_eq!(m.verilator_code, Some("WIDTHTRUNC"));
    }

    #[test]
    fn lookup_unknown_rule() {
        assert!(lookup("nonexistent").is_none());
    }

    #[test]
    fn suggest_close_match() {
        let s = suggest("width-trun");
        assert_eq!(s, Some("width-trunc"));
    }

    #[test]
    fn suggest_no_match() {
        assert!(suggest("zzzzzzz").is_none());
    }
}
