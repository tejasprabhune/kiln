//! Cargo-style status reporter.
//!
//! Cargo's pattern: a 12-char right-aligned bold-green verb on the left,
//! then a free-form message. Status goes to stderr so it doesn't pollute
//! command stdout (test PASS/FAIL, format diffs, etc., which callers
//! might pipe).
//!
//! No external deps. ANSI escapes are emitted only when stderr is a TTY
//! and the user hasn't set `NO_COLOR=1`.

use std::fmt;
use std::io::{IsTerminal, Write};
use std::sync::OnceLock;

const GREEN_BOLD: &str = "\x1b[1;32m";
const YELLOW_BOLD: &str = "\x1b[1;33m";
const RED_BOLD: &str = "\x1b[1;31m";
const CYAN_BOLD: &str = "\x1b[1;36m";
const WHITE_BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Cached process-wide reporter. Configured once in `main`.
static REPORTER: OnceLock<Reporter> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub struct Reporter {
    use_color: bool,
    verbose: bool,
}

impl Reporter {
    /// Initialise the global reporter. Subsequent calls are no-ops.
    pub fn init(verbose: bool) {
        let use_color = should_use_color();
        let _ = REPORTER.set(Reporter { use_color, verbose });
    }

    fn get() -> Reporter {
        *REPORTER.get_or_init(|| Reporter {
            use_color: should_use_color(),
            verbose: false,
        })
    }

    fn paint(&self, color: &str, s: &str) -> String {
        if self.use_color {
            format!("{color}{s}{RESET}")
        } else {
            s.to_string()
        }
    }
}

fn should_use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
        return true;
    }
    std::io::stderr().is_terminal()
}

/// Print a status line: 12-char right-aligned bold-green verb + message.
pub fn status(verb: &str, message: impl fmt::Display) {
    let r = Reporter::get();
    let _ = writeln!(
        std::io::stderr(),
        "{:>12} {message}",
        r.paint(GREEN_BOLD, verb)
    );
}

/// Cyan-bold "informational" prefix. Used for things that aren't a
/// success and aren't a warning; e.g., "Cache hit", "Skipping".
pub fn info(verb: &str, message: impl fmt::Display) {
    let r = Reporter::get();
    let _ = writeln!(
        std::io::stderr(),
        "{:>12} {message}",
        r.paint(CYAN_BOLD, verb)
    );
}

/// Yellow `warning:` prefix. Unused for now, kept on the public surface
/// because every other CLI helper exposes the matching severity.
#[allow(dead_code)]
pub fn warning(message: impl fmt::Display) {
    let r = Reporter::get();
    let _ = writeln!(
        std::io::stderr(),
        "{} {message}",
        r.paint(YELLOW_BOLD, "warning:")
    );
}

/// Red bold `error:` prefix.
pub fn error(message: impl fmt::Display) {
    let r = Reporter::get();
    let _ = writeln!(
        std::io::stderr(),
        "{} {message}",
        r.paint(RED_BOLD, "error:")
    );
}

/// Verbose-only status line, prefixed with a dim 12-char verb. Hidden
/// unless `-v` was passed (or `KILN_LOG=debug` is set).
pub fn debug(verb: &str, message: impl fmt::Display) {
    let r = Reporter::get();
    if !r.verbose {
        return;
    }
    let _ = writeln!(std::io::stderr(), "{:>12} {message}", r.paint(DIM, verb));
}

/// True if `stderr` is a TTY and color is enabled. Useful for callers
/// that want to inline color in their own multi-line output.
#[allow(dead_code)]
pub fn use_color() -> bool {
    Reporter::get().use_color
}

/// Wrap a static string in green bold (success). Returns the original
/// string when colors are off.
pub fn green(s: &str) -> String {
    Reporter::get().paint(GREEN_BOLD, s)
}

/// Wrap a static string in red bold (error / failure).
pub fn red(s: &str) -> String {
    Reporter::get().paint(RED_BOLD, s)
}

/// Wrap a static string in yellow bold (warning).
pub fn yellow(s: &str) -> String {
    Reporter::get().paint(YELLOW_BOLD, s)
}

/// Wrap a static string in dim (deemphasised).
pub fn dim(s: &str) -> String {
    Reporter::get().paint(DIM, s)
}

/// Wrap a string in bold white. Used for neutral headers (not success/warn/error).
pub fn bold_white(s: &str) -> String {
    Reporter::get().paint(WHITE_BOLD, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_no_color_is_passthrough() {
        let r = Reporter {
            use_color: false,
            verbose: false,
        };
        assert_eq!(r.paint(GREEN_BOLD, "hi"), "hi");
    }

    #[test]
    fn paint_with_color_wraps() {
        let r = Reporter {
            use_color: true,
            verbose: false,
        };
        let painted = r.paint(GREEN_BOLD, "hi");
        assert!(painted.starts_with(GREEN_BOLD));
        assert!(painted.ends_with(RESET));
        assert!(painted.contains("hi"));
    }
}
