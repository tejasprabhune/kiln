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
//! cache, plan, and diagnostic shape) and executed. Exit code 0 is
//! success; anything else is failure.
//!
//! Cocotb is documented in the milestones doc but deliberately deferred
//! beyond M5: it requires a Python runtime and cocotb installed system-
//! wide, which we don't want to pin into CI without a clear ADR.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    /// Extra arguments appended to the simulation binary invocation.
    #[serde(default)]
    pub args: Vec<String>,
}

/// Discover testbenches. If the manifest specifies `design.test_sources`
/// globs, those are expanded; otherwise falls back to `tests/*.sv`.
///
/// Manifest `[[test.cases]]` entries are merged in: each case is emitted as
/// a separate `DiscoveredTest` with its own `name` and `args`, referencing the
/// compiled binary for `case.testbench`. Auto-discovered tests whose stem
/// matches a case's `testbench` are suppressed so they don't also run bare.
pub fn discover(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<Vec<DiscoveredTest>, TestError> {
    let mut base: Vec<DiscoveredTest> = if manifest.design.test_sources.is_empty() {
        discover_dir(&project_root.join("tests"))?
    } else {
        let mut out = Vec::new();
        for pattern in &manifest.design.test_sources {
            let full = project_root.join(pattern);
            let pattern_str = full.to_string_lossy().into_owned();
            let Ok(paths) = glob::glob(&pattern_str) else {
                continue;
            };
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
                        args: Vec::new(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.source == b.source);
        out
    };

    if manifest.test.cases.is_empty() {
        return Ok(base);
    }

    // Build a lookup: testbench stem -> source path from base discovery.
    let stem_to_source: std::collections::HashMap<String, PathBuf> = base
        .iter()
        .map(|t| (t.top.clone(), t.source.clone()))
        .collect();

    // Suppress auto-discovered tests that are used as a parameterized base.
    let used_as_base: std::collections::HashSet<String> = manifest
        .test
        .cases
        .iter()
        .map(|c| c.testbench.clone())
        .collect();
    base.retain(|t| !used_as_base.contains(&t.top));

    // Emit one DiscoveredTest per manifest case.
    for case in &manifest.test.cases {
        if let Some(source) = stem_to_source.get(&case.testbench) {
            base.push(DiscoveredTest {
                name: case.name.clone(),
                source: source.clone(),
                top: case.testbench.clone(),
                args: case.args.clone(),
            });
        }
    }

    Ok(base)
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
                args: Vec::new(),
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
    run_one_with_options(project_root, manifest, base_source_set, test, false, false)
}

/// Build and run one test with explicit options.
///
/// When `verbose` is true, stdout and stderr from the simulation binary are
/// streamed to the terminal in real time. The returned `TestOutcome` will have
/// empty `stdout`/`stderr` fields in that case — the output was already
/// printed. `verbose` must not be used with `jobs > 1` since the streams from
/// concurrent tests would interleave; the caller is responsible for enforcing
/// this.
pub fn run_one_with_options(
    project_root: &Path,
    manifest: &Manifest,
    base_source_set: &SourceSet,
    test: &DiscoveredTest,
    trace: bool,
    verbose: bool,
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
    let wave_dir = if trace {
        let dir = project_root.join("target").join("kiln").join("waves");
        std::fs::create_dir_all(&dir).map_err(|source| TestError::Io {
            path: dir.clone(),
            source,
        })?;
        Some(dir)
    } else {
        None
    };

    // Resolve the simulation working directory. `wave_dir` wins when tracing
    // (the binary must write its dump there); otherwise use `test.working_dir`
    // from the manifest (resolved relative to the project root) if set.
    let resolved_cwd = if trace {
        wave_dir.clone()
    } else {
        manifest
            .test
            .working_dir
            .as_ref()
            .map(|d| project_root.join(d))
    };

    if verbose {
        run_streaming(&binary, resolved_cwd.as_deref(), &test.args, &test.name, start)
    } else {
        run_buffered(&binary, resolved_cwd.as_deref(), &test.args, &test.name, start)
    }
}

fn make_cmd(binary: &Path, cwd: Option<&Path>, args: &[String]) -> Command {
    let mut cmd = Command::new(binary);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args);
    cmd
}

/// Capture all output, then return it in the outcome.
fn run_buffered(
    binary: &Path,
    cwd: Option<&Path>,
    args: &[String],
    name: &str,
    start: Instant,
) -> Result<TestOutcome, TestError> {
    let output = make_cmd(binary, cwd, args)
        .output()
        .map_err(|source| TestError::Io { path: binary.to_path_buf(), source })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let passed = output.status.success();
    Ok(TestOutcome { name: name.to_string(), passed, elapsed: start.elapsed(), stdout, stderr })
}

/// Stream stdout and stderr to the terminal line by line as the simulation
/// runs. The returned outcome has empty stdout/stderr — output was already
/// printed. Uses a background thread for stderr so both streams drain
/// concurrently without deadlocking.
fn run_streaming(
    binary: &Path,
    cwd: Option<&Path>,
    args: &[String],
    name: &str,
    start: Instant,
) -> Result<TestOutcome, TestError> {
    let mut child = make_cmd(binary, cwd, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| TestError::Io { path: binary.to_path_buf(), source })?;

    let stderr_pipe = child.stderr.take().expect("stderr piped");
    let stderr_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stderr_pipe);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("{line}");
        }
    });

    if let Some(stdout_pipe) = child.stdout.take() {
        let reader = BufReader::new(stdout_pipe);
        for line in reader.lines().map_while(Result::ok) {
            println!("{line}");
        }
    }

    let _ = stderr_thread.join();
    let status = child
        .wait()
        .map_err(|source| TestError::Io { path: binary.to_path_buf(), source })?;
    let passed = status.success();

    Ok(TestOutcome {
        name: name.to_string(),
        passed,
        elapsed: start.elapsed(),
        stdout: String::new(),
        stderr: String::new(),
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
    run_many_with_options(project_root, manifest, source_set, tests, jobs, false, false)
}

/// Run a slice of tests in parallel, with `trace` and `verbose` propagated to
/// each per-test build/run. Order of returned outcomes matches `tests`.
/// `verbose` must only be used with `jobs == 1`.
pub fn run_many_with_options(
    project_root: &Path,
    manifest: &Manifest,
    source_set: &SourceSet,
    tests: &[DiscoveredTest],
    jobs: usize,
    trace: bool,
    verbose: bool,
) -> Vec<Result<TestOutcome, TestError>> {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    // Pre-compile each unique testbench (by top name) sequentially to warm the
    // cache before parallel workers start. Without this, N workers that share
    // a testbench all try to compile it simultaneously and stomp on each other.
    {
        let mut seen: HashSet<String> = HashSet::new();
        for test in tests {
            if seen.insert(test.top.clone()) {
                let mut files = source_set.files.clone();
                let canon = test.source.canonicalize().unwrap_or(test.source.clone());
                if !files.contains(&canon) {
                    files.push(canon);
                }
                let ss = SourceSet { project_root: source_set.project_root.clone(), files };
                let mut mft = manifest.clone();
                mft.design.top = test.top.clone();
                let plan = BuildPlan::new(&mft, &ss, Profile::Debug).with_trace(trace);
                let _ = verilator::compile(&plan);
            }
        }
    }

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
                let r = run_one_with_options(
                    project_root,
                    manifest,
                    source_set,
                    &tests[idx],
                    trace,
                    verbose,
                );
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
    kiln_build::render::format_diagnostics(diags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_manifest() -> Manifest {
        r#"
        [package]
        name = "demo"
        version = "0.1.0"

        [design]
        top = "t"
        "#
        .parse()
        .unwrap()
    }

    #[test]
    fn discover_returns_empty_when_no_tests_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let m = base_manifest();
        assert!(discover(tmp.path(), &m).unwrap().is_empty());
    }

    #[test]
    fn discover_finds_sv_files_and_uses_stem_as_top() {
        let tmp = tempfile::tempdir().unwrap();
        let m = base_manifest();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join("tests/smoke.sv"), "module smoke; endmodule").unwrap();
        std::fs::write(
            tmp.path().join("tests/another.sv"),
            "module another; endmodule",
        )
        .unwrap();
        std::fs::write(tmp.path().join("tests/notes.txt"), "ignore me").unwrap();
        let found = discover(tmp.path(), &m).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "another");
        assert_eq!(found[0].top, "another");
        assert_eq!(found[1].name, "smoke");
    }

    #[test]
    fn discover_alphabetically_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let m = base_manifest();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        for n in ["zeta.sv", "alpha.sv", "mu.sv"] {
            std::fs::write(tmp.path().join("tests").join(n), "").unwrap();
        }
        let names: Vec<_> = discover(tmp.path(), &m)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }
}
