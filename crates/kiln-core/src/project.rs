//! Project-layout helpers: locating the manifest file from a working directory.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// The canonical manifest filename.
pub const MANIFEST_FILENAME: &str = "Kiln.toml";

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("no `Kiln.toml` found in `{0}` or any parent directory")]
    NotFound(PathBuf),
}

/// Walk upward from `start` looking for a `Kiln.toml`. Returns the path to
/// the manifest file (not the directory).
pub fn find_manifest(start: &Path) -> Result<PathBuf, ProjectError> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join(MANIFEST_FILENAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
        cur = dir.parent();
    }
    Err(ProjectError::NotFound(start.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_manifest_in_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), "").unwrap();
        let found = find_manifest(tmp.path()).unwrap();
        assert_eq!(found, tmp.path().join(MANIFEST_FILENAME));
    }

    #[test]
    fn finds_manifest_in_parent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), "").unwrap();
        let found = find_manifest(&nested).unwrap();
        assert_eq!(found, tmp.path().join(MANIFEST_FILENAME));
    }

    #[test]
    fn missing_manifest_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = find_manifest(tmp.path()).unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));
    }
}
