//! `kiln env` — print discovered external tools and their versions.
//!
//! Useful for bug reports, CI sanity checks, and answering the question
//! "is my slang/verilator/bender/verible/surfer install actually being
//! picked up?"

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::reporter;

/// Tools kiln drives. The `version_arg` is the flag that prints a
/// version line on stdout.
const TOOLS: &[(&str, &str)] = &[
    ("slang", "--version"),
    ("verilator", "--version"),
    ("verible-verilog-format", "--version"),
    ("bender", "--version"),
    ("surfer", "--version"),
];

#[derive(Debug)]
pub struct ToolReport {
    pub name: &'static str,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

pub fn collect() -> Vec<ToolReport> {
    let mut out = Vec::with_capacity(TOOLS.len());
    for (name, version_arg) in TOOLS {
        let path = which_on_path(name);
        let version = path
            .as_ref()
            .and_then(|p| run_version(p, version_arg))
            .map(|s| s.lines().next().unwrap_or(&s).trim().to_string());
        out.push(ToolReport {
            name,
            path,
            version,
        });
    }
    out
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn run_version(bin: &std::path::Path, arg: &str) -> Option<String> {
    let out = Command::new(bin)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    if s.trim().is_empty() {
        let s = String::from_utf8_lossy(&out.stderr);
        if s.trim().is_empty() {
            None
        } else {
            Some(s.into_owned())
        }
    } else {
        Some(s.into_owned())
    }
}

pub fn run() -> Result<()> {
    let reports = collect();
    let kiln_version = env!("CARGO_PKG_VERSION");
    println!("kiln {kiln_version}");
    println!("rustc {}", rustc_version().unwrap_or_else(|| "?".into()));
    println!();
    println!("external tools (kiln drives these as subprocesses):");
    let name_width = TOOLS.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for r in &reports {
        match (&r.path, &r.version) {
            (Some(p), Some(v)) => {
                println!(
                    "  {:width$}  {}  {}",
                    r.name,
                    v,
                    reporter::dim(&p.display().to_string()),
                    width = name_width
                );
            }
            (Some(p), None) => {
                println!(
                    "  {:width$}  {}  {}",
                    r.name,
                    reporter::yellow("(version probe failed)"),
                    reporter::dim(&p.display().to_string()),
                    width = name_width
                );
            }
            (None, _) => {
                println!(
                    "  {:width$}  {}",
                    r.name,
                    reporter::red("not found on PATH"),
                    width = name_width
                );
            }
        }
    }
    Ok(())
}

fn rustc_version() -> Option<String> {
    let out = Command::new("rustc").arg("--version").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    Some(s.trim().to_string())
}
