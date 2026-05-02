//! Resolve manifest globs into a concrete list of source files.

use std::path::{Path, PathBuf};

use thiserror::Error;

use kiln_core::Manifest;

#[derive(Debug, Error)]
pub enum SourceSetError {
    #[error("invalid source glob `{glob}`: {source}")]
    InvalidGlob {
        glob: String,
        #[source]
        source: glob::PatternError,
    },

    #[error("error walking source glob `{glob}`: {source}")]
    WalkGlob {
        glob: String,
        #[source]
        source: glob::GlobError,
    },

    #[error("no source files matched the configured globs in `{root}`")]
    NoSources { root: PathBuf },
}

/// Resolved source files from a [`Manifest`]. Paths are absolute and
/// deduplicated; order is stable: the order globs appear in the manifest,
/// then alphabetical within each glob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSet {
    pub project_root: PathBuf,
    pub files: Vec<PathBuf>,
}

impl SourceSet {
    /// Resolve `manifest.design.sources` against `project_root`.
    pub fn resolve(project_root: &Path, manifest: &Manifest) -> Result<Self, SourceSetError> {
        let mut files: Vec<PathBuf> = Vec::new();
        let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

        for raw_glob in &manifest.design.sources {
            let pattern = if Path::new(raw_glob).is_absolute() {
                raw_glob.clone()
            } else {
                project_root.join(raw_glob).to_string_lossy().into_owned()
            };
            let entries = glob::glob(&pattern).map_err(|source| SourceSetError::InvalidGlob {
                glob: raw_glob.clone(),
                source,
            })?;
            let mut matched: Vec<PathBuf> = Vec::new();
            for entry in entries {
                let path = entry.map_err(|source| SourceSetError::WalkGlob {
                    glob: raw_glob.clone(),
                    source,
                })?;
                if path.is_file() {
                    let canonical = path.canonicalize().unwrap_or(path);
                    if seen.insert(canonical.clone()) {
                        matched.push(canonical);
                    }
                }
            }
            matched.sort();
            files.extend(matched);
        }

        if files.is_empty() {
            return Err(SourceSetError::NoSources {
                root: project_root.to_path_buf(),
            });
        }

        Ok(SourceSet {
            project_root: project_root.to_path_buf(),
            files,
        })
    }

    /// Returns the resolved files. Equivalent to `&self.files`.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(sources: &[&str]) -> Manifest {
        let mut sources_block = String::from("sources = [\n");
        for s in sources {
            sources_block.push_str(&format!("    \"{s}\",\n"));
        }
        sources_block.push(']');
        let toml_text = format!(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "demo"
            {sources_block}
            "#
        );
        toml_text.parse::<Manifest>().unwrap()
    }

    #[test]
    fn resolves_simple_glob() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.sv"), "module a; endmodule").unwrap();
        std::fs::write(src.join("b.sv"), "module b; endmodule").unwrap();
        let m = manifest(&["src/**/*.sv"]);
        let set = SourceSet::resolve(tmp.path(), &m).unwrap();
        assert_eq!(set.files.len(), 2);
        assert!(set.files.iter().all(|p| p.is_absolute()));
        let names: Vec<_> = set
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.sv", "b.sv"]);
    }

    #[test]
    fn dedupes_overlapping_globs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/x.sv"), "module x; endmodule").unwrap();
        std::fs::write(tmp.path().join("src/y.svh"), "// header").unwrap();
        let m = manifest(&["src/**/*.sv", "src/**/*.svh", "src/**/*.sv"]);
        let set = SourceSet::resolve(tmp.path(), &m).unwrap();
        assert_eq!(set.files.len(), 2, "duplicates should be filtered");
    }

    #[test]
    fn empty_match_errors() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let m = manifest(&["src/**/*.sv"]);
        let err = SourceSet::resolve(tmp.path(), &m).unwrap_err();
        assert!(matches!(err, SourceSetError::NoSources { .. }));
    }

    #[test]
    fn invalid_glob_pattern_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest(&["src/**/*.sv[", "src/**/*.svh"]);
        let err = SourceSet::resolve(tmp.path(), &m).unwrap_err();
        assert!(matches!(err, SourceSetError::InvalidGlob { .. }));
    }

    #[test]
    fn order_is_glob_order_then_alphabetical() {
        let tmp = tempfile::tempdir().unwrap();
        let inc = tmp.path().join("src/inc");
        std::fs::create_dir_all(&inc).unwrap();
        std::fs::write(tmp.path().join("src/zz.sv"), "").unwrap();
        std::fs::write(tmp.path().join("src/aa.sv"), "").unwrap();
        std::fs::write(inc.join("hdr.svh"), "").unwrap();
        let m = manifest(&["src/**/*.sv", "src/**/*.svh"]);
        let set = SourceSet::resolve(tmp.path(), &m).unwrap();
        let names: Vec<_> = set
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // sv files first (alphabetical within glob), then svh.
        assert_eq!(names, vec!["aa.sv", "zz.sv", "hdr.svh"]);
    }
}
