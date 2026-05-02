# `kiln` architecture overview

> **Source of truth for the roadmap is `kiln-milestones.md`.** This document
> is a thin one-page orientation; if it conflicts with `kiln-milestones.md`,
> the milestones doc wins.

## Mental model

`kiln` is a Cargo-style front door for the SystemVerilog open-source
toolchain. It does not reimplement parsers, simulators, or formatters; it
orchestrates existing best-in-class tools behind a single, opinionated
binary.

| Layer        | Tool wrapped                  | Crate           | Milestone introduced |
| ------------ | ----------------------------- | --------------- | -------------------- |
| Parsing / lint | `slang`                     | `slang-rs`, `kiln-lint` | M1, M3              |
| Build / sim  | `verilator`                   | `kiln-build`    | M2                   |
| Formatting   | `verible-verilog-format`      | `kiln-fmt`      | M6                   |
| Dependencies | `bender` (library)            | `kiln-deps`     | M4                   |
| Waveforms    | `surfer`                      | `kiln-wave`     | M7                   |
| Docs         | `slang-rs` + custom site gen  | `kiln-doc`      | M8                   |

`kiln` itself is pure Rust and builds with only a Rust toolchain. The
external tools above are runtime dependencies that the user installs
separately. If any are missing at runtime, the responsible code path
emits a clear, actionable error naming the missing tool and the install
command for the user's platform.

## Crate graph (current, M0)

```
kiln-cli ──> kiln-core
            (manifest, project model, error types)

slang-rs (stub)
kiln-build, kiln-deps, kiln-lint, kiln-fmt, kiln-test, kiln-wave, kiln-doc
    (all stubs at M0; populated in their respective milestones)
```

## Data flow at the milestone we're at

At M0, the data flow is intentionally minimal:

```
Kiln.toml ──[serde + validation]──> Manifest ──> kiln check-manifest (prints)
                                            \──> kiln new / kiln init (writes template)
```

M1 introduces `slang-rs` and the subprocess invocation discipline (one
helper, version validation, captured-fixture tests). M2 introduces the
build cache and Verilator backend. From M3 onward, `kiln check` uses the
typed AST that `slang-rs` exposes.

## Key design principles

1. **Wrap, don't reimplement.** Every layer above is a wrapper.
2. **Pure-Rust build.** No cmake, Python, or C++ compiler at build time.
3. **Subprocess over FFI.** Slang specifically: see ADR
   `docs/decisions/0001-slang-integration-strategy.md` (added in M1).
4. **One place per external invocation.** Each wrapper crate funnels its
   subprocess calls through a single helper (`run_slang`, etc.) so timeouts,
   stderr capture, and version checks are not sprinkled across the codebase.
5. **Errors bubble.** Only `kiln-cli` renders errors for humans.
6. **Tests come with the code.** Every public function and every CLI command
   ships with at least one test in the same PR.

For the ground truth on what each milestone delivers, see
`kiln-milestones.md`.
