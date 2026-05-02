//! `kiln test`.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, Manifest};
use kiln_test::{discover, run_many_with_options};

use crate::reporter;

pub fn run(
    filter: Option<String>,
    jobs: Option<usize>,
    no_fail_fast: bool,
    list: bool,
    trace: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let manifest = Manifest::load(&manifest_path)?;

    let mut tests = discover(&project_root)?;
    if let Some(f) = &filter {
        tests.retain(|t| t.name.contains(f));
    }

    if list {
        // `--list` is data-mode: stdout, no decoration, so callers can
        // pipe into xargs / jq / shell scripts.
        for t in &tests {
            println!("{}", t.name);
        }
        return Ok(());
    }

    if tests.is_empty() {
        reporter::info(
            "Skipping",
            "no tests matched (add testbenches under tests/<name>.sv)",
        );
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

    let jobs = jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });
    let trace_effective = trace || manifest.wave.enabled_by_default;

    let started = Instant::now();
    reporter::status(
        "Running",
        format!(
            "{} test{} ({} parallel{})",
            tests.len(),
            if tests.len() == 1 { "" } else { "s" },
            jobs,
            if trace_effective {
                ", with --trace"
            } else {
                ""
            }
        ),
    );
    let outcomes = run_many_with_options(
        &project_root,
        &manifest,
        &source_set,
        &tests,
        jobs,
        trace_effective,
    );

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut stopped_early = false;
    for o in &outcomes {
        match o {
            Ok(t) => {
                let label = if t.passed {
                    reporter::green("PASS")
                } else {
                    reporter::red("FAIL")
                };
                let elapsed = reporter::dim(&format_duration(t.elapsed));
                println!("test {} ... {label} ({elapsed})", t.name);
                if !t.passed {
                    failed += 1;
                    if !t.stdout.is_empty() {
                        println!("  stdout: {}", t.stdout.lines().last().unwrap_or(""));
                    }
                    if !t.stderr.is_empty() {
                        println!("  stderr: {}", t.stderr.lines().last().unwrap_or(""));
                    }
                    if !no_fail_fast {
                        stopped_early = true;
                        break;
                    }
                } else {
                    passed += 1;
                }
            }
            Err(e) => {
                println!("test ?: {} ({e})", reporter::red("ERROR"));
                failed += 1;
                if !no_fail_fast {
                    stopped_early = true;
                    break;
                }
            }
        }
    }

    let elapsed = started.elapsed();
    let total = outcomes.len();
    let summary = format!(
        "{} passed, {} failed, {} total in {}{}",
        passed,
        failed,
        total,
        format_duration(elapsed),
        if stopped_early {
            " (stopped on first failure)"
        } else {
            ""
        }
    );
    if failed > 0 {
        reporter::status("Result", reporter::red(&summary));
        anyhow::bail!("{failed} test(s) failed");
    }
    reporter::status("Result", reporter::green(&summary));
    Ok(())
}

fn format_duration(d: Duration) -> String {
    if d.as_secs() == 0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f32())
    }
}
