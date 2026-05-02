use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod check_manifest;
mod new;

/// The `kiln` CLI.
#[derive(Debug, Parser)]
#[command(
    name = "kiln",
    version,
    about = "A Cargo-style CLI for SystemVerilog",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new kiln project in `<name>/`.
    New {
        /// The name of the new project. Must be a valid SystemVerilog identifier.
        name: String,
        /// Where to create the new project. Defaults to the current directory.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,
    },

    /// Initialize a kiln project in the current directory.
    Init {
        /// The package name. Defaults to the directory name.
        #[arg(long)]
        name: Option<String>,
    },

    /// Parse `Kiln.toml` and print the resolved manifest. Used by tests.
    #[command(hide = true)]
    CheckManifest {
        /// Path to the manifest. Defaults to walking up from CWD.
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::New { name, path } => new::run_new(&name, path.as_deref()),
            Command::Init { name } => new::run_init(name.as_deref()),
            Command::CheckManifest { path } => check_manifest::run(path.as_deref()),
        }
    }
}
