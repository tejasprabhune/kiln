//! Parse `bender sources --flatten` JSON into [`ResolvedSources`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::runner::BenderError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSources {
    pub packages: Vec<ResolvedPackage>,
}

impl ResolvedSources {
    /// Flatten every package's sources into a single ordered list.
    pub fn all_files(&self) -> Vec<PathBuf> {
        self.packages
            .iter()
            .flat_map(|p| p.files.iter().cloned())
            .collect()
    }

    /// Union of include directories across packages, deduped.
    pub fn all_include_dirs(&self) -> Vec<PathBuf> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for pkg in &self.packages {
            for d in &pkg.include_dirs {
                if seen.insert(d.clone()) {
                    out.push(d.clone());
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPackage {
    pub package: String,
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub include_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub defines: BTreeMap<String, String>,
}

/// Parse the JSON written by `bender sources --flatten`.
///
/// Bender emits a JSON array of one object per "source group", each with
/// `package`, `files`, `include_dirs`, `defines`, and other fields we
/// don't currently use. The schema is well-defined; we ignore unknown
/// fields rather than rejecting them, so a future bender minor doesn't
/// break parsing.
pub(crate) fn parse(json: &str) -> Result<ResolvedSources, BenderError> {
    let raw: Vec<RawPackage> = serde_json::from_str(json.trim())
        .map_err(|e| BenderError::ParseOutput(format!("bender sources JSON: {e}")))?;
    let packages = raw
        .into_iter()
        .map(|r| ResolvedPackage {
            package: r.package,
            files: r.files,
            include_dirs: r.include_dirs,
            defines: r.defines,
        })
        .collect();
    Ok(ResolvedSources { packages })
}

/// Subset of bender's per-source-group schema we care about.
#[derive(Debug, Deserialize)]
struct RawPackage {
    package: String,
    #[serde(default)]
    files: Vec<PathBuf>,
    #[serde(default)]
    include_dirs: Vec<PathBuf>,
    #[serde(default)]
    defines: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_package() {
        let json = r#"
            [
              {
                "package": "tester",
                "files": ["/a/foo.sv", "/a/bar.sv"],
                "include_dirs": ["/a/inc"],
                "defines": {"WIDTH": "8"},
                "target": "all(*, all(*))"
              }
            ]
        "#;
        let r = parse(json).unwrap();
        assert_eq!(r.packages.len(), 1);
        assert_eq!(r.packages[0].package, "tester");
        assert_eq!(r.packages[0].files.len(), 2);
        assert_eq!(r.packages[0].defines.get("WIDTH"), Some(&"8".to_string()));
    }

    #[test]
    fn parses_multi_package() {
        let json = r#"
            [
              {"package": "a", "files": ["/x/a.sv"], "include_dirs": ["/x/inc"], "defines": {}},
              {"package": "b", "files": ["/y/b.sv"], "include_dirs": ["/x/inc"], "defines": {}}
            ]
        "#;
        let r = parse(json).unwrap();
        assert_eq!(r.all_files().len(), 2);
        // Both packages share an include dir; should be deduped.
        assert_eq!(r.all_include_dirs(), vec![PathBuf::from("/x/inc")]);
    }

    #[test]
    fn handles_empty_array() {
        let r = parse("[]").unwrap();
        assert!(r.packages.is_empty());
        assert!(r.all_files().is_empty());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse("not json").is_err());
    }
}
