# kiln

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A Cargo-style CLI for SystemVerilog. `kiln` is an opinionated front door to
the open-source SystemVerilog toolchain — wrapping
[Slang](https://github.com/MikePopoloski/slang),
[Bender](https://github.com/pulp-platform/bender),
[Verilator](https://www.veripool.org/verilator/),
[Verible](https://github.com/chipsalliance/verible), and
[Surfer](https://surfer-project.org/) behind a single binary modeled on `cargo`.

> Status: pre-0.1.0. See `docs/status.md` for what shipped per milestone and
> `kiln-milestones.md` for the roadmap.

## Install

`kiln` itself builds with only a Rust toolchain (1.93+) — no cmake, Python,
or C++ compiler required at *kiln*'s build time:

```bash
cargo install --path crates/kiln-cli
```

`kiln` invokes external tools as subprocesses at runtime. Install the ones
you need for the commands you use:

| Tool                       | Used by                       | Install                                                   |
| -------------------------- | ----------------------------- | --------------------------------------------------------- |
| `slang`                    | `kiln check`, `kiln doc`      | build from <https://github.com/MikePopoloski/slang>       |
| `verilator` (≥ 5.0)        | `kiln build`/`run`/`test`     | `brew install verilator` / `apt install verilator` (≥ 22.10) / build from source |
| `bender`                   | `kiln add`/`update`/`tree`    | `cargo install bender`                                    |
| `verible-verilog-format`   | `kiln fmt`                    | download from <https://github.com/chipsalliance/verible/releases> |
| `surfer`                   | `kiln wave`                   | `brew install surfer-project/tap/surfer`                  |

If a tool is missing at runtime, the responsible subcommand emits a clear,
actionable error naming the tool and the install command.

## The full happy path

```bash
# 1. New project.
kiln new counter
cd counter

# 2. Edit src/counter.sv until it compiles.
kiln check                      # fast: slang elaboration only
kiln build                      # invokes verilator, caches under target/kiln/

# 3. Add a self-checking testbench under tests/<name>.sv that prints "PASS".
kiln test                       # discovers tests/, runs in parallel
kiln test smoke                 # substring filter
kiln test --trace               # also dumps FST waves to target/kiln/waves/
kiln wave                       # opens the most recent FST in surfer
kiln wave smoke                 # opens a specific test's FST

# 4. Add dependencies.
kiln add axi --git https://github.com/pulp-platform/axi.git --version 0.39
kiln tree                       # bender's dep graph
kiln update                     # refresh Kiln.lock
kiln build                      # dep sources are picked up automatically

# 5. Format and document.
kiln fmt                        # in place via verible-verilog-format
kiln fmt --check                # CI-friendly; non-zero exit on diff
kiln fmt --check --format json  # tool-friendly; see docs/json-output.md
kiln doc                        # static HTML site under target/doc/
```

See `examples/hello-counter/` for a working starting point and
`examples/with-deps/` for a path-based dependency.

## Project layout

This is a Cargo workspace. Each subcrate has its own README:

- `crates/kiln-cli` — the `kiln` binary
- `crates/kiln-core` — manifest, project model, shared error types
- `crates/kiln-build` — build pipeline + Verilator backend
- `crates/kiln-deps` — dependency resolution (wraps `bender`)
- `crates/kiln-lint` — linting via `slang-rs`
- `crates/kiln-fmt` — formatting via Verible
- `crates/kiln-test` — test runner
- `crates/kiln-wave` — Surfer integration
- `crates/kiln-doc` — documentation generation
- `crates/slang-rs` — pure-Rust subprocess wrapper around the `slang` CLI

## Architecture decisions

Important choices live as ADRs under `docs/decisions/`:

- `0000-msrv-policy.md` — Rust 1.93 (current stable).
- `0001-slang-integration-strategy.md` — wrap slang as a subprocess, not via FFI.
- `0002-slang-version-policy.md` — minimum slang `v10.0`.
- `0003-bender-integration.md` — wrap bender as a subprocess, with a path forward to its library API.

## Contributing

See `CONTRIBUTING.md`. The project plan and per-session rules of the road
live in `kiln-milestones.md` and `CLAUDE.md` at the repo root.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)

at your option.
