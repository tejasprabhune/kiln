//! `kiln watch <subcommand>` — re-run a subcommand whenever sources change.
//!
//! Watches the project root recursively, ignoring `target/` and `.git/`.
//! Filesystem events are debounced (200 ms) so a single editor save doesn't
//! fire twice. Exits cleanly on Ctrl-C.
//!
//! Subcommand failures are reported and the loop keeps going — same shape
//! as `cargo watch`.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use kiln_core::find_manifest;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};

use crate::reporter;

const DEBOUNCE: Duration = Duration::from_millis(200);

/// File extensions that should trigger a re-run when changed.
const TRIGGER_EXTS: &[&str] = &["sv", "svh", "v", "toml", "lock", "hex", "elf"];

/// Path components that mean "ignore this subtree."
const IGNORE_PARTS: &[&str] = &["target", ".git", "node_modules", ".kiln-cache"];

/// What to run on each tick.
pub fn run(subcommand: WatchSubcommand) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();

    reporter::status(
        "Watching",
        format!("`{}` for {}", project_root.display(), subcommand.label()),
    );

    // Run once immediately so the user sees current state without waiting
    // for an edit, mirroring cargo-watch's default.
    run_once(&subcommand);

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(DEBOUNCE, move |res| {
        let _ = tx.send(res);
    })
    .context("creating filesystem debouncer")?;
    debouncer
        .watcher()
        .watch(&project_root, RecursiveMode::Recursive)
        .context("starting filesystem watch")?;

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                if events
                    .iter()
                    .any(|e| should_trigger(&e.path, &project_root))
                {
                    let when = Instant::now();
                    let triggered = events
                        .iter()
                        .find(|e| should_trigger(&e.path, &project_root))
                        .map(|e| e.path.display().to_string())
                        .unwrap_or_else(|| "<unknown>".into());
                    reporter::status(
                        "Changed",
                        format!(
                            "{} ({} event{})",
                            triggered,
                            events.len(),
                            if events.len() == 1 { "" } else { "s" }
                        ),
                    );
                    run_once(&subcommand);
                    let dt = when.elapsed();
                    tracing::debug!(target: "kiln-cli", ms = dt.as_millis(), "watch tick complete");
                }
            }
            Ok(Err(err)) => {
                reporter::info("Watch", format!("notify error: {err}"));
            }
            Err(_) => {
                // Channel closed (debouncer dropped). Exit cleanly.
                break;
            }
        }
    }
    Ok(())
}

fn run_once(sub: &WatchSubcommand) {
    let started = Instant::now();
    let outcome = match sub {
        WatchSubcommand::Check => crate::commands::check_for_watch(),
        WatchSubcommand::Build => crate::commands::build_for_watch(),
        WatchSubcommand::Test(filters) => crate::commands::test_for_watch(filters.clone()),
        WatchSubcommand::Fmt => crate::commands::fmt_for_watch(),
    };
    let elapsed = started.elapsed();
    match outcome {
        Ok(()) => reporter::status(
            "Watch",
            reporter::green(&format!("ok in {}", fmt_dur(elapsed))),
        ),
        Err(e) => {
            reporter::status(
                "Watch",
                reporter::red(&format!("failed in {}: {e}", fmt_dur(elapsed))),
            );
        }
    }
}

fn fmt_dur(d: Duration) -> String {
    if d.as_secs() == 0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f32())
    }
}

/// True if a filesystem event on `path` should trigger a rerun.
fn should_trigger(path: &Path, project_root: &Path) -> bool {
    // Ignore paths inside any IGNORE_PARTS subtree.
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    for part in rel.components() {
        let s = part.as_os_str().to_string_lossy();
        if IGNORE_PARTS.iter().any(|ig| s == *ig) {
            return false;
        }
    }
    // Trigger only on files with a known extension.
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    TRIGGER_EXTS.iter().any(|t| t.eq_ignore_ascii_case(ext))
}

/// Subcommands that `kiln watch` knows how to drive.
#[derive(Debug, Clone)]
pub enum WatchSubcommand {
    Check,
    Build,
    /// Test with optional filter substrings.
    Test(Vec<String>),
    Fmt,
}

impl WatchSubcommand {
    pub fn label(&self) -> &'static str {
        match self {
            WatchSubcommand::Check => "check",
            WatchSubcommand::Build => "build",
            WatchSubcommand::Test(_) => "test",
            WatchSubcommand::Fmt => "fmt --check",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn should_trigger_on_sv_file() {
        let root = PathBuf::from("/proj");
        assert!(should_trigger(&root.join("src/top.sv"), &root));
        assert!(should_trigger(&root.join("src/inc.svh"), &root));
        assert!(should_trigger(&root.join("Kiln.toml"), &root));
    }

    #[test]
    fn should_skip_target_and_git() {
        let root = PathBuf::from("/proj");
        assert!(!should_trigger(
            &root.join("target/kiln/abc/Vfoo.bin"),
            &root
        ));
        assert!(!should_trigger(&root.join(".git/HEAD"), &root));
    }

    #[test]
    fn should_skip_unknown_extension() {
        let root = PathBuf::from("/proj");
        assert!(!should_trigger(&root.join("notes.md"), &root));
        assert!(!should_trigger(&root.join("Makefile"), &root));
    }

    #[test]
    fn should_match_extension_case_insensitively() {
        let root = PathBuf::from("/proj");
        assert!(should_trigger(&root.join("src/top.SV"), &root));
    }
}
