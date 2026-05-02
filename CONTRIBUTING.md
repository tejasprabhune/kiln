# Contributing to kiln

Thanks for your interest. This document covers local development, testing,
and the architecture-decision-record (ADR) process.

## Local setup

You need a Rust toolchain (1.85+, pinned in `rust-toolchain.toml`; see ADR
`docs/decisions/0000-msrv-policy.md` for why we deviated from the milestones
doc's stated 1.75). For
runtime functionality you also need the external tools `kiln` invokes:

```bash
brew install slang verilator verible surfer-project/tap/surfer
```

`kiln` itself builds with only the Rust toolchain — that is a hard product
requirement. Do not add cmake, Python, or a C++ compiler as a build-time
dependency.

## Common commands

```bash
# Format check + lint + test (run before every commit).
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features

# Build just the CLI.
cargo build -p kiln-cli

# Install locally for end-to-end testing.
cargo install --path crates/kiln-cli --debug

# Run slang-rs tests that require the slang binary on PATH.
cargo test -p slang-rs --features e2e

# Update snapshot tests after a deliberate output change.
cargo insta review
```

## Code conventions

- No `unwrap()` or `expect()` in non-test code. Use `?` with `anyhow` in
  binaries, `thiserror` in libraries.
- No `println!` outside `crates/kiln-cli`. Use `tracing::{info, warn, error}`.
- Tests come with the code, not after. Every public function gets at least
  one test. Every CLI command gets at least one integration test.
- Conventional Commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`,
  `chore:`. First line ≤ 72 chars; body explains *why*.

## ADR process

If you hit a design decision that affects more than the current task — a
trade-off, a versioning policy, an external-tool integration strategy — write
an ADR.

```bash
docs/decisions/NNNN-short-kebab-title.md
```

`NNNN` is zero-padded and monotonically increasing. The template:

```markdown
# NNNN. Short title

- Status: proposed | accepted | superseded by NNNN
- Date: YYYY-MM-DD

## Context

What is the problem? What are the constraints?

## Decision

What we are doing.

## Consequences

What becomes easier? What becomes harder? What did we trade off?
```

If a task is genuinely blocked, file an ADR with status `proposed`, stop that
task, and move to the next one in the same milestone.

## Branches and PRs

One milestone per branch, one PR per milestone. Branch names:
`milestone/m0-foundation`, `milestone/m1-slang-cli`, etc.

PR descriptions list which acceptance criteria from `kiln-milestones.md` they
address, and link to the test(s) that prove each one.
