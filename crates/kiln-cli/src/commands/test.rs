//! `kiln test`.
//!
//! Cargo-style: per-test outcome printed as it completes, live progress
//! ticker beneath the finished output, summary at the end with
//! `pass/fail/timeout/filtered`. Persistence in
//! `target/kiln/last-run.json` powers `--rerun` and `--skip-passed`.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, HookPhase, Manifest};
use kiln_test::{
    discover, load_last_run, run_many_with_options, save_last_run, DiscoveredTest, LastRun,
    LastRunEntry, ProgressEvent, RunOptions, Status,
};

use crate::commands::{apply_feature_flags, FeatureFlags};
use crate::hooks;
use crate::reporter;

/// Flat parameter object so the dispatch arm in `commands/mod.rs` stays tidy.
pub struct Args {
    pub filters: Vec<String>,
    pub exact: bool,
    pub skip: Vec<String>,
    pub tag: Vec<String>,
    pub jobs: Option<usize>,
    pub no_fail_fast: bool,
    pub list: bool,
    pub nocapture: bool,
    pub show_output: bool,
    pub rerun: bool,
    pub skip_passed: bool,
    pub trace: bool,
    #[allow(dead_code)]
    pub profile: String,
    pub features: FeatureFlags,
}

pub fn run(args: Args) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let mut manifest = Manifest::load(&manifest_path)?;
    apply_feature_flags(&mut manifest, &args.features)?;

    hooks::run_pre_hook(&project_root, &manifest.hooks, HookPhase::PreBuild)?;
    hooks::run_pre_hook(&project_root, &manifest.hooks, HookPhase::PreTest)?;
    let outcome = run_inner(args, &manifest, &project_root);
    hooks::run_post_hook(&project_root, &manifest.hooks, HookPhase::PostTest);
    outcome
}

fn run_inner(args: Args, manifest: &Manifest, project_root: &Path) -> Result<()> {
    // Re-bind to keep the rest of the body unchanged. The original
    // function did this as locals.
    let manifest = manifest.clone();
    let project_root = project_root.to_path_buf();

    let all_tests = discover(&project_root, &manifest)?;
    let total_discovered = all_tests.len();

    let last_run = if args.rerun || args.skip_passed {
        Some(load_last_run(&project_root))
    } else {
        None
    };

    let tests = filter_tests(all_tests, &args, last_run.as_ref());

    if args.list {
        for t in &tests {
            println!("{}", t.name);
        }
        return Ok(());
    }

    if tests.is_empty() {
        let filtered_out = total_discovered;
        if filtered_out == 0 {
            reporter::info(
                "Skipping",
                "no tests matched (add testbenches under tests/<name>.sv)",
            );
        } else {
            reporter::info(
                "Skipping",
                format!("no tests matched filters ({filtered_out} discovered)"),
            );
        }
        return Ok(());
    }

    let mut source_set =
        SourceSet::resolve(&project_root, &manifest).context("resolving project source set")?;
    if !manifest.dependencies.is_empty() {
        reporter::status("Resolving", "dependencies via bender");
        let resolved = kiln_deps::resolve(&project_root, &manifest)?;
        for f in resolved.all_files() {
            if !source_set.files.contains(&f) {
                source_set.files.push(f);
            }
        }
    }

    let jobs = args.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });
    if args.nocapture && jobs > 1 {
        anyhow::bail!(
            "--nocapture (--verbose) requires --jobs 1 (streaming output from parallel tests would interleave)"
        );
    }

    let trace_effective = args.trace || manifest.wave.enabled_by_default;

    let started = Instant::now();
    let filtered_out = total_discovered.saturating_sub(tests.len());
    reporter::status(
        "Running",
        format!(
            "{} test{} ({} parallel{}{})",
            tests.len(),
            if tests.len() == 1 { "" } else { "s" },
            jobs,
            if trace_effective {
                ", with --trace"
            } else {
                ""
            },
            if filtered_out > 0 {
                format!(", {filtered_out} filtered")
            } else {
                String::new()
            },
        ),
    );

    // Channel: workers send ProgressEvent; this thread renders.
    let (tx, rx) = mpsc::channel::<ProgressEvent>();
    let opts = RunOptions {
        trace: trace_effective,
        nocapture: args.nocapture,
        progress_tx: Some(tx),
    };

    // Run in a scoped thread so we can render from the main thread.
    let outcomes_thread = std::thread::scope(|scope| -> Vec<_> {
        let pr = &project_root;
        let mf = &manifest;
        let ss = &source_set;
        let ts = &tests;
        let handle = scope.spawn(move || run_many_with_options(pr, mf, ss, ts, jobs, &opts));
        render_progress(rx, &tests, args.no_fail_fast, args.nocapture);
        handle.join().expect("test runner thread")
    });

    let mut last_run_record = LastRun::default();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    let mut errored = 0usize;
    let mut failure_blocks: Vec<(String, String, String)> = Vec::new();
    let mut show_output_blocks: Vec<(String, String)> = Vec::new();
    let mut stopped_early = false;
    for (i, o) in outcomes_thread.iter().enumerate() {
        let name = tests[i].name.clone();
        match o {
            Ok(t) => {
                last_run_record.outcomes.insert(
                    t.name.clone(),
                    LastRunEntry {
                        status: t.status,
                        elapsed_ms: t.elapsed.as_millis(),
                    },
                );
                match t.status {
                    Status::Pass => {
                        passed += 1;
                        if args.show_output && !t.stdout.is_empty() {
                            show_output_blocks.push((t.name.clone(), t.stdout.clone()));
                        }
                    }
                    Status::Fail => {
                        failed += 1;
                        if !args.nocapture {
                            failure_blocks.push((
                                t.name.clone(),
                                t.stdout.clone(),
                                t.stderr.clone(),
                            ));
                        }
                    }
                    Status::Timeout => {
                        timed_out += 1;
                        if !args.nocapture {
                            failure_blocks.push((
                                t.name.clone(),
                                t.stdout.clone(),
                                t.stderr.clone(),
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                errored += 1;
                last_run_record.outcomes.insert(
                    name.clone(),
                    LastRunEntry {
                        status: Status::Fail,
                        elapsed_ms: 0,
                    },
                );
                failure_blocks.push((name, String::new(), e.to_string()));
            }
        }
    }
    if !args.no_fail_fast && (failed + timed_out + errored) > 0 {
        // Detect whether any test was skipped due to a stop. We can't
        // tell directly, but the runner ran them all in parallel; the
        // semantic is: with default fail-fast, we don't stop the in-flight
        // batch but we report it. Match cargo's "stopped on first failure"
        // wording when the count of rendered outcomes < tests.len().
        if outcomes_thread.iter().any(|r| {
            matches!(
                r.as_ref().map(|t| t.status),
                Ok(Status::Fail) | Ok(Status::Timeout)
            ) || r.is_err()
        }) {
            stopped_early = false; // We don't actually stop early today.
        }
    }

    // Failure blocks first (mirrors cargo).
    if !failure_blocks.is_empty() {
        eprintln!();
        eprintln!("failures:");
        for (name, stdout, stderr) in &failure_blocks {
            eprintln!();
            eprintln!("---- {name} stdout ----");
            for line in stdout.lines() {
                eprintln!("{line}");
            }
            if !stderr.is_empty() {
                eprintln!("---- {name} stderr ----");
                for line in stderr.lines() {
                    eprintln!("{line}");
                }
            }
        }
        eprintln!();
        eprintln!("failures:");
        for (name, _, _) in &failure_blocks {
            eprintln!("    {name}");
        }
    }

    // Optional: passing-test stdout dump.
    if args.show_output && !show_output_blocks.is_empty() {
        eprintln!();
        eprintln!("successes (--show-output):");
        for (name, stdout) in &show_output_blocks {
            eprintln!();
            eprintln!("---- {name} stdout ----");
            for line in stdout.lines() {
                eprintln!("{line}");
            }
        }
    }

    let elapsed = started.elapsed();
    let total = outcomes_thread.len();
    let summary = format!(
        "{} passed, {} failed, {} timeout{}, {} filtered, {} total in {}{}",
        passed,
        failed,
        timed_out,
        if errored > 0 {
            format!(", {errored} error")
        } else {
            String::new()
        },
        filtered_out,
        total,
        format_duration(elapsed),
        if stopped_early {
            " (stopped on first failure)"
        } else {
            ""
        }
    );
    let any_fail = failed + timed_out + errored > 0;

    // Persist last-run before the bail!.
    let _ = save_last_run(&project_root, &last_run_record);

    if any_fail {
        reporter::status("Result", reporter::red(&summary));
        anyhow::bail!("{} test(s) failed", failed + timed_out + errored);
    }
    reporter::status("Result", reporter::green(&summary));
    Ok(())
}

/// Apply --filters / --skip / --tag / --rerun / --skip-passed.
fn filter_tests(
    all: Vec<DiscoveredTest>,
    args: &Args,
    last_run: Option<&LastRun>,
) -> Vec<DiscoveredTest> {
    let filters = &args.filters;
    let skip = &args.skip;
    let tag = &args.tag;
    all.into_iter()
        .filter(|t| {
            // Positional filters: OR semantics. Empty list = everything.
            if !filters.is_empty() {
                let any = filters.iter().any(|f| {
                    if args.exact {
                        t.name == *f
                    } else {
                        t.name.contains(f)
                    }
                });
                if !any {
                    return false;
                }
            }
            // Skip filters: exclude if any matches.
            if !skip.is_empty() && skip.iter().any(|s| t.name.contains(s)) {
                return false;
            }
            // Tag filters: keep if any tag matches.
            if !tag.is_empty() && !tag.iter().any(|tg| t.tags.iter().any(|tt| tt == tg)) {
                return false;
            }
            // last-run filters.
            if let Some(lr) = last_run {
                if args.rerun {
                    match lr.outcomes.get(&t.name) {
                        Some(e) if e.status.is_pass() => return false,
                        _ => {}
                    }
                } else if args.skip_passed {
                    if let Some(e) = lr.outcomes.get(&t.name) {
                        if e.status.is_pass() {
                            return false;
                        }
                    }
                }
            }
            true
        })
        .collect()
}

/// Drain the progress channel: print per-test outcomes as they arrive
/// and (on a TTY) display a one-line in-flight ticker beneath them.
fn render_progress(
    rx: mpsc::Receiver<ProgressEvent>,
    tests: &[DiscoveredTest],
    _no_fail_fast: bool,
    nocapture: bool,
) {
    let total = tests.len();
    // Track in-flight test names ordered by start.
    let mut in_flight: Vec<String> = Vec::new();
    let mut finished: usize = 0;
    let mut builds_done = false;
    let use_ticker = !nocapture && std::io::stderr().is_terminal();

    if use_ticker {
        write_ticker(&in_flight, finished, total, builds_done);
    }

    while let Ok(ev) = rx.recv() {
        match ev {
            ProgressEvent::BuildsDone => {
                builds_done = true;
                if use_ticker {
                    clear_ticker();
                    reporter::info("Compiled", "all testbenches; running…");
                    write_ticker(&in_flight, finished, total, builds_done);
                }
            }
            ProgressEvent::Started { name } => {
                in_flight.push(name);
                if use_ticker {
                    write_ticker(&in_flight, finished, total, builds_done);
                }
            }
            ProgressEvent::Finished { outcome, .. } => {
                if use_ticker {
                    clear_ticker();
                }
                match &outcome {
                    Ok(t) => {
                        if let Some(pos) = in_flight.iter().position(|n| n == &t.name) {
                            in_flight.remove(pos);
                        }
                        let label = match t.status {
                            Status::Pass => reporter::green("PASS"),
                            Status::Fail => reporter::red("FAIL"),
                            Status::Timeout => reporter::yellow("TIMEOUT"),
                        };
                        let elapsed = reporter::dim(&format_duration(t.elapsed));
                        println!("test {} ... {label} ({elapsed})", t.name);
                    }
                    Err(e) => {
                        let label = reporter::red("ERROR");
                        println!("test ? ... {label}: {e}");
                    }
                }
                finished += 1;
                if use_ticker {
                    write_ticker(&in_flight, finished, total, builds_done);
                }
            }
        }
    }
    if use_ticker {
        clear_ticker();
    }
}

fn write_ticker(in_flight: &[String], finished: usize, total: usize, builds_done: bool) {
    let phase = if builds_done { "Running" } else { "Building" };
    let snapshot: Vec<&str> = in_flight.iter().take(4).map(|s| s.as_str()).collect();
    let extra = in_flight.len().saturating_sub(snapshot.len());
    let mut s = format!("    {} {}/{}", reporter::dim(phase), finished, total,);
    if !snapshot.is_empty() {
        s.push_str(" — ");
        s.push_str(&snapshot.join(", "));
        if extra > 0 {
            s.push_str(&format!(" +{extra}"));
        }
    }
    let mut e = std::io::stderr();
    let _ = write!(e, "\r\x1b[2K{s}");
    let _ = e.flush();
}

fn clear_ticker() {
    let mut e = std::io::stderr();
    let _ = write!(e, "\r\x1b[2K");
    let _ = e.flush();
}

fn format_duration(d: Duration) -> String {
    if d.as_secs() == 0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::Detect;

    fn dt(name: &str, tags: &[&str]) -> DiscoveredTest {
        DiscoveredTest {
            name: name.to_string(),
            source: std::path::PathBuf::from("/dev/null"),
            top: name.to_string(),
            args: Vec::new(),
            detect: Detect::ExitCode,
            timeout: None,
            prebuild: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            working_dir: None,
        }
    }

    fn args(filters: &[&str], skip: &[&str], tag: &[&str], exact: bool) -> Args {
        Args {
            filters: filters.iter().map(|s| s.to_string()).collect(),
            exact,
            skip: skip.iter().map(|s| s.to_string()).collect(),
            tag: tag.iter().map(|s| s.to_string()).collect(),
            jobs: None,
            no_fail_fast: false,
            list: false,
            nocapture: false,
            show_output: false,
            rerun: false,
            skip_passed: false,
            trace: false,
            profile: "test".into(),
            features: FeatureFlags::default(),
        }
    }

    #[test]
    fn filter_or_semantics() {
        let all = vec![
            dt("isa_add", &["isa"]),
            dt("isa_sub", &["isa"]),
            dt("c_tests_fib", &["c"]),
            dt("smoke", &[]),
        ];
        let kept = filter_tests(all, &args(&["isa_", "smoke"], &[], &[], false), None);
        let names: Vec<_> = kept.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["isa_add", "isa_sub", "smoke"]);
    }

    #[test]
    fn filter_skip() {
        let all = vec![dt("isa_add", &[]), dt("isa_addi", &[]), dt("isa_sub", &[])];
        let kept = filter_tests(all, &args(&["isa_"], &["addi"], &[], false), None);
        let names: Vec<_> = kept.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["isa_add", "isa_sub"]);
    }

    #[test]
    fn filter_tag() {
        let all = vec![
            dt("a", &["fast"]),
            dt("b", &["slow"]),
            dt("c", &["fast", "isa"]),
        ];
        let kept = filter_tests(all, &args(&[], &[], &["isa"], false), None);
        let names: Vec<_> = kept.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["c"]);
    }

    #[test]
    fn filter_exact() {
        let all = vec![dt("isa_add", &[]), dt("isa_addi", &[])];
        let kept = filter_tests(all, &args(&["isa_add"], &[], &[], true), None);
        let names: Vec<_> = kept.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["isa_add"]);
    }

    #[test]
    fn filter_rerun_skips_passed() {
        let all = vec![dt("a", &[]), dt("b", &[]), dt("c", &[])];
        let mut lr = LastRun::default();
        lr.outcomes.insert(
            "a".into(),
            LastRunEntry {
                status: Status::Pass,
                elapsed_ms: 10,
            },
        );
        lr.outcomes.insert(
            "b".into(),
            LastRunEntry {
                status: Status::Fail,
                elapsed_ms: 10,
            },
        );
        let mut a = args(&[], &[], &[], false);
        a.rerun = true;
        let kept = filter_tests(all, &a, Some(&lr));
        let names: Vec<_> = kept.iter().map(|t| t.name.as_str()).collect();
        // a passed -> dropped. b failed -> kept. c never ran -> kept.
        assert_eq!(names, vec!["b", "c"]);
    }

    #[test]
    fn filter_skip_passed_keeps_unknown() {
        let all = vec![dt("a", &[]), dt("b", &[]), dt("c", &[])];
        let mut lr = LastRun::default();
        lr.outcomes.insert(
            "a".into(),
            LastRunEntry {
                status: Status::Pass,
                elapsed_ms: 10,
            },
        );
        let mut a = args(&[], &[], &[], false);
        a.skip_passed = true;
        let kept = filter_tests(all, &a, Some(&lr));
        let names: Vec<_> = kept.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["b", "c"]);
    }
}
