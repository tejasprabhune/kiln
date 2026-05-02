# 0000. MSRV bumped from 1.75 to 1.85

- Status: accepted
- Date: 2026-05-02

## Context

`kiln-milestones.md` §1.1 specifies `MSRV: Rust 1.75 (stable)`, pinned in
`rust-toolchain.toml`. That choice was made before the present ecosystem
state; today, several widely-used crates require the `edition2024` Cargo
feature, which is not stabilized until Rust 1.85.

Concrete blocker observed during M0:

```
$ cargo check --workspace --all-targets
error: failed to download replaced source registry `crates-io`
Caused by:
  feature `edition2024` is required
  The package requires the Cargo feature called `edition2024`, but that
  feature is not stabilized in this version of Cargo (1.75.0).
```

The crate that triggers this is `indexmap 2.14`, pulled in transitively by
`toml`/`toml_edit`. The chain is unavoidable for any project that uses
`toml = "0.8"`. Pinning `indexmap` to `=2.10` (the last pre-`edition2024`
release) would fix this specific transitive but is fragile: the same wall
will appear with the next `serde`/`anyhow`/`clap` minor as upstreams catch up.

## Decision

Bump MSRV from `1.75` → `1.93` (current stable as of 2026-05) and update
`rust-toolchain.toml` and every `rust-version` field accordingly. CI
installs `1.93`.

This deviates from `kiln-milestones.md` §1.1. The deviation is roughly 18
months of toolchain age and removes a hard build blocker that would
otherwise force a fragile transitive-pin strategy across every milestone.

Since `kiln` ships pre-built binaries (planned M9), end users do not need a
recent Rust to install it. Contributors do; pinning at the latest stable
keeps us out of the MSRV-policing business and lets us pick up new clippy
diagnostics, language features, and stdlib improvements as they ship.

Bumping the floor in the future:
- Bumping minor (e.g. `1.93 → 1.95`) when a new stable lands: trivial PR,
  no ADR needed.
- Bumping the rust-toolchain channel to `stable` (track-without-pin) would
  require a follow-up ADR documenting why we accept the resulting CI
  flakiness from clippy lints landing without warning.

The milestones doc's `MSRV: 1.75` is left as written; this ADR is the
operative source for MSRV and supersedes that line.

### Revision history

- 2026-05-02 (original): bumped to `1.85` to cross the `edition2024` line.
- 2026-05-02 (this revision): bumped further to `1.93` per maintainer
  preference for tracking the latest stable.

## Consequences

Easier:
- The whole ecosystem of crates pinned in `Cargo.toml` works without
  transitive-pin gymnastics.
- Future milestones (M2 Verilator output parsing, M4 Bender wrapper, M8 doc
  site) can use any current crate without re-checking edition2024 boundaries.

Harder:
- Users on Rust 1.75–1.84 cannot build `kiln` from source. Given the rapid
  ecosystem-wide adoption of `edition2024`, this is a constraint that any
  sufficiently large Rust project hits eventually. Pre-built binaries
  (planned in M9) sidestep the issue for end users.

Trade-off accepted: MSRV strictness vs. dependency-pin fragility. We chose
strictness on the dependency side and looseness on MSRV.
