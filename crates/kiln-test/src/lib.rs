// `RunError` carries the per-test invocation context.
#![allow(clippy::result_large_err)]
// The runner's `Vec<Option<Result<...>>>` shape is intentional: it lets
// the parallel scheduler write results back in input-order while
// preserving per-test errors.
#![allow(clippy::type_complexity)]
//! Test discovery and runner for `kiln`.
//!
//! Today: native SystemVerilog testbenches under `tests/*.sv`. Each
//! file's top module is the filename stem; the testbench is built
//! through `kiln-build`'s Verilator backend (so it reuses the same
//! cache, plan, and diagnostic shape) and executed. Exit code 0 plus
//! the literal token `"PASS"` on stdout is success; anything else is
//! failure.
//!
//! Cocotb is documented in the milestones doc but deliberately deferred
//! beyond M5: it requires a Python runtime and cocotb installed system-
//! wide, which we don't want to pin into CI without a clear ADR.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use kiln_build::backend::verilator;
use kiln_build::{BuildPlan, Profile, SourceSet};
use kiln_core::Manifest;

#[derive(Debug, Error)]
pub enum TestError {
    #[error(transparent)]
    SourceSet(#[from] kiln_build::SourceSetError),

    #[error(transparent)]
    Backend(#[from] kiln_build::BackendError),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One discovered native SystemVerilog testbench.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredTest {
    pub name: String,
    pub source: PathBuf,
    pub top: String,
}

/// Discover testbenches. If the manifest specifies `design.test_sources`
/// globs, those are expanded; otherwise falls back to `tests/*.sv`.
pub fn discover(project_root: &Path, manifest: &Manifest) -> Result<Vec<DiscoveredTest>, TestError> {
    if manifest.design.test_sources.is_empty() {
        discover_dir(&project_root.join("tests"))
    } else {
        let mut out = Vec::new();
        for pattern in &manifest.design.test_sources {
            let full = project_root.join(pattern);
            let pattern_str = full.to_string_lossy().into_owned();
            let Ok(paths) = glob::glob(&pattern_str) else { continue };
            for path in paths.flatten() {
                if path.extension().and_then(|s| s.to_str()) == Some("sv") {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("test")
                        .to_string();
                    out.push(DiscoveredTest {
                        name: stem.clone(),
                        source: path,
                        top: stem,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.source == b.source);
        Ok(out)
    }
}

fn discover_dir(dir: &Path) -> Result<Vec<DiscoveredTest>, TestError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|source| TestError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("sv") {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("test")
                .to_string();
            out.push(DiscoveredTest {
                name: stem.clone(),
                source: path,
                top: stem,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Outcome of running a single test.
#[derive(Debug, Clone)]
pub struct TestOutcome {
    pub name: String,
    pub passed: bool,
    pub elapsed: Duration,
    pub stdout: String,
    pub stderr: String,
}

/// Build and run one test. The build reuses `kiln-build`'s cache.
pub fn run_one(
    project_root: &Path,
    manifest: &Manifest,
    base_source_set: &SourceSet,
    test: &DiscoveredTest,
) -> Result<TestOutcome, TestError> {
    run_one_with_options(project_root, manifest, base_source_set, test, false)
}

/// Build and run one test with explicit options.
pub fn run_one_with_options(
    project_root: &Path,
    manifest: &Manifest,
    base_source_set: &SourceSet,
    test: &DiscoveredTest,
    trace: bool,
) -> Result<TestOutcome, TestError> {
    let start = Instant::now();

    // Construct a SourceSet that includes both the project's RTL and this
    // testbench file. Tests run with their own top, separate cache key.
    let mut files = base_source_set.files.clone();
    let canon = test.source.canonicalize().unwrap_or(test.source.clone());
    if !files.contains(&canon) {
        files.push(canon);
    }
    let source_set = SourceSet {
        project_root: base_source_set.project_root.clone(),
        files,
    };

    // Use a cloned manifest with the test's top so the cache key keys on it.
    let mut manifest_for_test = manifest.clone();
    manifest_for_test.design.top = test.top.clone();

    let plan = BuildPlan::new(&manifest_for_test, &source_set, Profile::Debug).with_trace(trace);
    let outcome = verilator::compile(&plan)?;
    let binary = match outcome.binary {
        Some(b) => b,
        None => {
            return Ok(TestOutcome {
                name: test.name.clone(),
                passed: false,
                elapsed: start.elapsed(),
                stdout: String::new(),
                stderr: format_diagnostics(&outcome.diagnostics),
            });
        }
    };

    // When tracing, run the binary in `<project>/target/kiln/waves/`
    // so its `$dumpfile("<top>.fst")` lands in the right place.
    let mut cmd = Command::new(&binary);
    if trace {
        let dir = project_root.join("target").join("kiln").join("waves");
        std::fs::create_dir_all(&dir).map_err(|source| TestError::Io {
            path: dir.clone(),
            source,
        })?;
        cmd.current_dir(&dir);
    } else {
        let _ = project_root;
    }
    let output = cmd.output().map_err(|source| TestError::Io {
        path: binary.clone(),
        source,
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let passed = output.status.success() && stdout.contains("PASS");

    Ok(TestOutcome {
        name: test.name.clone(),
        passed,
        elapsed: start.elapsed(),
        stdout,
        stderr,
    })
}

/// Run a slice of tests in parallel, up to `jobs` concurrent workers.
pub fn run_many(
    project_root: &Path,
    manifest: &Manifest,
    source_set: &SourceSet,
    tests: &[DiscoveredTest],
    jobs: usize,
) -> Vec<Result<TestOutcome, TestError>> {
    run_many_with_options(project_root, manifest, source_set, tests, jobs, false)
}

/// Run a slice of tests in parallel, with `trace` propagated to each
/// per-test build/run. Order of returned outcomes matches `tests`.
pub fn run_many_with_options(
    project_root: &Path,
    manifest: &Manifest,
    source_set: &SourceSet,
    tests: &[DiscoveredTest],
    jobs: usize,
    trace: bool,
) -> Vec<Result<TestOutcome, TestError>> {
    use std::sync::{Arc, Mutex};

    let next = Arc::new(Mutex::new(0usize));
    let results: Arc<Mutex<Vec<Option<Result<TestOutcome, TestError>>>>> =
        Arc::new(Mutex::new((0..tests.len()).map(|_| None).collect()));
    let workers = jobs.max(1).min(tests.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = Arc::clone(&next);
            let results = Arc::clone(&results);
            scope.spawn(move || loop {
                let idx = {
                    let mut g = next.lock().unwrap();
                    let i = *g;
                    if i >= tests.len() {
                        return;
                    }
                    *g += 1;
                    i
                };
                let r =
                    run_one_with_options(project_root, manifest, source_set, &tests[idx], trace);
                let mut g = results.lock().unwrap();
                g[idx] = Some(r);
            });
        }
    });

    let mut g = results.lock().unwrap();
    g.drain(..)
        .map(|o| o.expect("worker must produce"))
        .collect()
}

fn format_diagnostics(diags: &[kiln_build::BuildDiagnostic]) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for d in diags {
        let _ = writeln!(
            s,
            "{:?}: {} at {:?}:{:?}:{:?}",
            d.severity, d.message, d.file, d.line, d.column
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_returns_empty_when_no_tests_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(discover(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn discover_finds_sv_files_and_uses_stem_as_top() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join("tests/smoke.sv"), "module smoke; endmodule").unwrap();
        std::fs::write(
            tmp.path().join("tests/another.sv"),
            "module another; endmodule",
        )
        .unwrap();
        std::fs::write(tmp.path().join("tests/notes.txt"), "ignore me").unwrap();
        let found = discover(tmp.path()).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "another");
        assert_eq!(found[0].top, "another");
        assert_eq!(found[1].name, "smoke");
    }

    #[test]
    fn discover_alphabetically_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        for n in ["zeta.sv", "alpha.sv", "mu.sv"] {
            std::fs::write(tmp.path().join("tests").join(n), "").unwrap();
        }
        let names: Vec<_> = discover(tmp.path())
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }
}
