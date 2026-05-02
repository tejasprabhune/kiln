# 0001. Slang integration strategy: subprocess wrapper, not FFI

- Status: accepted
- Date: 2026-05-02

## Context

`kiln` needs to drive Slang for two things:

1. **Diagnostics** for `kiln check` and `kiln build` (M2, M3).
2. **Typed AST** for `kiln-lint` rules and `kiln doc` (M3, M8).

Slang exposes these in three ways:

- `libslang` C++ shared library, with a stable ABI.
- The `slang` CLI binary, which accepts source files on its command line and
  emits diagnostics + an `--ast-json` dump on stdout.
- A TCP-based slang-server (LSP), which is a higher-level surface aimed at
  editors.

The naive choice for a Rust wrapper is FFI to `libslang`. That comes with
real costs:

1. **Build-time C++ toolchain.** `cargo install kiln` would require cmake +
   a C++17 compiler at the user's machine. That contradicts the
   product-level requirement that `kiln` itself builds with only the Rust
   toolchain (see `kiln-milestones.md` §1.1 and the **Things you will be
   tempted to do — don't** section in `CLAUDE.md`).
2. **Build-time slang sources.** Either we vendor slang (a multi-MB drop
   that needs syncing with upstream), bundle a build script that fetches
   it, or require the user to have it pre-installed for the bindings to
   link. Each path is a maintenance burden.
3. **API surface stability.** Slang's C++ API is *not* declared stable
   across minor versions. Slang's CLI flags and JSON schema, in contrast,
   change rarely and predictably.

The cost of not using FFI:

1. Per-call process startup. Empirically slang starts in ~50 ms on modern
   hardware. For interactive CLI use this is invisible. Long-running tools
   (LSP, watch-mode) would notice; we do not have those at M1.
2. JSON serialization of the AST. Multi-MB for large designs. Mitigatable
   with buffered I/O and (later) by delegating walking to slang via custom
   subcommands. For now this is acceptable.

## Decision

`slang-rs` is a subprocess wrapper around the `slang` CLI. No FFI, no C++
build dependency, no vendored sources. The user installs slang the same way
they install Verilator: a runtime dependency.

Implementation rules:

- All `Command::spawn` calls go through a single `run_slang()` helper. This
  is the only place that sets timeouts, captures stderr, and turns non-zero
  exits into structured errors.
- Binary discovery is cached on a `Slang` handle constructed once per CLI
  invocation. `Slang::new()` consults `KILN_SLANG_PATH`, then `PATH`.
  `Slang::with_path(p)` is also exposed for tests.
- Slang's version is queried once in `Slang::new()` (via `slang --version`)
  and stored on the handle. Version validation is enforced at construction
  time — see ADR 0002.
- The JSON AST output is parsed with `serde_json::from_reader` over a
  buffered reader. We do not slurp the entire stdout into a `String` first.
- Every typed AST node carries a `#[serde(flatten)] extra: serde_json::Value`
  field so unknown subtrees and new fields don't break deserialization.

## Consequences

Easier:

- `cargo install kiln` works with only `rustup` installed. End users do not
  need cmake or a C++ compiler.
- No coupling to slang's internal C++ API. Slang minor-version bumps are
  far less likely to break us than libslang ABI changes would.
- The wrapper layer is small, testable, and deterministic. Captured-stdout
  fixtures let unit tests run without slang installed.

Harder:

- Per-call process startup adds a fixed ~50 ms tax. This is acceptable for
  CLI-driven workflows, the only ones we ship in M1–M9. If we later add
  watch-mode or LSP-style hot paths, they will need a different strategy
  (probably running slang-server as a long-lived subprocess and talking
  LSP-over-stdio). That decision will live in a later ADR; it does not
  reverse this one.
- Diagnostic surface is what slang prints, not what libslang exposes
  internally. If we ever need richer error data (e.g., type-level
  context), we will lobby upstream to expose it via the CLI, not bridge to
  libslang.

## Reversal

Reversing this decision (going to FFI) requires a new ADR that explicitly
supersedes this one and addresses the cmake / C++-toolchain / vendoring
question raised above. Do not silently introduce FFI bindings.
