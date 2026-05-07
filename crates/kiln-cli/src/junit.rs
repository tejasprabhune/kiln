//! JUnit XML emitter for `kiln test --reporter junit`.
//!
//! Targets the de-facto Jenkins/GitLab dialect:
//!
//! ```xml
//! <testsuites name="kiln-test" tests="N" failures="F" time="S">
//!   <testsuite name="<package>" tests="N" failures="F" time="S">
//!     <testcase name="..." classname="<package>" time="S" />
//!     <testcase name="..." classname="<package>" time="S">
//!       <failure type="exit" message="exited with N">stderr...</failure>
//!     </testcase>
//!     <testcase name="..." classname="<package>" time="S">
//!       <failure type="timeout" message="wallclock exceeded">stderr...</failure>
//!     </testcase>
//!   </testsuite>
//! </testsuites>
//! ```
//!
//! Captured stdout is included as `<system-out>` only when `verbose`
//! is true, mirroring the `-v` semantics of `kiln test`.

use std::fmt::Write;
use std::time::Duration;

/// One row in the report.
#[derive(Debug, Clone)]
pub struct JunitCase {
    pub name: String,
    pub classname: String,
    pub elapsed: Duration,
    pub outcome: JunitOutcome,
    /// Captured stdout. Included as `<system-out>` only if `verbose` is true.
    pub stdout: String,
}

/// Distinct outcome variants so consumers can render timeouts differently
/// from regular failures.
#[derive(Debug, Clone)]
pub enum JunitOutcome {
    Pass,
    Fail { message: String, stderr: String },
    Timeout { message: String, stderr: String },
    Error { message: String, stderr: String },
}

/// Render a complete JUnit XML document.
pub fn render(suite_name: &str, cases: &[JunitCase], verbose: bool) -> String {
    let total = cases.len();
    let failures = cases
        .iter()
        .filter(|c| !matches!(c.outcome, JunitOutcome::Pass))
        .count();
    let total_time: f64 = cases.iter().map(|c| c.elapsed.as_secs_f64()).sum();

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<testsuites name=\"kiln-test\" tests=\"{total}\" failures=\"{failures}\" time=\"{:.3}\">",
        total_time
    );
    let _ = writeln!(
        out,
        "  <testsuite name=\"{}\" tests=\"{total}\" failures=\"{failures}\" time=\"{:.3}\">",
        xml_escape(suite_name),
        total_time
    );
    for c in cases {
        let _ = write!(
            out,
            "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\"",
            xml_escape(&c.classname),
            xml_escape(&c.name),
            c.elapsed.as_secs_f64()
        );
        match &c.outcome {
            JunitOutcome::Pass => {
                if verbose && !c.stdout.is_empty() {
                    out.push_str(">\n");
                    let _ = writeln!(
                        out,
                        "      <system-out>{}</system-out>",
                        xml_escape(&c.stdout)
                    );
                    out.push_str("    </testcase>\n");
                } else {
                    out.push_str(" />\n");
                }
            }
            JunitOutcome::Fail { message, stderr }
            | JunitOutcome::Timeout { message, stderr }
            | JunitOutcome::Error { message, stderr } => {
                let kind = match &c.outcome {
                    JunitOutcome::Fail { .. } => "failure",
                    JunitOutcome::Timeout { .. } => "timeout",
                    JunitOutcome::Error { .. } => "error",
                    JunitOutcome::Pass => unreachable!(),
                };
                out.push_str(">\n");
                let _ = writeln!(
                    out,
                    "      <failure type=\"{kind}\" message=\"{}\">{}</failure>",
                    xml_escape(message),
                    xml_escape(stderr)
                );
                if verbose && !c.stdout.is_empty() {
                    let _ = writeln!(
                        out,
                        "      <system-out>{}</system-out>",
                        xml_escape(&c.stdout)
                    );
                }
                out.push_str("    </testcase>\n");
            }
        }
    }
    out.push_str("  </testsuite>\n");
    out.push_str("</testsuites>\n");
    out
}

/// Escape the five XML predefined entities. JUnit consumers also choke on
/// stray control characters (other than `\t`, `\n`, `\r`); strip those.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(ch),
            c if (c as u32) < 0x20 => {
                // Drop other ASCII control characters; XML 1.0 forbids them.
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(name: &str, outcome: JunitOutcome) -> JunitCase {
        JunitCase {
            name: name.to_string(),
            classname: "demo".to_string(),
            elapsed: Duration::from_millis(120),
            outcome,
            stdout: String::new(),
        }
    }

    #[test]
    fn empty_suite_renders_zero_counts() {
        let xml = render("demo", &[], false);
        assert!(xml.contains("tests=\"0\""));
        assert!(xml.contains("failures=\"0\""));
        assert!(xml.contains("</testsuites>"));
    }

    #[test]
    fn pass_renders_self_closing_testcase() {
        let xml = render("demo", &[case("p", JunitOutcome::Pass)], false);
        assert!(xml.contains("<testcase"));
        assert!(xml.contains("name=\"p\""));
        assert!(xml.contains("/>"));
    }

    #[test]
    fn fail_includes_failure_element_with_stderr() {
        let xml = render(
            "demo",
            &[case(
                "f",
                JunitOutcome::Fail {
                    message: "exit 1".into(),
                    stderr: "Assertion failed at line 4".into(),
                },
            )],
            false,
        );
        assert!(xml.contains("<failure type=\"failure\""));
        assert!(xml.contains("message=\"exit 1\""));
        assert!(xml.contains("Assertion failed at line 4"));
    }

    #[test]
    fn timeout_uses_distinct_failure_type() {
        let xml = render(
            "demo",
            &[case(
                "slow",
                JunitOutcome::Timeout {
                    message: "wallclock exceeded".into(),
                    stderr: "<timeout>".into(),
                },
            )],
            false,
        );
        assert!(xml.contains("<failure type=\"timeout\""));
    }

    #[test]
    fn xml_special_chars_in_messages_are_escaped() {
        let xml = render(
            "demo",
            &[case(
                "weird",
                JunitOutcome::Fail {
                    message: "<bad> & \"oops\"".into(),
                    stderr: "stack: <unwind>".into(),
                },
            )],
            false,
        );
        assert!(xml.contains("&lt;bad&gt;"));
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&quot;oops&quot;"));
        assert!(xml.contains("&lt;unwind&gt;"));
    }

    #[test]
    fn control_chars_stripped() {
        let xml = render(
            "demo",
            &[case(
                "ctrl",
                JunitOutcome::Fail {
                    message: "ok".into(),
                    stderr: "before\x07after".into(),
                },
            )],
            false,
        );
        assert!(xml.contains("beforeafter"));
        assert!(!xml.contains('\x07'));
    }

    #[test]
    fn verbose_includes_system_out_for_passes() {
        let mut c = case("p", JunitOutcome::Pass);
        c.stdout = "PASSED!".into();
        let quiet = render("demo", std::slice::from_ref(&c), false);
        assert!(!quiet.contains("system-out"));
        let verbose = render("demo", std::slice::from_ref(&c), true);
        assert!(verbose.contains("<system-out>PASSED!</system-out>"));
    }

    #[test]
    fn time_aggregates_at_the_suite_level() {
        let mut c1 = case("a", JunitOutcome::Pass);
        c1.elapsed = Duration::from_millis(500);
        let mut c2 = case("b", JunitOutcome::Pass);
        c2.elapsed = Duration::from_millis(750);
        let xml = render("demo", &[c1, c2], false);
        // 0.500 + 0.750 = 1.250
        assert!(xml.contains("time=\"1.250\""));
    }
}
