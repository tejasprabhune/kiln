use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod build;
mod check;
mod check_manifest;
mod deps;
mod doc;
mod fmt;
mod new;
mod test;
mod wave;

/// The `kiln` CLI.
#[derive(Debug, Parser)]
#[command(
    name = "kiln",
    version,
    about = "A Cargo-style CLI for SystemVerilog",
    propagate_version = true
)]
pub struct Cli {
    /// Verbose output. Surfaces cache hits, subprocess invocations, and
    /// other dimmed status lines that are otherwise hidden.
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

impl Cli {
    pub fn global_verbose(&self) -> bool {
        self.verbose
    }
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

    /// Run a fast slang elaboration check (no Verilator).
    Check {
        /// Treat warnings as failures.
        #[arg(long)]
        deny_warnings: bool,
        #[arg(short, long)]
        verbose: bool,
    },

    /// Build the design with Verilator.
    Build {
        /// Build with optimization (`-O3`, `--x-assign 0`).
        #[arg(long)]
        release: bool,
        /// Verbose tracing.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Build and run the simulator binary. Args after `--` are forwarded.
    Run {
        #[arg(long)]
        release: bool,
        #[arg(short, long)]
        verbose: bool,
        /// Arguments forwarded to the simulator binary after `--`.
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Remove the kiln build cache (`target/kiln/`).
    Clean,

    /// Add a dependency to `Kiln.toml` and refresh `Kiln.lock`.
    #[command(disable_version_flag = true)]
    Add {
        /// Dependency name.
        name: String,
        /// Git URL.
        #[arg(long, group = "source")]
        git: Option<String>,
        /// Git revision (e.g., a tag or commit).
        #[arg(long, requires = "git")]
        rev: Option<String>,
        /// Git branch.
        #[arg(long, requires = "git")]
        branch: Option<String>,
        /// Semver version constraint (for git deps that publish tags).
        #[arg(long, requires = "git")]
        version: Option<String>,
        /// Local filesystem path.
        #[arg(long, group = "source")]
        path: Option<PathBuf>,
    },

    /// Remove a dependency from `Kiln.toml` and refresh `Kiln.lock`.
    Remove { name: String },

    /// Refresh `Kiln.lock` against `Kiln.toml`.
    Update,

    /// Print the dependency tree.
    Tree,

    /// Format SystemVerilog sources via verible-verilog-format.
    Fmt {
        /// Don't write changes; exit non-zero if any file would change.
        #[arg(long)]
        check: bool,
        /// Output format. `plain` is human-readable; `json` is for tools.
        #[arg(long, value_parser = ["plain", "json"], default_value = "plain")]
        format: String,
    },

    /// Discover and run testbenches.
    Test {
        /// Substring filter on test names.
        filter: Option<String>,
        /// Number of parallel jobs. Defaults to available parallelism.
        #[arg(short, long)]
        jobs: Option<usize>,
        /// Keep going after the first failure.
        #[arg(long)]
        no_fail_fast: bool,
        /// Print discovered tests, do not run.
        #[arg(long)]
        list: bool,
        /// Build with FST trace support and dump waves to target/kiln/waves/.
        #[arg(long)]
        trace: bool,
    },

    /// Generate a static documentation site under `target/doc/`.
    Doc {
        /// Open the generated index page in a browser.
        #[arg(long)]
        open: bool,
    },

    /// Open a recorded FST waveform in surfer.
    Wave {
        /// Test name. Defaults to the most recently produced FST.
        test: Option<String>,
        /// Print the FST path instead of opening surfer.
        #[arg(long)]
        print_path: bool,
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
            Command::Check {
                deny_warnings,
                verbose,
            } => check::run(deny_warnings, verbose),
            Command::Build { release, verbose } => build::run_build(release, verbose).map(|_| ()),
            Command::Run {
                release,
                verbose,
                args,
            } => build::run_run(release, verbose, args),
            Command::Clean => build::run_clean(),
            Command::Add {
                name,
                git,
                rev,
                branch,
                version,
                path,
            } => deps::run_add(name, git, rev, branch, version, path),
            Command::Remove { name } => deps::run_remove(name),
            Command::Update => deps::run_update(),
            Command::Tree => deps::run_tree(),
            Command::Fmt { check, format } => {
                let f = match format.as_str() {
                    "json" => fmt::OutputFormat::Json,
                    _ => fmt::OutputFormat::Plain,
                };
                fmt::run(check, f)
            }
            Command::Test {
                filter,
                jobs,
                no_fail_fast,
                list,
                trace,
            } => test::run(filter, jobs, no_fail_fast, list, trace),
            Command::Doc { open } => doc::run(open),
            Command::Wave { test, print_path } => wave::run(test, print_path),
            Command::CheckManifest { path } => check_manifest::run(path.as_deref()),
        }
    }
}
