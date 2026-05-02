//! Typed wrapper over slang's `--diag-json` output.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SlangError;

/// A single diagnostic emitted by slang.
///
/// Schema observed in slang v10:
///
/// ```json
/// {
///   "severity": "error" | "warning" | "note",
///   "message": "...",
///   "optionName": "...",     // warnings; identifies the -W option
///   "location": "file:line:col",
///   "symbolPath": "..."      // optional
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// Slang's `-W<name>` knob this diagnostic was emitted under, when known.
    /// Maps onto what the milestones doc calls "diagnostic code".
    #[serde(
        default,
        rename = "optionName",
        skip_serializing_if = "Option::is_none"
    )]
    pub option_name: Option<String>,
    /// Source location, parsed from slang's `"file:line:col"` string.
    /// `None` for diagnostics that don't carry a position (e.g.
    /// "unknown warning option `-Wxyz`").
    #[serde(
        default,
        deserialize_with = "deserialize_location",
        skip_serializing_if = "Option::is_none"
    )]
    pub location: Option<Location>,
    #[serde(
        default,
        rename = "symbolPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub symbol_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// A source location parsed from slang's `"file:line:col"` strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
}

impl Location {
    pub fn parse(s: &str) -> Option<Self> {
        let mut iter = s.rsplitn(3, ':');
        let col: u32 = iter.next()?.parse().ok()?;
        let line: u32 = iter.next()?.parse().ok()?;
        let file_str = iter.next()?;
        Some(Location {
            file: PathBuf::from(file_str),
            line,
            column: col,
        })
    }
}

/// Permissive deserializer: accepts both the string `"file:line:col"`
/// (slang's actual output) and a structured object `{file, line, column}`
/// (so handcrafted fixtures and round-trips work too).
fn deserialize_location<'de, D>(de: D) -> Result<Option<Location>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Str(String),
        Obj(Location),
    }

    let opt: Option<Repr> = Option::deserialize(de)?;
    match opt {
        None => Ok(None),
        Some(Repr::Obj(loc)) => Ok(Some(loc)),
        Some(Repr::Str(s)) => Location::parse(&s)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("invalid location string: `{s}`"))),
    }
}

/// Parse the JSON body slang writes to its `--diag-json` file. The file is
/// always a JSON array (possibly empty).
pub(crate) fn parse_diagnostics(json: &str) -> Result<Vec<Diagnostic>, SlangError> {
    serde_json::from_str(json).map_err(|e| SlangError::ParseDiagnostics(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_array() {
        let diags = parse_diagnostics("[]").unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn parses_real_syntax_error_capture() {
        let json = include_str!("../tests/fixtures/captured/syntax_error.diag.json");
        let diags = parse_diagnostics(json).unwrap();
        assert_eq!(diags.len(), 4);
        assert!(diags.iter().all(|d| d.severity == Severity::Error));
        let first = &diags[0];
        assert_eq!(first.message, "expected ';'");
        let loc = first.location.as_ref().unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 11);
        assert_eq!(loc.file.to_string_lossy(), "syntax_error.sv");
    }

    #[test]
    fn parses_warning_with_option_name() {
        let json = include_str!("../tests/fixtures/captured/width_trunc.diag.json");
        let diags = parse_diagnostics(json).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].option_name.as_deref(), Some("width-trunc"));
        assert_eq!(diags[0].symbol_path.as_deref(), Some("width_demo"));
    }

    #[test]
    fn location_parser_round_trip() {
        let loc = Location::parse("path/to/foo.sv:42:7").unwrap();
        assert_eq!(loc.file.to_string_lossy(), "path/to/foo.sv");
        assert_eq!(loc.line, 42);
        assert_eq!(loc.column, 7);
    }

    #[test]
    fn location_parser_handles_windows_drives() {
        let loc = Location::parse("C:/work/foo.sv:1:2").unwrap();
        assert_eq!(loc.file.to_string_lossy(), "C:/work/foo.sv");
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 2);
    }

    #[test]
    fn location_parser_rejects_garbage() {
        assert!(Location::parse("not a location").is_none());
        assert!(Location::parse("file.sv:not:numeric").is_none());
    }

    #[test]
    fn rejects_invalid_json() {
        let err = parse_diagnostics("not json").unwrap_err();
        assert!(matches!(err, SlangError::ParseDiagnostics(_)));
    }
}
