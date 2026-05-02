// `WaveError` carries paths and captured stderr from the surfer invocation.
#![allow(clippy::result_large_err)]
//! Surfer waveform integration for `kiln`.
//!
//! `kiln test --trace` (and `kiln build --trace`) instruct Verilator to
//! dump FST. Traces land at `target/kiln/waves/<test>.fst`.
//! `kiln wave [<test>]` finds the right FST and shells out to surfer.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WaveError {
    #[error(
        "could not find `surfer` on PATH.\n\
         Install surfer (e.g. `brew install surfer-project/tap/surfer`, \
         or download a release from https://surfer-project.org)."
    )]
    SurferNotFound,

    #[error("no FST waves found under {0}. Run `kiln test --trace` first.")]
    NoWaves(PathBuf),

    #[error("no FST wave for test `{name}` at {path}")]
    MissingForTest { name: String, path: PathBuf },

    #[error("failed to invoke surfer at {path}: {source}")]
    Invocation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

const SURFER: &str = "surfer";

/// Directory where traces land.
pub fn wave_dir(project_root: &Path) -> PathBuf {
    project_root.join("target").join("kiln").join("waves")
}

/// FST path for a named test.
pub fn fst_path(project_root: &Path, test_name: &str) -> PathBuf {
    wave_dir(project_root).join(format!("{test_name}.fst"))
}

/// Find the most recent FST in `wave_dir`, by mtime.
pub fn most_recent_fst(project_root: &Path) -> Result<PathBuf, WaveError> {
    let dir = wave_dir(project_root);
    if !dir.is_dir() {
        return Err(WaveError::NoWaves(dir));
    }
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries = std::fs::read_dir(&dir).map_err(|source| WaveError::Io {
        path: dir.clone(),
        source,
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("fst") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &newest {
            None => newest = Some((mtime, path)),
            Some((m, _)) if mtime > *m => newest = Some((mtime, path)),
            _ => {}
        }
    }
    newest.map(|(_, p)| p).ok_or(WaveError::NoWaves(dir))
}

/// Open `fst_path` in surfer. Spawns and *detaches* — surfer is a GUI;
/// the kiln command should not block on it.
pub fn open(fst: &Path) -> Result<(), WaveError> {
    let bin = locate_surfer()?;
    std::process::Command::new(&bin)
        .arg(fst)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_child| ())
        .map_err(|source| WaveError::Invocation { path: bin, source })
}

fn locate_surfer() -> Result<PathBuf, WaveError> {
    let path_var = std::env::var_os("PATH").ok_or(WaveError::SurferNotFound)?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(SURFER);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(WaveError::SurferNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_recent_picks_newest_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = wave_dir(tmp.path());
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.fst");
        let b = dir.join("b.fst");
        std::fs::write(&a, "1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&b, "2").unwrap();
        let newest = most_recent_fst(tmp.path()).unwrap();
        assert_eq!(newest, b);
    }

    #[test]
    fn empty_dir_yields_no_waves() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(wave_dir(tmp.path())).unwrap();
        let err = most_recent_fst(tmp.path()).unwrap_err();
        assert!(matches!(err, WaveError::NoWaves(_)));
    }

    #[test]
    fn ignores_non_fst() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = wave_dir(tmp.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignore").unwrap();
        std::fs::write(dir.join("dump.vcd"), "ignore").unwrap();
        let err = most_recent_fst(tmp.path()).unwrap_err();
        assert!(matches!(err, WaveError::NoWaves(_)));
    }

    #[test]
    fn missing_surfer_message_has_install_hint() {
        let original = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", "/tmp/no/such/dir");
        }
        let err = locate_surfer().unwrap_err();
        if let Some(p) = original {
            unsafe {
                std::env::set_var("PATH", p);
            }
        }
        assert!(matches!(err, WaveError::SurferNotFound));
        let msg = err.to_string();
        assert!(msg.contains("surfer"));
        assert!(msg.contains("brew install") || msg.contains("surfer-project.org"));
    }
}
