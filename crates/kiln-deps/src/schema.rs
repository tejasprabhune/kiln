//! `[dependencies]` schema and `Kiln.toml` editing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runner::BenderError;

/// One entry in `[dependencies]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Dependency {
    Git {
        git: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    Path {
        path: PathBuf,
    },
}

/// `BTreeMap<name, Dependency>`.
pub type DependencyTable = BTreeMap<String, Dependency>;

/// Parse the loosely-typed `[dependencies]` from the manifest into our
/// typed representation. Errors out for entries that aren't valid
/// git or path specs.
pub fn parse_dependencies(
    deps: &BTreeMap<String, toml::Value>,
) -> Result<DependencyTable, BenderError> {
    let mut out = DependencyTable::new();
    for (name, value) in deps {
        let dep: Dependency =
            value
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| BenderError::BadDependency {
                    name: name.clone(),
                    reason: e.to_string(),
                })?;
        out.insert(name.clone(), dep);
    }
    Ok(out)
}

/// Apply `f` to the `[dependencies]` table of the manifest at
/// `manifest_path`, preserving formatting / comments via `toml_edit`.
pub fn edit_manifest<F>(manifest_path: &Path, f: F) -> Result<(), BenderError>
where
    F: FnOnce(&mut toml_edit::Table),
{
    let text = std::fs::read_to_string(manifest_path).map_err(|source| BenderError::Io {
        path: manifest_path.to_path_buf(),
        source,
    })?;
    let mut doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|e: toml_edit::TomlError| BenderError::BadDependency {
                name: "<manifest>".to_string(),
                reason: e.to_string(),
            })?;
    let item = doc
        .entry("dependencies")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = item
        .as_table_mut()
        .ok_or_else(|| BenderError::BadDependency {
            name: "<manifest>".to_string(),
            reason: "[dependencies] is not a table".to_string(),
        })?;
    f(table);
    std::fs::write(manifest_path, doc.to_string()).map_err(|source| BenderError::Io {
        path: manifest_path.to_path_buf(),
        source,
    })?;
    Ok(())
}

pub fn insert_dependency(table: &mut toml_edit::Table, name: &str, dep: &Dependency) {
    let mut entry = toml_edit::InlineTable::new();
    match dep {
        Dependency::Git {
            git,
            version,
            rev,
            branch,
        } => {
            entry.insert("git", git.clone().into());
            if let Some(v) = version {
                entry.insert("version", v.clone().into());
            }
            if let Some(r) = rev {
                entry.insert("rev", r.clone().into());
            }
            if let Some(b) = branch {
                entry.insert("branch", b.clone().into());
            }
        }
        Dependency::Path { path } => {
            entry.insert("path", path.to_string_lossy().into_owned().into());
        }
    }
    table.insert(
        name,
        toml_edit::Item::Value(toml_edit::Value::InlineTable(entry)),
    );
}

pub fn remove_dependency(table: &mut toml_edit::Table, name: &str) {
    table.remove(name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_dep() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "local".to_string(),
            toml::Value::Table({
                let mut t = toml::value::Table::new();
                t.insert("path".to_string(), toml::Value::String("../local".into()));
                t
            }),
        );
        let parsed = parse_dependencies(&deps).unwrap();
        assert!(matches!(parsed.get("local"), Some(Dependency::Path { .. })));
    }

    #[test]
    fn parses_git_dep_with_version() {
        let toml_text = r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [dependencies]
            axi = { git = "https://example.com/axi.git", version = "0.39" }
        "#;
        let m: kiln_core::Manifest = toml_text.parse().unwrap();
        let parsed = parse_dependencies(&m.dependencies).unwrap();
        match parsed.get("axi") {
            Some(Dependency::Git { git, version, .. }) => {
                assert_eq!(git, "https://example.com/axi.git");
                assert_eq!(version.as_deref(), Some("0.39"));
            }
            other => panic!("expected git dep, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_dep_shape() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "weird".to_string(),
            toml::Value::Table({
                let mut t = toml::value::Table::new();
                t.insert("registry".to_string(), toml::Value::String("crates".into()));
                t
            }),
        );
        assert!(parse_dependencies(&deps).is_err());
    }

    #[test]
    fn edit_manifest_inserts_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Kiln.toml");
        std::fs::write(
            &p,
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\n\n[design]\ntop = \"t\"\n",
        )
        .unwrap();
        edit_manifest(&p, |t| {
            insert_dependency(
                t,
                "axi",
                &Dependency::Git {
                    git: "https://example.com/axi.git".into(),
                    version: Some("0.39".into()),
                    rev: None,
                    branch: None,
                },
            );
        })
        .unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(after.contains("[dependencies]") || after.contains("dependencies"));
        assert!(after.contains("axi"));
        assert!(after.contains("https://example.com/axi.git"));
    }

    #[test]
    fn edit_manifest_removes_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Kiln.toml");
        std::fs::write(
            &p,
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\n\n[design]\ntop = \"t\"\n\n[dependencies]\naxi = { git = \"u\" }\n",
        )
        .unwrap();
        edit_manifest(&p, |t| remove_dependency(t, "axi")).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(!after.contains("axi"));
    }
}
