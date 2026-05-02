//! `kiln test`.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, Manifest};
use kiln_test::{discover, run_many};

pub fn run(
    filter: Option<String>,
    jobs: Option<usize>,
    no_fail_fast: bool,
    list: bool,
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
        for t in &tests {
            println!("{}", t.name);
        }
        return Ok(());
    }

    if tests.is_empty() {
        println!("No tests matched. Add testbenches under tests/<name>.sv.");
        return Ok(());
    }

    let mut source_set =
        SourceSet::resolve(&project_root, &manifest).context("resolving project source set")?;
    if !manifest.dependencies.is_empty() {
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
    let outcomes = run_many(&project_root, &manifest, &source_set, &tests, jobs);

    let mut passed = 0usize;
    let mut failed = 0usize;
    for o in &outcomes {
        match o {
            Ok(t) => {
                let label = if t.passed { "PASS" } else { "FAIL" };
                let elapsed = format_duration(t.elapsed);
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
                        break;
                    }
                } else {
                    passed += 1;
                }
            }
            Err(e) => {
                println!("test ?: ERROR ({e})");
                failed += 1;
                if !no_fail_fast {
                    break;
                }
            }
        }
    }

    println!(
        "\ntest result: {} passed, {} failed, {} total",
        passed,
        failed,
        outcomes.len()
    );
    if failed > 0 {
        anyhow::bail!("{failed} test(s) failed");
    }
    Ok(())
}

fn format_duration(d: Duration) -> String {
    if d.as_secs() == 0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f32())
    }
}
