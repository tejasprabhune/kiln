# 0002. Slang minimum-version policy

- Status: accepted
- Date: 2026-05-02

## Context

`slang-rs` invokes the user-installed `slang` CLI. The CLI's flags and JSON
output schema are stable across most minor releases, but not all of them:
specifically, `--diag-json` and `--ast-json-source-info` were introduced in
later releases, and the AST JSON kind names have shifted slightly over time.

We need a clear policy for:

1. Which slang versions `slang-rs` claims to work with.
2. How a too-old slang is detected and reported to the user.
3. When and why we move the floor.

## Decision

**Minimum supported slang version: `v10.0`.**

This is the latest tagged release at the time of M1 (2026-05). v10.0
includes both `--diag-json` and `--ast-json-source-info`, the two flags
`slang-rs` relies on as of M1.

Discovery:

- `Slang::new()` invokes `slang --version`, parses the first line of stdout,
  and stores a `SlangVersion { major, minor, patch, raw }` on the handle.
- `Slang::check_version()` returns `Err(SlangError::UnsupportedVersion)` if
  `(major, minor) < (10, 0)`. The error formats with both the found version
  and the platform-specific install hint.
- The version comparison uses tuple ordering, not full semver, since
  slang's version strings have varied (e.g., `slang 7.0`, `slang version
  10.0+abcdef`). The parser is permissive: it pulls out the first
  `<digits>.<digits>(.<digits>)?` it sees on the first non-empty line.

Error format:

```
slang at /usr/local/bin/slang reports version 8.2, but slang-rs requires v10.0 or newer.
Install a newer slang: build from https://github.com/MikePopoloski/slang
```

Bumping the minimum:

- Bumping the minor floor (e.g., `v10.0 → v10.2`) requires a one-line PR
  updating `MIN_VERSION` and the snapshot test for the error message.
- Bumping the major floor (e.g., `v10.x → v11.0`) requires a new ADR that
  supersedes this one and lists which behaviors of the older majors we are
  giving up on.

## Consequences

Easier:

- Every `slang-rs` API can assume both `--diag-json` and structured AST
  JSON. Callers don't need to fall back to the human-readable diagnostic
  format. Substantially simpler than maintaining two diagnostic parsers.
- The version-error message is centralized — every code path that requires
  slang flows through `Slang::new()` → `check_version()`, so the user sees
  a consistent error regardless of which subcommand triggered the check.

Harder:

- Users on older distros that ship pre-v10 slang will need to build slang
  from source. The kiln-side workaround is documented in
  `crates/slang-rs/README.md` and surfaced inline in the version-error.
  This is the same expectation as for Verilator (M2 requires a recent
  Verilator), so the install pattern is consistent across kiln's
  external-tool wrappers.

## What this ADR does *not* commit to

- A maximum supported version. Slang's CLI is stable enough that we
  optimistically support every version `≥ v10.0`. If a future release
  breaks something, that's the trigger for the next ADR — not this one.
- Tracking pre-release / git-HEAD slang. We pin to tagged releases only.
