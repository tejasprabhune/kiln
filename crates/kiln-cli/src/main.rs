use std::error::Error;
use std::process::ExitCode;

use clap::Parser;

mod commands;
mod render;
mod reporter;

use commands::Cli;

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();
    reporter::Reporter::init(cli.global_verbose());
    match cli.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Pretty-print every cause level with an `error:` prefix and
            // dim continuation arrows.
            reporter::error(&err);
            let mut source: Option<&dyn Error> = err.source();
            while let Some(s) = source {
                eprintln!("       {} {s}", reporter::dim("↳"));
                source = s.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("KILN_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
