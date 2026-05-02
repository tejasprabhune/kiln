# slang-rs

Pure-Rust subprocess wrapper around the `slang` SystemVerilog compiler CLI.

`slang-rs` shells out to a `slang` binary that the user installs separately
(e.g., `brew install slang`). It is **not** an FFI binding to libslang; this
is a deliberate decision documented in
`docs/decisions/0001-slang-integration-strategy.md`.

Stubbed at M0; the real implementation lands in M1.

## Tests

- `cargo test -p slang-rs` runs the unit tests, which use captured slang
  output stored under `tests/fixtures/captured/`. They do not require the
  `slang` binary to be installed.
- `cargo test -p slang-rs --features e2e` additionally runs end-to-end tests
  that invoke the real `slang` binary on `PATH`.
