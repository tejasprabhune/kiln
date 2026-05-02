use std::process::ExitCode;

use clap::Parser;

mod commands;
mod render;

use commands::Cli;

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();
    match cli.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("KILN_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
