//! Project-level lifecycle hook execution.
//!
//! Hooks are declared in `[hooks]` and run as single shell lines at the
//! project root. Pre-* hook failures abort the parent subcommand;
//! post-* hooks log on failure but never change the parent outcome.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};

use kiln_core::{HookPhase, Hooks};

use crate::reporter;

/// Run a pre-* hook. Returns `Err` with the failed command's stderr if
/// the hook exits non-zero or fails to spawn.
pub fn run_pre_hook(project_root: &Path, hooks: &Hooks, phase: HookPhase) -> Result<()> {
    let Some(cmd) = hooks.for_phase(phase) else {
        return Ok(());
    };
    reporter::status("Hook", format!("`{}`", short_cmd(cmd)));
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| anyhow!("failed to spawn `{cmd}`: {e}"))?;
    if !output.success() {
        return Err(anyhow!(
            "{} hook (`{cmd}`) exited with {}",
            phase_name(phase),
            output.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Run a post-* hook. Failures are logged but never returned.
pub fn run_post_hook(project_root: &Path, hooks: &Hooks, phase: HookPhase) {
    let Some(cmd) = hooks.for_phase(phase) else {
        return;
    };
    reporter::status("Hook", format!("`{}`", short_cmd(cmd)));
    let result = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match result {
        Ok(s) if !s.success() => {
            reporter::info(
                "Hook",
                format!(
                    "{} hook (`{cmd}`) exited with {} (continuing)",
                    phase_name(phase),
                    s.code().unwrap_or(-1)
                ),
            );
        }
        Err(e) => {
            reporter::info(
                "Hook",
                format!(
                    "failed to spawn {} hook (`{cmd}`): {e} (continuing)",
                    phase_name(phase)
                ),
            );
        }
        Ok(_) => {}
    }
}

fn phase_name(phase: HookPhase) -> &'static str {
    match phase {
        HookPhase::PreCheck => "pre-check",
        HookPhase::PreBuild => "pre-build",
        HookPhase::PreTest => "pre-test",
        HookPhase::PostTest => "post-test",
    }
}

/// Truncate a long shell line for display in status output.
fn short_cmd(cmd: &str) -> String {
    const MAX: usize = 60;
    if cmd.len() <= MAX {
        cmd.to_string()
    } else {
        format!("{}…", &cmd[..MAX])
    }
}
