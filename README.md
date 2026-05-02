# kiln

[![CI](https://github.com/tejasprabhune/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/tejasprabhune/kiln/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A Cargo-style CLI for SystemVerilog. `kiln` is an opinionated front door to
the open-source SystemVerilog toolchain — wrapping
[Slang](https://github.com/MikePopoloski/slang),
[Bender](https://github.com/pulp-platform/bender),
[Verilator](https://www.veripool.org/verilator/),
[Verible](https://github.com/chipsalliance/verible), and
[Surfer](https://surfer-project.org/) behind a single binary modeled on `cargo`.

> Status: under active development. See `kiln-milestones.md` for the roadmap
> and `docs/status.md` for what's currently shipping.

## Install

`kiln` itself builds with only a Rust toolchain — no cmake, Python, or C++
compiler required:

```bash
cargo install --path crates/kiln-cli
```

`kiln` invokes external tools as subprocesses at runtime. Install the ones you
need:

| Tool      | Used by                | Install (macOS)            |
| --------- | ---------------------- | -------------------------- |
| `slang`   | `kiln check`           | `brew install slang`       |
| `verilator` | `kiln build`/`kiln run` | `brew install verilator`   |
| `verible-verilog-format` | `kiln fmt`  | `brew install verible`     |
| `surfer`  | `kiln wave`            | `brew install surfer-project/tap/surfer` |

## Quick start

```bash
kiln new my_design
cd my_design
kiln check
kiln build
kiln run
```

## Project layout

This is a Cargo workspace. Each subcrate has its own README:

- `crates/kiln-cli` — the `kiln` binary
- `crates/kiln-core` — manifest, project model, shared error types
- `crates/kiln-build` — build pipeline + simulator backends
- `crates/kiln-deps` — dependency resolution (wraps `bender`)
- `crates/kiln-lint` — linting via `slang-rs`
- `crates/kiln-fmt` — formatting via Verible
- `crates/kiln-test` — test runner
- `crates/kiln-wave` — Surfer integration
- `crates/kiln-doc` — documentation generation
- `crates/slang-rs` — pure-Rust subprocess wrapper around the `slang` CLI

## Contributing

See `CONTRIBUTING.md`. The high-level rules of the road also live in
`CLAUDE.md` at the repo root.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)

at your option.
