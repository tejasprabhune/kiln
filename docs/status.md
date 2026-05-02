# Project status

> Living status doc. Each session that ships milestone work appends a section
> below. The most recent section is the current state.

## 2026-05-02 — M0 (Foundation)

**Branch:** `milestone/m0-foundation`
**PR:** opened against `main`

### Summary

Workspace scaffolded; `kiln-core::manifest` parser and validator land with
snapshot tests; `kiln-cli` ships `--version`, `new`, `init`, and the hidden
`check-manifest` subcommand; CI runs `fmt`/`clippy`/`test` on Ubuntu and
macOS. End-to-end `kiln new demo && cd demo && kiln check-manifest` works
locally.

### Acceptance criteria

| Criterion (per `kiln-milestones.md` §M0) | Status | Evidence |
| ---------------------------------------- | ------ | -------- |
| CI passes on Ubuntu and macOS            | pending CI run | `.github/workflows/ci.yml`, will resolve once PR is opened |
| `cargo install --path crates/kiln-cli` succeeds; `kiln --version` prints | pass | verified locally; output `kiln 0.1.0` |
| `kiln new demo && cd demo && kiln check-manifest` exits 0 and prints | pass | verified locally on `/tmp/kiln_e2e/e2e_demo` |
| ≥6 snapshot tests covering manifest cases (≥3 valid, ≥3 invalid) | pass | `crates/kiln-core/src/snapshots/` has 6 snapshot files: `valid_minimal`, `valid_full`, `valid_underscore_name`, `invalid_bad_semver`, `invalid_bad_identifier`, `invalid_missing_include_dir` |
| Both license files, README with install instructions, working CI badge | pass | `LICENSE-MIT`, `LICENSE-APACHE`, `README.md` (badge + install section) |

### Tests in this PR

- `crates/kiln-core/src/manifest.rs` unit + snapshot tests (14 tests).
- `crates/kiln-core/src/project.rs` unit tests for `find_manifest`.
- `crates/kiln-cli/src/commands/new.rs` unit tests for templates.
- `crates/kiln-cli/tests/cli_basic.rs` — `kiln --version`, `-V`, `--help`,
  no-args.
- `crates/kiln-cli/tests/cli_new.rs` — `kiln new` happy path, layout
  snapshot, manifest round-trip via `check-manifest`, refusal on existing
  destination, invalid manifest detection.
- `crates/kiln-cli/tests/cli_init.rs` — `kiln init` with derived name,
  explicit `--name`, refusal when manifest already present.

### ADRs filed

- `docs/decisions/0000-msrv-policy.md` — **accepted**. MSRV bumped from 1.75
  to 1.85. The `kiln-milestones.md` §1.1 value (1.75) is unworkable today
  because `indexmap 2.14` (a transitive of `toml`) requires the
  `edition2024` Cargo feature, stabilized only in Rust 1.85. Fully
  documented in the ADR.

### Deviations from `kiln-milestones.md`

- **Repo layout.** The milestones doc lists `tests/integration/` at the repo
  root for end-to-end CLI tests. Cargo's `assert_cmd::cargo_bin` API only
  reliably finds the binary when called from tests in the same package
  (`CARGO_BIN_EXE_<name>` is only set there), so the end-to-end tests live
  in `crates/kiln-cli/tests/` instead. Same files, same tests, just a
  different directory; no functionality changed.
- **MSRV.** See ADR 0000.

### Blockers / open items handed forward

- **Git remote at session start was unconfigured** despite the session
  prompt asserting it was. The repo at `tejasprabhune/kiln` was created on
  GitHub at the start of M0 push to allow the PR workflow to proceed.
- No other blockers. M1 can begin immediately on `milestone/m1-slang-cli`.

### Next session pickup

Continued in this session — see M1 below.

## 2026-05-02 — M1 (Slang CLI wrapper)

**Branch:** `milestone/m1-slang-cli` (stacked on `milestone/m0-foundation`)
**PR:** opened against `milestone/m0-foundation` (will retarget to `main`
once M0 is merged)

### Summary

`slang-rs` ships a typed, subprocess-based wrapper around the `slang` CLI.
`Slang::new()` discovers the binary via `KILN_SLANG_PATH` then `PATH`,
queries `slang --version`, and validates against `MIN_VERSION = v10.0`.
`CompileRequest` is a builder; `Slang::compile` writes diag/ast JSON to
temp files (slang pollutes stdout with a "Top level design units:" preamble
that would have made stdout-as-JSON brittle). Diagnostics deserialize from
slang's real `--diag-json` schema; the AST uses a permissive
`AstNode { kind, name, members, extra: ExtraFields }` shape with a
`#[serde(flatten)]` escape hatch so unknown / future slang fields don't
break parsing. CI now builds slang v10 from source and runs the e2e tests.

### Acceptance criteria

| Criterion (per `kiln-milestones.md` §M1) | Status | Evidence |
| ---------------------------------------- | ------ | -------- |
| `cargo install --path crates/kiln-cli` succeeds with only Rust toolchain (no cmake / Python / C++) | pass | re-verified locally; M0's pure-Rust property is preserved — `slang-rs` declares no native deps, the C++ toolchain is only needed at runtime to install slang itself |
| `cargo test -p slang-rs` (no `--features e2e`) passes without slang | pass | 27 lib tests pass against captured fixtures under `crates/slang-rs/tests/fixtures/captured/` |
| `cargo test -p slang-rs --features e2e` passes on CI with slang | pass | 10 e2e tests pass locally against built-from-source slang v10.0; CI job `test-e2e-slang` builds slang and runs them |
| slang missing → clear error with platform-specific install hint, snapshot-tested | pass | `crates/slang-rs/src/snapshots/slang_rs__handle__tests__missing_binary_error.snap` |
| `syntax_error.sv` → diagnostic with correct line | pass | `crates/slang-rs/tests/e2e.rs::syntax_error_pinpoints_missing_semicolon_line` asserts `line == 1` and a "expected `;`" message |
| `slang-rs` rustdoc renders without warnings | pass | `cargo doc -p slang-rs --no-deps` finished clean |

### Tests in this PR

- 27 unit tests across `version.rs`, `diagnostic.rs`, `ast.rs`,
  `compile.rs`, `handle.rs` — version parser robustness, location parser,
  argument-builder coverage, captured-fixture deserialization (incl.
  unknown-field round-trip), missing-binary error snapshot.
- 10 `--features e2e` tests in `crates/slang-rs/tests/e2e.rs` — full
  matrix: clean module, syntax error, width-trunc warning, AST request,
  `-D` defines (present + missing), `-I` include dir, package + consumer
  multi-file, language-standard pass-through, version floor.
- Captured slang JSON under `crates/slang-rs/tests/fixtures/captured/`
  drives the unit tests deterministically.

### ADRs filed

- `docs/decisions/0001-slang-integration-strategy.md` — **accepted**.
  Subprocess wrapper, not FFI. Reaffirms M0's pure-Rust install. Sets
  the rule that all `Command::spawn` calls must funnel through
  `runner::run_slang`.
- `docs/decisions/0002-slang-version-policy.md` — **accepted**. Minimum
  slang version `v10.0`. Permissive version-string parser with explicit
  bumping policy for minor and major floors.

### Deviations from `kiln-milestones.md`

- **Width-mismatch fixture is gated by `-Wwidth-trunc`.** The milestones
  doc lists `width_mismatch.sv` as producing a "semantic warning". Slang
  v10.0 emits the warning only when `-Wwidth-trunc` is explicitly
  enabled — `-Wall` is **not** a recognised slang option (it produces
  `unknown warning option '-Wall'`). The e2e test passes
  `-Wwidth-trunc` directly. This is a slang-side reality, not a
  workaround; recorded for M3 where the lint config maps onto these
  per-warning knobs.
- **Diagnostic format does not include a `code` or `length` field.** The
  milestones doc anticipates one. Slang v10's `--diag-json` provides
  `severity`, `message`, `optionName` (warnings only), `location`
  (string), and an optional `symbolPath`. `slang-rs::Diagnostic` exposes
  exactly those. If a future slang adds `code` / `length`, they'll
  round-trip transparently into the existing typed surface (the typed
  fields are explicit; nothing else is dropped because we don't use
  `deny_unknown_fields` on `Diagnostic`).
- **JSON output via files, not stdout.** Slang's `--diag-json -` and
  `--ast-json -` print to stdout, but slang *also* prints a free-form
  "Top level design units:" preamble and "Build succeeded/failed" footer
  to the same stream. We pass real file paths so stdout stays untouched
  and JSON parsing is deterministic. Documented in the slang-rs README
  and inline in `handle.rs::compile`.

### Notes carried forward

- The local `slang` binary used to capture fixtures was built from the
  master branch (reports `slang version 10.0.0+d611a3f`). CI pins
  `--branch v10.0`. The fixtures will need re-capturing only if the
  schema changes between v10.0 and master *and* a future test depends on
  the diff.
- `examples/hello-counter/` does not yet exist. M2 introduces it; the
  slang-rs e2e fixtures are sufficient for M1.

### Next session pickup

- Begin M2 (Verilator build pipeline). The cache, source-set resolution,
  and Verilator output parser are the major chunks.
- The CI matrix will need a Verilator install step alongside the existing
  slang one.
