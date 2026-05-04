// `RunError` carries the per-test invocation context.
#![allow(clippy::result_large_err)]
// The runner's `Vec<Option<Result<...>>>` shape is intentional: it lets
// the parallel scheduler write results back in input-order while
// preserving per-test errors.
#![allow(clippy::type_complexity)]
//! Test discovery and runner for `kiln`.
//!
//! Today: native SystemVerilog testbenches under `tests/*.sv` (or globs
//! from `[design] test_sources`). Each file's top module is the
//! filename stem; the testbench is built through `kiln-build`'s
//! Verilator backend (so it reuses the same cache, plan, and
//! diagnostic shape) and executed.
//!
//! Pass/fail is determined by the per-test [`Detect`] rule. Default is
//! exit-code; testbenches that always `$finish()` cleanly can opt into
//! stdout-pattern detection so kiln correctly flags failures even when
//! the simulator exits 0.
//!
//! Cocotb is documented in the milestones doc but deliberately deferred
//! beyond M5: it requires a Python runtime and cocotb installed system-
//! wide, which we don't want to pin into CI without a clear ADR.

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use kiln_build::backend::verilator;
use kiln_build::{BuildPlan, Profile, SourceSet};
use kiln_core::{Detect, Manifest, TestMatrix};

#[derive(Debug, Error)]
pub enum TestError {
    #[error(transparent)]
    SourceSet(#[from] kiln_build::SourceSetError),

    #[error(transparent)]
    Backend(#[from] kiln_build::BackendError),

    #[error("prebuild command `{cmd}` failed (exit {code}). stderr:\n{stderr}")]
    Prebuild {
        cmd: String,
        code: i32,
        stderr: String,
    },

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Outcome status for a single test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Detection rule matched success.
    Pass,
    /// Detection rule matched failure or exit code != 0.
    Fail,
    /// Wallclock exceeded the configured per-test timeout.
    Timeout,
}

impl Status {
    pub fn is_pass(self) -> bool {
        matches!(self, Status::Pass)
    }
}

/// One discovered native SystemVerilog testbench (or a parameterized
/// case generated from `[[test.cases]]` / `[[test.matrix]]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredTest {
    pub name: String,
    pub source: PathBuf,
    pub top: String,
    /// Extra arguments appended to the simulation binary invocation.
    #[serde(default)]
    pub args: Vec<String>,
    /// Detection rule.
    #[serde(default)]
    pub detect: Detect,
    /// Wallclock timeout. `None` = no timeout.
    #[serde(default)]
    pub timeout: Option<Duration>,
    /// Optional shell command run before the simulator. The runner
    /// dedupes by command string across a single `run_many*` call.
    #[serde(default)]
    pub prebuild: Option<String>,
    /// Tags for `--tag` selection.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Per-case working-directory override. Falls back to
    /// `manifest.test.working_dir`, then to the project root.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

/// Discover testbenches.
///
/// Discovery sources, in order:
/// 1. `manifest.design.test_sources` globs (or `tests/*.sv` if empty).
/// 2. `[[test.cases]]` entries — explicit named cases that reuse a
///    discovered testbench's binary with their own args/detect/etc.
/// 3. `[[test.matrix]]` entries — glob-driven parameterized cases.
///
/// Discovered testbenches whose stem matches a base for a case or
/// matrix are *suppressed* from the bare list so they don't run twice.
pub fn discover(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<Vec<DiscoveredTest>, TestError> {
    let default_detect = manifest
        .test
        .detect
        .clone()
        .unwrap_or(Detect::ExitCode);
    let default_timeout = manifest.test.timeout.map(|d| d.as_duration());

    let mut base: Vec<DiscoveredTest> = if manifest.design.test_sources.is_empty() {
        discover_dir(&project_root.join("tests"), &default_detect, default_timeout)?
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
                        detect: default_detect.clone(),
                        timeout: default_timeout,
                        prebuild: None,
                        tags: Vec::new(),
                        working_dir: None,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.source == b.source);
        out
    };

    // Build a lookup: testbench stem -> source path from base discovery.
    let stem_to_source: BTreeMap<String, PathBuf> = base
        .iter()
        .map(|t| (t.top.clone(), t.source.clone()))
        .collect();

    // Suppress auto-discovered tests that are used as a parameterized base.
    let mut used_as_base: HashSet<String> = HashSet::new();
    for c in &manifest.test.cases {
        used_as_base.insert(c.testbench.clone());
    }
    for m in &manifest.test.matrix {
        used_as_base.insert(m.testbench.clone());
    }
    base.retain(|t| !used_as_base.contains(&t.top));

    // Emit one DiscoveredTest per manifest case.
    for case in &manifest.test.cases {
        let Some(source) = stem_to_source.get(&case.testbench) else {
            continue;
        };
        let detect = case
            .detect
            .clone()
            .unwrap_or_else(|| default_detect.clone());
        let timeout = case
            .timeout
            .map(|d| d.as_duration())
            .or(default_timeout);
        base.push(DiscoveredTest {
            name: case.name.clone(),
            source: source.clone(),
            top: case.testbench.clone(),
            args: case.args.clone(),
            detect,
            timeout,
            prebuild: case.prebuild.clone(),
            tags: case.tags.clone(),
            working_dir: case.working_dir.clone(),
        });
    }

    // Expand each [[test.matrix]] entry over its glob.
    for matrix in &manifest.test.matrix {
        expand_matrix(
            project_root,
            matrix,
            &stem_to_source,
            &default_detect,
            default_timeout,
            &mut base,
        );
    }

    base.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(base)
}

fn expand_matrix(
    project_root: &Path,
    matrix: &TestMatrix,
    stem_to_source: &BTreeMap<String, PathBuf>,
    default_detect: &Detect,
    default_timeout: Option<Duration>,
    out: &mut Vec<DiscoveredTest>,
) {
    let Some(source) = stem_to_source.get(&matrix.testbench) else {
        return;
    };
    let pattern = project_root.join(&matrix.inputs);
    let pattern_str = pattern.to_string_lossy().into_owned();
    let Ok(paths) = glob::glob(&pattern_str) else {
        return;
    };
    let detect = matrix
        .detect
        .clone()
        .unwrap_or_else(|| default_detect.clone());
    let timeout = matrix
        .timeout
        .map(|d| d.as_duration())
        .or(default_timeout);
    let mut matches: Vec<PathBuf> = paths.flatten().collect();
    matches.sort();
    for path in matches {
        let abs = path.canonicalize().unwrap_or(path.clone());
        let rel = abs.strip_prefix(project_root).unwrap_or(&abs);
        let stem = abs
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let name = abs
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let parent = rel
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let subs: BTreeMap<&str, String> = {
            let mut m = BTreeMap::new();
            m.insert("stem", stem.clone());
            m.insert("name", name.clone());
            m.insert("path", rel.to_string_lossy().into_owned());
            m.insert("abs_path", abs.to_string_lossy().into_owned());
            m.insert("parent", parent);
            m
        };
        let test_name = format!("{}{}", matrix.name_prefix, stem);
        let args: Vec<String> = matrix.args.iter().map(|a| substitute(a, &subs)).collect();
        let prebuild = matrix.prebuild.as_deref().map(|p| substitute(p, &subs));
        out.push(DiscoveredTest {
            name: test_name,
            source: source.clone(),
            top: matrix.testbench.clone(),
            args,
            detect: detect.clone(),
            timeout,
            prebuild,
            tags: matrix.tags.clone(),
            working_dir: matrix.working_dir.clone(),
        });
    }
}

fn substitute(template: &str, subs: &BTreeMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut key = String::new();
            let mut closed = false;
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    closed = true;
                    break;
                }
                key.push(nc);
                chars.next();
            }
            if closed {
                if let Some(v) = subs.get(key.as_str()) {
                    out.push_str(v);
                } else {
                    out.push('{');
                    out.push_str(&key);
                    out.push('}');
                }
            } else {
                out.push('{');
                out.push_str(&key);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn discover_dir(
    dir: &Path,
    default_detect: &Detect,
    default_timeout: Option<Duration>,
) -> Result<Vec<DiscoveredTest>, TestError> {
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
                detect: default_detect.clone(),
                timeout: default_timeout,
                prebuild: None,
                tags: Vec::new(),
                working_dir: None,
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
    pub status: Status,
    pub elapsed: Duration,
    pub stdout: String,
    pub stderr: String,
    /// Exit code, if the process completed (vs. was killed for timeout).
    pub exit_code: Option<i32>,
}

impl TestOutcome {
    pub fn passed(&self) -> bool {
        self.status.is_pass()
    }
}

/// Run-time options applied uniformly across `run_one` / `run_many`.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Trace mode: build with FST and run from `target/kiln/waves/`.
    pub trace: bool,
    /// Stream the simulator's stdout/stderr to the terminal in real
    /// time. Captured fields on the returned outcome are empty.
    /// Must only be used with `jobs == 1` for parallel runs.
    pub nocapture: bool,
    /// Optional channel that receives outcomes as they complete (for
    /// streaming display). When set, parallel workers send results here
    /// rather than the caller waiting on `run_many*` to return.
    pub progress_tx: Option<mpsc::Sender<ProgressEvent>>,
}

/// Events emitted by the runner for streaming display.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// Build completed; entering the run phase.
    BuildsDone,
    /// A test started.
    Started { name: String },
    /// A test finished. Index is the position in the input slice.
    Finished {
        index: usize,
        outcome: Result<TestOutcome, String>,
    },
}

/// Build and run one test with default options. Convenience wrapper.
pub fn run_one(
    project_root: &Path,
    manifest: &Manifest,
    base_source_set: &SourceSet,
    test: &DiscoveredTest,
) -> Result<TestOutcome, TestError> {
    run_one_with_options(project_root, manifest, base_source_set, test, &RunOptions::default())
}

/// Build (or reuse cached binary) and run one test.
pub fn run_one_with_options(
    project_root: &Path,
    manifest: &Manifest,
    base_source_set: &SourceSet,
    test: &DiscoveredTest,
    opts: &RunOptions,
) -> Result<TestOutcome, TestError> {
    let start = Instant::now();

    if let Some(prebuild) = &test.prebuild {
        run_prebuild(project_root, prebuild)?;
    }

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

    let plan =
        BuildPlan::new(&manifest_for_test, &source_set, Profile::Debug).with_trace(opts.trace);
    let outcome = verilator::compile(&plan)?;
    let binary = match outcome.binary {
        Some(b) => b,
        None => {
            return Ok(TestOutcome {
                name: test.name.clone(),
                status: Status::Fail,
                elapsed: start.elapsed(),
                stdout: String::new(),
                stderr: format_diagnostics(&outcome.diagnostics),
                exit_code: outcome.exit_code,
            });
        }
    };

    // When tracing, run the binary in `<project>/target/kiln/waves/`
    // so its `$dumpfile("<top>.fst")` lands in the right place.
    let wave_dir = if opts.trace {
        let dir = project_root.join("target").join("kiln").join("waves");
        std::fs::create_dir_all(&dir).map_err(|source| TestError::Io {
            path: dir.clone(),
            source,
        })?;
        Some(dir)
    } else {
        None
    };

    // Resolve sim cwd. Priority: trace dir > per-case working_dir >
    // [test] working_dir > project_root.
    let resolved_cwd = if opts.trace {
        wave_dir.clone()
    } else if let Some(d) = &test.working_dir {
        Some(project_root.join(d))
    } else {
        manifest
            .test
            .working_dir
            .as_ref()
            .map(|d| project_root.join(d))
    };

    if opts.nocapture {
        run_streaming(
            &binary,
            resolved_cwd.as_deref(),
            &test.args,
            &test.name,
            &test.detect,
            test.timeout,
            start,
        )
    } else {
        run_buffered(
            &binary,
            resolved_cwd.as_deref(),
            &test.args,
            &test.name,
            &test.detect,
            test.timeout,
            start,
        )
    }
}

fn run_prebuild(project_root: &Path, cmd: &str) -> Result<(), TestError> {
    tracing::debug!(target: "kiln-test", cmd, "running prebuild");
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| TestError::Io {
            path: PathBuf::from("sh"),
            source,
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stderr = if stderr.is_empty() {
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else {
            stderr
        };
        return Err(TestError::Prebuild {
            cmd: cmd.to_string(),
            code: output.status.code().unwrap_or(-1),
            stderr,
        });
    }
    Ok(())
}

fn make_cmd(binary: &Path, cwd: Option<&Path>, args: &[String]) -> Command {
    let mut cmd = Command::new(binary);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args);
    cmd
}

fn classify(detect: &Detect, exit_code: Option<i32>, stdout: &str) -> Status {
    match detect {
        Detect::ExitCode => match exit_code {
            Some(0) => Status::Pass,
            _ => Status::Fail,
        },
        Detect::Patterns {
            stdout_contains,
            stdout_must_not_contain,
        } => {
            for needle in stdout_must_not_contain {
                if stdout.contains(needle) {
                    return Status::Fail;
                }
            }
            for needle in stdout_contains {
                if !stdout.contains(needle) {
                    return Status::Fail;
                }
            }
            Status::Pass
        }
    }
}

/// Capture all output, then return it. Honors timeout by killing on
/// expiry and returning [`Status::Timeout`]. Drains stdout/stderr on
/// background threads so a chatty test cannot fill the pipe and
/// deadlock the parent.
fn run_buffered(
    binary: &Path,
    cwd: Option<&Path>,
    args: &[String],
    name: &str,
    detect: &Detect,
    timeout: Option<Duration>,
    start: Instant,
) -> Result<TestOutcome, TestError> {
    let mut child = make_cmd(binary, cwd, args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| TestError::Io {
            path: binary.to_path_buf(),
            source,
        })?;

    // Drain pipes on threads so a process producing >64 KB doesn't block.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread = stdout_pipe.map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = p.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_thread = stderr_pipe.map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = p.read_to_end(&mut buf);
            buf
        })
    });

    let mut timed_out = false;
    let exit_code = if let Some(t) = timeout {
        wait_with_timeout(&mut child, t, &mut timed_out)?
    } else {
        let status = child.wait().map_err(|source| TestError::Io {
            path: binary.to_path_buf(),
            source,
        })?;
        status.code()
    };

    let stdout_bytes = stdout_thread
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr_bytes = stderr_thread
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();

    let status = if timed_out {
        Status::Timeout
    } else {
        classify(detect, exit_code, &stdout)
    };

    Ok(TestOutcome {
        name: name.to_string(),
        status,
        elapsed: start.elapsed(),
        stdout,
        stderr,
        exit_code,
    })
}

/// Stream stdout/stderr to the terminal as the simulation runs. Empty
/// captured fields. Honors timeout: on expiry, kill and return
/// [`Status::Timeout`]. With `--nocapture` enabled, detection by
/// stdout patterns is best-effort: we tee the bytes into a buffer for
/// classification *and* to stdout for the user.
fn run_streaming(
    binary: &Path,
    cwd: Option<&Path>,
    args: &[String],
    name: &str,
    detect: &Detect,
    timeout: Option<Duration>,
    start: Instant,
) -> Result<TestOutcome, TestError> {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    let mut child = make_cmd(binary, cwd, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| TestError::Io {
            path: binary.to_path_buf(),
            source,
        })?;

    // Tee both streams: print + accumulate. Detection patterns can then
    // see the full stdout even in nocapture mode.
    let stdout_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_buf_w = Arc::clone(&stdout_buf);
    let stdout_thread = stdout_pipe.map(|p| {
        std::thread::spawn(move || {
            let reader = BufReader::new(p);
            for line in reader.lines().map_while(Result::ok) {
                println!("{line}");
                let mut g = stdout_buf_w.lock().unwrap();
                g.extend_from_slice(line.as_bytes());
                g.push(b'\n');
            }
        })
    });
    let stderr_buf_w = Arc::clone(&stderr_buf);
    let stderr_thread = stderr_pipe.map(|p| {
        std::thread::spawn(move || {
            let reader = BufReader::new(p);
            for line in reader.lines().map_while(Result::ok) {
                let _ = writeln!(std::io::stderr(), "{line}");
                let mut g = stderr_buf_w.lock().unwrap();
                g.extend_from_slice(line.as_bytes());
                g.push(b'\n');
            }
        })
    });

    let mut timed_out = false;
    let exit_code = if let Some(t) = timeout {
        wait_with_timeout(&mut child, t, &mut timed_out)?
    } else {
        let status = child.wait().map_err(|source| TestError::Io {
            path: binary.to_path_buf(),
            source,
        })?;
        status.code()
    };

    if let Some(h) = stdout_thread {
        let _ = h.join();
    }
    if let Some(h) = stderr_thread {
        let _ = h.join();
    }

    let stdout = {
        let g = stdout_buf.lock().unwrap();
        String::from_utf8_lossy(&g).into_owned()
    };

    let status = if timed_out {
        Status::Timeout
    } else {
        classify(detect, exit_code, &stdout)
    };

    Ok(TestOutcome {
        name: name.to_string(),
        status,
        elapsed: start.elapsed(),
        // Already streamed — clear captures to avoid double-printing
        // at the summary stage. Detection consumed `stdout` above.
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
    })
}

/// Poll a child until it exits or `timeout` elapses; on timeout, kill
/// the process and (briefly) wait for it to die.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    timed_out: &mut bool,
) -> Result<Option<i32>, TestError> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code()),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    // Best-effort reap so we don't leave zombies.
                    let _ = child.wait();
                    *timed_out = true;
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => {
                return Err(TestError::Io {
                    path: PathBuf::new(),
                    source: e,
                });
            }
        }
    }
}

/// Run a slice of tests in parallel, up to `jobs` concurrent workers.
/// Convenience wrapper preserving older signature.
pub fn run_many(
    project_root: &Path,
    manifest: &Manifest,
    source_set: &SourceSet,
    tests: &[DiscoveredTest],
    jobs: usize,
) -> Vec<Result<TestOutcome, TestError>> {
    run_many_with_options(project_root, manifest, source_set, tests, jobs, &RunOptions::default())
}

/// Run a slice of tests in parallel, with `RunOptions` propagated.
/// Order of returned outcomes matches `tests`. `nocapture` must only be
/// used with `jobs == 1`. If `opts.progress_tx` is set, [`ProgressEvent`]s
/// are emitted as work happens; otherwise the caller just gets the final
/// vector at the end.
pub fn run_many_with_options(
    project_root: &Path,
    manifest: &Manifest,
    source_set: &SourceSet,
    tests: &[DiscoveredTest],
    jobs: usize,
    opts: &RunOptions,
) -> Vec<Result<TestOutcome, TestError>> {
    use std::sync::{Arc, Mutex};

    // Pre-compile each unique testbench (by top name) sequentially to warm
    // the cache before parallel workers start. Without this, N workers that
    // share a testbench all try to compile it simultaneously and stomp.
    {
        let mut seen: HashSet<String> = HashSet::new();
        for test in tests {
            if seen.insert(test.top.clone()) {
                let mut files = source_set.files.clone();
                let canon = test.source.canonicalize().unwrap_or(test.source.clone());
                if !files.contains(&canon) {
                    files.push(canon);
                }
                let ss = SourceSet {
                    project_root: source_set.project_root.clone(),
                    files,
                };
                let mut mft = manifest.clone();
                mft.design.top = test.top.clone();
                let plan = BuildPlan::new(&mft, &ss, Profile::Debug).with_trace(opts.trace);
                let _ = verilator::compile(&plan);
            }
        }
    }
    if let Some(tx) = &opts.progress_tx {
        let _ = tx.send(ProgressEvent::BuildsDone);
    }

    // Run prebuilds upfront, deduped, sequentially. We do this before the
    // parallel run so a `make -C software/c_tests/<x>` doesn't get re-run
    // 6 times for 6 sibling tests, and so prebuild failures fail fast.
    let mut seen_prebuilds: HashSet<String> = HashSet::new();
    let mut prebuild_errors: BTreeMap<String, String> = BTreeMap::new();
    for test in tests {
        if let Some(p) = &test.prebuild {
            if !seen_prebuilds.insert(p.clone()) {
                continue;
            }
            if let Err(e) = run_prebuild(project_root, p) {
                prebuild_errors.insert(p.clone(), e.to_string());
            }
        }
    }

    let next = Arc::new(Mutex::new(0usize));
    let results: Arc<Mutex<Vec<Option<Result<TestOutcome, TestError>>>>> =
        Arc::new(Mutex::new((0..tests.len()).map(|_| None).collect()));
    let workers = jobs.max(1).min(tests.len().max(1));

    // Build per-worker options: the worker should NOT re-run prebuilds
    // (we already did them upfront), so we strip them.
    let prebuild_errors = Arc::new(prebuild_errors);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = Arc::clone(&next);
            let results = Arc::clone(&results);
            let prebuild_errors = Arc::clone(&prebuild_errors);
            let progress = opts.progress_tx.clone();
            let opts_inner = RunOptions {
                trace: opts.trace,
                nocapture: opts.nocapture,
                progress_tx: None,
            };
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
                let test = &tests[idx];
                if let Some(tx) = &progress {
                    let _ = tx.send(ProgressEvent::Started {
                        name: test.name.clone(),
                    });
                }
                // Strip prebuild here; we've already run it.
                let test_no_prebuild = DiscoveredTest {
                    prebuild: None,
                    ..test.clone()
                };
                let r = if let Some(p) = &test.prebuild {
                    if let Some(err) = prebuild_errors.get(p) {
                        Ok(TestOutcome {
                            name: test.name.clone(),
                            status: Status::Fail,
                            elapsed: Duration::from_secs(0),
                            stdout: String::new(),
                            stderr: format!("prebuild failed: {err}"),
                            exit_code: None,
                        })
                    } else {
                        run_one_with_options(
                            project_root,
                            manifest,
                            source_set,
                            &test_no_prebuild,
                            &opts_inner,
                        )
                    }
                } else {
                    run_one_with_options(
                        project_root,
                        manifest,
                        source_set,
                        &test_no_prebuild,
                        &opts_inner,
                    )
                };
                if let Some(tx) = &progress {
                    let outcome_for_event = match &r {
                        Ok(o) => Ok(o.clone()),
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx.send(ProgressEvent::Finished {
                        index: idx,
                        outcome: outcome_for_event,
                    });
                }
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

// ============================================================================
// last-run.json persistence (for --rerun and --skip-passed).
// ============================================================================

/// Persisted record of the previous `kiln test` invocation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastRun {
    pub outcomes: BTreeMap<String, LastRunEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastRunEntry {
    pub status: Status,
    /// Elapsed in milliseconds; cheap to display, easy to round-trip.
    pub elapsed_ms: u128,
}

/// Path to the persisted last-run file under the build cache root.
pub fn last_run_path(project_root: &Path) -> PathBuf {
    project_root
        .join("target")
        .join("kiln")
        .join("last-run.json")
}

/// Load the previous run, returning an empty record if missing or malformed.
pub fn load_last_run(project_root: &Path) -> LastRun {
    let p = last_run_path(project_root);
    let Ok(text) = std::fs::read_to_string(&p) else {
        return LastRun::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Persist the latest outcomes, merging into any existing record.
pub fn save_last_run(project_root: &Path, run: &LastRun) -> std::io::Result<()> {
    let p = last_run_path(project_root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(run).expect("LastRun serialises");
    std::fs::write(&p, json)
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

    #[test]
    fn classify_exit_code_passes_on_zero() {
        assert_eq!(classify(&Detect::ExitCode, Some(0), ""), Status::Pass);
        assert_eq!(classify(&Detect::ExitCode, Some(1), ""), Status::Fail);
        assert_eq!(classify(&Detect::ExitCode, None, ""), Status::Fail);
    }

    #[test]
    fn classify_patterns_requires_substring() {
        let d = Detect::Patterns {
            stdout_contains: vec!["[PASS]".to_string()],
            stdout_must_not_contain: vec!["[failed]".to_string(), "Timeout!".to_string()],
        };
        assert_eq!(
            classify(&d, Some(0), "blah\n[PASS] - foo\nbar"),
            Status::Pass
        );
        assert_eq!(
            classify(&d, Some(0), "blah\n[PASS]\n[failed]"),
            Status::Fail
        );
        assert_eq!(
            classify(&d, Some(0), "no marker here"),
            Status::Fail
        );
        assert_eq!(
            classify(&d, Some(0), "[PASS]\nTimeout!"),
            Status::Fail
        );
    }

    #[test]
    fn substitute_handles_known_and_unknown_keys() {
        let mut subs = BTreeMap::new();
        subs.insert("stem", "fib".to_string());
        subs.insert("path", "software/c_tests/fib/fib.hex".to_string());
        assert_eq!(substitute("name={stem}", &subs), "name=fib");
        assert_eq!(
            substitute("+hex_file={path}+x={stem}", &subs),
            "+hex_file=software/c_tests/fib/fib.hex+x=fib"
        );
        // Unknown keys pass through verbatim so user can spot typos.
        assert_eq!(substitute("ke{ynope}y", &subs), "ke{ynope}y");
        // Bare braces don't crash.
        assert_eq!(substitute("a{b", &subs), "a{b");
    }

    #[test]
    fn matrix_expands_glob() {
        let tmp = tempfile::tempdir().unwrap();
        // Set up testbench
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join("tests/foo_tb.sv"), "module foo_tb; endmodule")
            .unwrap();
        // Fixtures
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::fs::write(tmp.path().join("data/a.hex"), "").unwrap();
        std::fs::write(tmp.path().join("data/b.hex"), "").unwrap();
        let toml = format!(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [[test.matrix]]
            testbench = "foo_tb"
            inputs = "data/*.hex"
            name_prefix = "case_"
            args = ["+hex_file={{stem}}"]
            "#,
        );
        let _ = toml; // silence if unused
        let m: Manifest = r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [[test.matrix]]
            testbench = "foo_tb"
            inputs = "data/*.hex"
            name_prefix = "case_"
            args = ["+hex_file={stem}"]
            "#
        .parse()
        .unwrap();
        let found = discover(tmp.path(), &m).unwrap();
        let names: Vec<_> = found.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["case_a", "case_b"]);
        assert_eq!(found[0].args, vec!["+hex_file=a"]);
        assert_eq!(found[1].args, vec!["+hex_file=b"]);
    }

    #[test]
    fn matrix_inherits_default_detect_and_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join("tests/foo_tb.sv"), "").unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::fs::write(tmp.path().join("data/x.hex"), "").unwrap();
        let m: Manifest = r#"
            [package]
            name = "demo"
            version = "0.1.0"
            [design]
            top = "t"
            [test]
            timeout = "30s"
            detect = { patterns = { stdout_contains = ["[PASS]"], stdout_must_not_contain = ["[failed]"] } }
            [[test.matrix]]
            testbench = "foo_tb"
            inputs = "data/*.hex"
            "#
        .parse()
        .unwrap();
        let found = discover(tmp.path(), &m).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].timeout, Some(Duration::from_secs(30)));
        match &found[0].detect {
            Detect::Patterns {
                stdout_contains, ..
            } => {
                assert_eq!(stdout_contains, &vec!["[PASS]".to_string()]);
            }
            _ => panic!("expected patterns"),
        }
    }
}
