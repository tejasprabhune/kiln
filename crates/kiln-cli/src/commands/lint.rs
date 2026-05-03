//! `kiln lint list` and `kiln lint explain`.

use anyhow::Result;
use kiln_core::lint_map::{self, CANONICAL_RULES};
use kiln_core::LintSeverity;

use crate::reporter;

pub fn run_list() -> Result<()> {
    eprintln!("{:>12} canonical rules\n", reporter::bold_white("Lint"));
    for rule in CANONICAL_RULES {
        let tools: Vec<&str> = [
            rule.slang_option.map(|_| "slang"),
            rule.verilator_code.map(|_| "verilator"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let sev = severity_label(rule.default_severity);
        eprintln!(
            "  {:<20} {}  {}  {}",
            rule.canonical,
            sev,
            reporter::dim(&format!("({})", tools.join(", "))),
            rule.description,
        );
    }
    Ok(())
}

pub fn run_explain(name: &str) -> Result<()> {
    match lint_map::lookup(name) {
        Some(rule) => {
            eprintln!("{:>12} {}", reporter::bold_white("Rule"), rule.canonical);
            eprintln!(
                "{:>12} {}",
                reporter::dim("severity"),
                severity_label(rule.default_severity)
            );
            if let Some(s) = rule.slang_option {
                eprintln!("{:>12} {s}", reporter::dim("slang"));
            }
            if let Some(v) = rule.verilator_code {
                eprintln!("{:>12} {v}", reporter::dim("verilator"));
            }
            eprintln!();
            eprintln!("  {}", rule.description);
        }
        None => {
            let suggestion = lint_map::suggest(name);
            if let Some(s) = suggestion {
                reporter::error(format!("unknown lint rule `{name}`; did you mean `{s}`?"));
            } else {
                reporter::error(format!("unknown lint rule `{name}`"));
                eprintln!(
                    "       {} run `kiln lint list` to see all known rules",
                    reporter::dim("hint:")
                );
            }
            anyhow::bail!("unknown lint rule `{name}`");
        }
    }
    Ok(())
}

fn severity_label(sev: LintSeverity) -> String {
    match sev {
        LintSeverity::Error => reporter::red("error"),
        LintSeverity::Warn => reporter::yellow("warn"),
        LintSeverity::Off | LintSeverity::Deny => reporter::dim("off"),
    }
}
