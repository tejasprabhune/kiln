//! `kiln lint list` and `kiln lint explain`.

use anyhow::Result;
use kiln_core::lint_map::{self, CANONICAL_RULES};

pub fn run_list() -> Result<()> {
    println!("Canonical rules (apply to all supported tools):");
    for rule in CANONICAL_RULES {
        let tools: Vec<&str> = [
            rule.slang_option.map(|_| "slang"),
            rule.verilator_code.map(|_| "verilator"),
        ]
        .into_iter()
        .flatten()
        .collect();
        println!(
            "  {:<20} [{:?}]  ({})  {}",
            rule.canonical,
            rule.default_severity,
            tools.join(", "),
            rule.description,
        );
    }
    Ok(())
}

pub fn run_explain(name: &str) -> Result<()> {
    match lint_map::lookup(name) {
        Some(rule) => {
            println!("Rule: {}", rule.canonical);
            println!("Default severity: {:?}", rule.default_severity);
            if let Some(s) = rule.slang_option {
                println!("Slang option: {s}");
            }
            if let Some(v) = rule.verilator_code {
                println!("Verilator code: {v}");
            }
            println!();
            println!("{}", rule.description);
            // TODO: expand with longer description
        }
        None => {
            let suggestion = lint_map::suggest(name);
            if let Some(s) = suggestion {
                eprintln!("unknown lint rule `{name}`; did you mean `{s}`?");
            } else {
                eprintln!("unknown lint rule `{name}`");
                eprintln!("Run `kiln lint list` to see all known rules.");
            }
            anyhow::bail!("unknown lint rule `{name}`");
        }
    }
    Ok(())
}
