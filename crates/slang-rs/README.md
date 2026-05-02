# slang-rs

Pure-Rust subprocess wrapper around the
[slang](https://github.com/MikePopoloski/slang) SystemVerilog compiler CLI.

`slang-rs` shells out to a `slang` binary that the user installs separately.
It is **not** an FFI binding to libslang. This is a deliberate decision:

- `cargo install kiln` works with only a Rust toolchain — no cmake, no
  Python, no C++ compiler.
- The slang CLI surface is far more stable than the libslang ABI.
- Build-time complexity is shifted off the kiln side.

The decision is documented in
`docs/decisions/0001-slang-integration-strategy.md` of the kiln repo. Do not
reverse it without a new ADR.

## Runtime dependency

`slang-rs` requires the `slang` binary on `PATH`, or its path in the
`KILN_SLANG_PATH` environment variable. The minimum supported version is
`v10.0` (see `docs/decisions/0002-slang-version-policy.md`).

### Install slang

There is no Homebrew formula for the SystemVerilog `slang` (`brew install
slang` installs `s-lang`, a different language). Build from source:

```bash
git clone https://github.com/MikePopoloski/slang.git
cd slang
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
# Then add `slang/build/bin` to your PATH, or copy the binary somewhere
# on PATH like /usr/local/bin.
```

If `slang` is missing or too old, `slang-rs` returns a structured error
with a platform-specific install hint inline.

## Public API

```rust
use slang_rs::{Slang, CompileRequest};

let slang = Slang::new()?;                  // discovers binary, validates version
let req = CompileRequest::builder()
    .source("src/top.sv")
    .top("top")
    .want_ast(true)                         // also dump the AST
    .build();
let result = slang.compile(&req)?;
for diag in &result.diagnostics {
    println!("{:?}: {}", diag.severity, diag.message);
}
```

The full surface:

- [`Slang::new`] / [`Slang::with_path`] — construct a handle.
- [`Slang::version`] / [`Slang::check_version`] — version reporting.
- [`Slang::compile`] — run slang with a `CompileRequest`, get a
  `CompileResult { ast, diagnostics }`.
- [`CompileRequest::builder`] — typed builder for sources, include dirs,
  defines, top module, language standard, parse-only, AST request,
  passthrough args.
- [`Diagnostic`] / [`Severity`] — typed diagnostic wrapper over slang's
  `--diag-json`.
- [`Ast`] / [`AstNode`] — typed wrapper over slang's `--ast-json`. Each
  node carries a `kind` discriminator, an optional `name`, optional
  `members`, and an [`ExtraFields`] map for everything else, so unknown
  fields and new slang versions don't break deserialization.

## Tests

```bash
# Unit tests — work without slang installed; use captured fixtures.
cargo test -p slang-rs

# End-to-end tests — invoke the real `slang` binary.
cargo test -p slang-rs --features e2e
```

CI runs both. The e2e job builds slang v10.0 from source on each runner
(see `.github/workflows/ci.yml`).

## Implementation notes

- Every `Command::spawn` call goes through `runner::run_slang`, the only
  place that captures stdout/stderr and exit codes. This keeps timeouts,
  environment scrubbing, and error conversion in one place.
- AST and diagnostic JSON are written by slang to *files* (passed via
  `--ast-json <path>` and `--diag-json <path>`), not to stdout. Slang
  always writes a "Top level design units:..." preamble to stdout, so
  parsing stdout-as-JSON would be brittle. Files give us a clean read.
- Each AST node has `#[serde(flatten)] extra: ExtraFields`. Fields slang
  hasn't told us about (or that arrived in a newer minor) round-trip
  through this map without crashing the deserializer.
