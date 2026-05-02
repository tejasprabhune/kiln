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

- Begin M1 (slang CLI wrapper). Tasks ordered:
  1. ADR `0001-slang-integration-strategy.md` (subprocess vs. FFI).
  2. ADR `0002-slang-version-policy.md` (minimum slang version, source of
     `slang --version`).
  3. `Slang` struct + binary discovery + version validation.
  4. `CompileRequest` builder, `run_slang` helper, `CompileResult` /
     `Diagnostic` / `Ast` types.
  5. Captured-output fixtures + unit tests (no slang on PATH required).
  6. `--features e2e` integration tests gated to CI.
- Local note: `slang` is **not** installed on the dev machine that ran M0.
  Unit tests for `slang-rs` will run locally; e2e tests need either a
  `brew install slang` step or to be deferred to CI. The CI image will need
  a `slang` install added in M1.
