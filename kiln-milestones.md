# `kiln` — A Cargo-style CLI for SystemVerilog

> A unified, opinionated front door to the open-source SystemVerilog toolchain.
> Wraps best-in-class existing tools (Slang, Bender, Verilator, Verible, Surfer)
> behind a single `kiln` binary modeled on `cargo`.

---

## 0. How to use this document

This document is the source of truth for the `kiln` project. It is structured so
that an autonomous agent (Claude Code) can execute each milestone end-to-end with
minimal external clarification.

**Execution rules for the agent:**

1. Work milestones in numerical order. Do not skip ahead.
2. Each milestone has explicit **Acceptance Criteria** — a milestone is "done"
   only when every criterion passes in CI.
3. Write tests *as you go*, not at the end. Every public function gets at least
   one test. Every CLI command gets at least one integration test.
4. Open a PR at the end of each milestone. PR description must list which
   acceptance criteria are met and link to the test(s) that prove it.
5. If a task is genuinely blocked (e.g., upstream API doesn't exist), document
   the blocker in `docs/decisions/` as an ADR (Architecture Decision Record) and
   move to the next task in the same milestone. Do not invent workarounds
   silently.
6. Never use `unwrap()` or `expect()` in non-test code. Use `?` with `anyhow`
   (in binaries) or `thiserror` (in libraries).
7. Commit messages follow Conventional Commits (`feat:`, `fix:`, `refactor:`,
   `test:`, `docs:`, `chore:`).

**Overnight target:** M0 through M2 inclusive. M3+ may begin if time allows but
should not be considered required for the first overnight session.

---

## 1. Project conventions

### 1.1 Identity

- **Name:** `kiln` (binary), `kiln-*` (crate names in workspace)
- **License:** Dual MIT / Apache-2.0. Add both `LICENSE-MIT` and `LICENSE-APACHE`
  at the repo root. Every `Cargo.toml` declares `license = "MIT OR Apache-2.0"`.
- **MSRV (Minimum Supported Rust Version):** Rust 1.75 (stable). Pinned in
  `rust-toolchain.toml`.
- **Edition:** 2021.

### 1.2 Repository layout

```
kiln/
├── Cargo.toml                    # workspace root
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── README.md
├── LICENSE-MIT
├── LICENSE-APACHE
├── CONTRIBUTING.md
├── .github/workflows/ci.yml
├── crates/
│   ├── kiln-cli/                # main binary (`kiln`)
│   ├── kiln-core/               # manifest, project model, error types
│   ├── kiln-build/              # build pipeline + simulator drivers
│   ├── kiln-deps/               # dependency resolution (wraps bender)
│   ├── kiln-lint/               # linting via slang
│   ├── kiln-fmt/                # formatting (wraps verible)
│   ├── kiln-test/               # test runner
│   ├── kiln-wave/               # waveform integration (Surfer)
│   ├── kiln-doc/                # documentation generation
│   └── slang-rs/                # subprocess wrapper around the `slang` CLI
├── examples/
│   ├── hello-counter/            # minimal example used by integration tests
│   ├── with-deps/                # exercises dependency resolution
│   └── uvm-lite/                 # exercises testing
├── docs/
│   ├── decisions/                # ADRs
│   ├── manifest-spec.md          # the Kiln.toml schema
│   └── architecture.md
└── tests/
    └── integration/              # end-to-end CLI tests
```

### 1.3 Core dependencies (pinned in workspace `Cargo.toml`)

| Purpose                | Crate                       |
| ---------------------- | --------------------------- |
| CLI parsing            | `clap` (derive)             |
| TOML parsing           | `toml` + `serde`            |
| Error handling (bins)  | `anyhow`                    |
| Error handling (libs)  | `thiserror`                 |
| Logging                | `tracing` + `tracing-subscriber` |
| Pretty diagnostics     | `ariadne`                   |
| Process invocation     | `std::process` + `tokio` (where async helps) |
| Hashing                | `blake3`                    |
| Testing (snapshot)     | `insta`                     |
| Testing (CLI)          | `assert_cmd` + `predicates` |
| Dependency resolution  | `bender` (from crates.io)   |
| JSON parsing           | `serde_json`                |

### 1.4 Code conventions

- `rustfmt.toml`: default, `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`.
- `clippy.toml`: deny `clippy::pedantic` opt-ins listed in `clippy.toml`. Allowed exceptions documented inline.
- All public items have rustdoc with at least one usage example.
- Errors bubble; the binary is the only place where errors are rendered for humans.
- No `println!` outside the CLI crate. Use `tracing::{info, warn, error}`.

### 1.5 CI requirements (every milestone must keep these green)

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`
- Integration tests run on Ubuntu 22.04 with `slang`, `verilator`, and `verible`
  installed and on PATH. macOS runs on best-effort.

---

## 2. Milestones

### M0 — Foundation

**Goal:** A scaffolded Rust workspace with a working `kiln` binary that can
create new projects, parse a manifest, and report version info. No real build
logic yet — this milestone exists to make every subsequent milestone faster.

**Tasks:**

1. Initialize Cargo workspace per layout in §1.2. Empty stub crates for the
   ones not used yet (`lib.rs` with module doc and one trivial test).
2. Set up `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, dual
   license files, README skeleton.
3. Configure `.github/workflows/ci.yml`:
    - matrix: Ubuntu 22.04, macOS latest
    - jobs: `fmt`, `clippy`, `test`
    - cache `~/.cargo` and `target/`
4. Implement `kiln-core::manifest`:
    - `Manifest` struct deserialized from `Kiln.toml` via serde.
    - Schema (see `docs/manifest-spec.md`, also draft in this milestone):
      ```toml
      [package]
      name = "my_design"
      version = "0.1.0"
      authors = ["Jane <jane@example.com>"]
      description = "A widget"
      license = "MIT OR Apache-2.0"

      [design]
      top = "my_design_top"           # required for build
      sources = ["src/**/*.sv"]       # glob list, default ["src/**/*.{sv,svh,v}"]
      include_dirs = ["src/include"]
      defines = { FOO = "1", BAR = "" }

      [dependencies]
      # populated in M4
      ```
    - Validation: name must be a valid SV identifier, version must be semver,
      every `include_dirs` entry must exist when not in `kiln new`.
    - Snapshot tests covering valid manifests, invalid manifests (bad semver,
      bad identifier, unknown keys with `deny_unknown_fields`), default values.
5. Implement `kiln-cli` commands:
    - `kiln --version` / `kiln -V`
    - `kiln new <name>` — creates a new directory with template manifest,
      `src/<name>.sv` containing a stub module, `tests/` directory, `.gitignore`.
    - `kiln init` — same as `new` but in current directory.
    - `kiln check-manifest` (hidden, for testing) — parses `Kiln.toml` and
      prints the parsed value; exits non-zero on failure.
6. Add `tests/integration/` with `assert_cmd`-based tests for `kiln new` and
   `kiln init`. Verify the generated project structure exactly with snapshot tests.
7. Write `CONTRIBUTING.md` covering local dev setup, test commands, ADR process.
8. Write `docs/architecture.md` with a one-page overview pointing at this
   milestones doc as the source of truth.

**Acceptance criteria:**

- [ ] CI passes on Ubuntu and macOS.
- [ ] `cargo install --path crates/kiln-cli` succeeds; `kiln --version` prints.
- [ ] `kiln new demo && cd demo && kiln check-manifest` exits 0 and prints
      the parsed manifest.
- [ ] Snapshot tests cover at least 6 manifest cases (3 valid, 3 invalid).
- [ ] Repo has both license files, README with install instructions, and a
      working CI badge.

**Out of scope:**

- Any actual build, lint, or simulation logic.
- Dependency resolution.
- Interaction with slang or Verilator.

---

### M1 — Slang CLI wrapper

**Goal:** A pure-Rust crate (`slang-rs`) that drives the `slang` binary as a
subprocess and exposes a typed Rust API for diagnostics and the AST. No FFI,
no C++, no cmake. The user installs slang as a runtime dependency the same way
they install Verilator (e.g., `brew install slang`).

**Background:** Slang's CLI is a stable, public-API surface that exposes
essentially everything we need: `--ast-json` produces a full elaborated AST,
diagnostics are emitted in a parseable format, and the binary is fast enough
that ~50ms of startup per invocation is invisible for CLI use. We previously
considered FFI; that decision was reversed for install-UX reasons (see
`docs/decisions/0001-slang-integration-strategy.md`).

**Tasks:**

1. ADR `docs/decisions/0001-slang-integration-strategy.md` documenting the
   subprocess-vs-FFI decision and the tradeoffs accepted.
2. ADR `docs/decisions/0002-slang-version-policy.md` pinning the minimum
   supported slang version. Use the latest stable release; document the version
   discovery mechanism (`slang --version`).
3. In `crates/slang-rs/`:
    - `Slang` struct: locates the `slang` binary on PATH, caches its version,
      validates against the minimum version. Configurable path override via
      `KILN_SLANG_PATH` env var or `Slang::with_path()`.
    - `Slang::check_version()` returns a clear error if slang is missing or
      too old, including the install instructions string.
    - `CompileRequest` builder: source files, include dirs, defines, top
      module, optional `--std` selection.
    - `Slang::compile(req)` invokes the slang binary with appropriate flags
      (`--ast-json -`, plus diagnostic flags), captures stdout + stderr,
      returns a `CompileResult`.
    - `CompileResult { ast: Ast, diagnostics: Vec<Diagnostic> }`.
    - `Diagnostic` struct: severity (`Error | Warning | Note`), code, message,
      file, line, column, length. Deserialized from slang's diagnostic output.
    - `Ast`: typed wrapper over the JSON tree. Initial coverage:
      `CompilationUnit`, `Module`, `Interface`, `Package`, `Port`, `Parameter`.
      Extensible via `serde_json::Value` escape hatch on every node so unknown
      fields don't break us. Round-trip test asserts unknown-field tolerance.
    - All process invocation goes through a single `run_slang()` helper that
      handles timeouts, captures stderr, and surfaces non-zero exit codes as
      structured errors.
4. Diagnostic format: prefer slang's `--diag-json` flag if available; otherwise
   parse the human-readable output with a small nom or regex parser. Verify
   which is current by running `slang --help` against the pinned version and
   document in the ADR.
5. Test fixtures under `crates/slang-rs/tests/fixtures/`:
    - `valid_module.sv` — parses cleanly.
    - `syntax_error.sv` — missing semicolon.
    - `width_mismatch.sv` — semantic warning.
    - `package_pkg.sv` + `consumer.sv` — multi-file scenario.
    - `with_includes/` — tests include-dir handling.
    - `with_defines.sv` — tests `+define+` handling.
6. Tests:
    - Each fixture has an integration test asserting expected diagnostic count,
      severity distribution, and source location of the first error.
    - All tests gated behind `#[cfg_attr(not(feature = "e2e"), ignore)]` since
      they require slang on PATH. CI installs slang and runs with `--features e2e`.
    - Unit tests for the JSON deserializers run without slang installed (they
      use captured-output fixtures stored under `tests/fixtures/captured/`).
7. `crates/slang-rs/README.md` documents the runtime dependency, install
   instructions per platform, and the supported slang version range.

**Acceptance criteria:**

- [ ] `cargo install --path crates/kiln-cli` succeeds on a box with only the
      Rust toolchain installed (no cmake, no Python, no C++ compiler).
- [ ] `cargo test -p slang-rs` (without `--features e2e`) passes with no slang
      binary present, exercising the JSON deserializers against captured fixtures.
- [ ] `cargo test -p slang-rs --features e2e` passes on CI with slang installed.
- [ ] `slang-rs` reports a clear, actionable error if slang is missing
      (snapshot-tested message includes platform-specific install hint).
- [ ] Diagnostics for `syntax_error.sv` correctly identify the line of the
      missing semicolon.
- [ ] `slang-rs` rustdoc renders without warnings.

**Out of scope (this milestone):**

- Programmatic AST traversal (covered in M3 once we know which node types lint
  actually needs).
- Incremental compilation. Slang's CLI compiles from scratch each call; this is
  fine at our scale.
- In-process parsing. If we ever need it for performance, it goes behind a
  `slang-rs` feature flag and does not change the public API.

---

### M2 — Build pipeline (Verilator)

**Goal:** `kiln build` and `kiln run` work end-to-end on a single-package
project with no dependencies, driving Verilator under the hood.

**Tasks:**

1. In `crates/kiln-build/`:
    - `SourceSet`: resolved list of source files from manifest globs, with
      absolute paths and per-file metadata.
    - `BuildPlan`: struct describing the inputs, outputs, simulator backend,
      flags, top module.
    - `Cache`: content-hashed cache keyed on `(source_hash, flags_hash,
      top_module)`. Stored under `target/kiln/<hash>/`.
2. Verilator backend (`kiln-build::backend::verilator`):
    - Detect `verilator` on PATH; error with install instructions otherwise.
    - Invoke with `--binary --top-module <top> --sv -Wall <sources>`.
    - Capture stderr; parse it into structured `Diagnostic`s.
    - Verilator output parser:
        - Regex / nom-based parser for the `%Error:`, `%Warning:`, `%Note:`
          format and the `file:line:col:` prefix variant.
        - Round-trip tests against captured Verilator output samples in
          `tests/fixtures/verilator-output/`.
    - Place built binary at `target/kiln/<hash>/<top>.bin`.
3. `kiln build`:
    - Loads manifest, builds plan, runs Verilator, prints diagnostics in
      ariadne style with source spans, exits non-zero on error.
    - `--release` flag: passes `-O3` and `--x-assign 0` to Verilator.
    - `-v / --verbose`: enables tracing at debug level.
4. `kiln run`:
    - Implies `kiln build` first.
    - Executes the built binary; forwards args after `--`.
5. `kiln clean`:
    - Removes `target/kiln/`.
6. Integration test: `examples/hello-counter/` is a 4-bit up-counter testbench.
    - `kiln build` produces a binary.
    - `kiln run` executes it; the binary prints "PASS" on stdout, the test
      asserts on that.
7. Update `docs/architecture.md` with a build-pipeline diagram (mermaid).

**Acceptance criteria:**

- [ ] `cd examples/hello-counter && kiln run` prints "PASS".
- [ ] Editing a source file and re-running `kiln build` recompiles. Not editing
      it does not (cache hit).
- [ ] Introducing a syntax error produces a diagnostic with correct file/line/col
      that visually points at the offending token.
- [ ] `kiln build --release` is at least as fast at runtime as the default
      build (sanity check; not a benchmark).

**Out of scope:**

- Dependencies (M4).
- Slang-based fast lint (M3 — Verilator's lint is what we use here).
- Test discovery / `kiln test` (M5).
- Wave dumping (M7).

---

### M3 — Diagnostics layer (Slang-powered fast check)

**Goal:** `kiln check` runs Slang for sub-second feedback without invoking
Verilator. Diagnostics are rendered identically to M2's build diagnostics.

**Tasks:**

1. Extend `slang-rs` with the AST-walking primitives needed for lint:
    - Typed accessors on `Ast` for: top-level modules, packages, interfaces,
      port lists, parameter declarations, signal declarations.
    - `Span { file, line, col, length }` available on every node we expose.
    - A `Visitor` trait over the typed AST so `kiln-lint` rules don't touch
      the underlying `serde_json::Value` directly.
    - Performance note: AST JSON for large designs is non-trivial (multi-MB).
      Use `serde_json::from_reader` with a buffered reader; do not slurp the
      whole stdout into a `String` before parsing.
2. `kiln-lint::Linter` driving `slang-rs`.
3. `kiln check`:
    - Runs Slang elaboration over the manifest's source set.
    - Renders diagnostics with `ariadne` (color, span, hint).
    - Exit codes: 0 on clean, 1 on warnings (only with `--deny-warnings`),
      2 on errors.
4. Severity configuration in `Kiln.toml`:
    ```toml
    [lint]
    unused_signal = "warn"        # error | warn | allow
    width_mismatch = "error"
    ```
    Each Slang diagnostic ID maps to a knob. Unknown IDs default to Slang's
    own severity.
5. Tests:
    - `examples/hello-counter` produces zero diagnostics.
    - A new `examples/lint-demo/` with intentional smells produces the
      expected set of diagnostics.

**Acceptance criteria:**

- [ ] `kiln check` on the hello-counter example completes in < 200ms on the
      CI runner (fail-soft if runner is slow; just record).
- [ ] Severity overrides in `Kiln.toml` round-trip correctly.
- [ ] Diagnostic rendering is visually identical between `kiln build` and
      `kiln check` (both use the shared rendering crate).

**Out of scope:**

- Auto-fix / `cargo fix` equivalent.
- LSP integration (M9 stretch).

---

### M4 — Dependencies (wrap Bender)

**Goal:** `kiln add`, `kiln update`, and a `Kiln.lock` lockfile work.
Dependencies are resolved by delegating to `bender` as a library.

**Background:** Bender is published on crates.io and exposes its resolver as a
library. We do not fork or vendor it; we depend on a pinned version. We may
need to upstream small API additions — that is acceptable, but should be done
via PRs to bender's repo, not local patches.

**Tasks:**

1. Add `bender` as a workspace dependency, pinned to a specific minor version.
2. `Kiln.toml` `[dependencies]` schema:
    ```toml
    [dependencies]
    axi = { git = "https://github.com/pulp-platform/axi.git", version = "0.39" }
    common_cells = { git = "https://github.com/pulp-platform/common_cells.git", rev = "v1.32.0" }
    local_ip = { path = "../local_ip" }
    ```
3. `kiln-deps` translates this to a `Bender.yml`-equivalent in-memory model
   and invokes Bender's resolver.
4. `Kiln.lock` written in TOML with exact resolved versions and content
   hashes. Format documented in `docs/lockfile-spec.md`.
5. Source files from dependencies are included in the build's `SourceSet`,
   with their own `include_dirs` and `defines`. Bender's per-group settings map
   onto our model.
6. Commands:
    - `kiln add <git-url> [--rev <r> | --version <v>]`
    - `kiln remove <name>`
    - `kiln update [<name>]` — updates lockfile.
    - `kiln tree` — prints dependency tree.
7. `examples/with-deps/` depends on a small published IP (pick a stable, small
   PULP IP like `common_cells`). Integration test: `kiln build` succeeds.
8. ADR `docs/decisions/0003-bender-integration.md`: documents the wrapping
   strategy, what we expose, and what Bender APIs we depend on.

**Acceptance criteria:**

- [ ] `kiln add` mutates `Kiln.toml` and writes `Kiln.lock`.
- [ ] `kiln build` works on `examples/with-deps/` with the dependency fetched
      from git.
- [ ] Removing the dependency directory and running `kiln build` re-fetches
      based on the lockfile.
- [ ] `kiln tree` output is stable (snapshot-tested).

**Out of scope:**

- A central registry. Git-based deps only for now.
- Vendoring / `cargo vendor` equivalent (later).
- Workspace-level dependency unification across multiple packages (later).

---

### M5 — Testing

**Goal:** `kiln test` discovers and runs testbenches with Cargo-like
ergonomics. Cocotb is the default Python-driven backend; native SV testbenches
are supported as a simpler path.

**Tasks:**

1. Test discovery rules:
    - `tests/*.sv` → native SV testbench, top module = filename stem.
    - `tests/*/test.py` + `tests/*/<dut>.sv` → cocotb test.
    - Manifest override:
      ```toml
      [[test]]
      name = "smoke"
      path = "tests/custom/path.sv"
      top = "smoke_tb"
      ```
2. Native SV testbench runner: builds with Verilator (reusing M2 pipeline),
   runs the binary, expects a `$finish` with no errors.
3. Cocotb runner: invokes cocotb-runner via subprocess with the right
   environment. Detects cocotb installation; clear error if missing.
4. `kiln test`:
    - `[NAME_FILTER]` positional: substring match against test names.
    - `--jobs <N>`: parallel execution (default = num CPUs).
    - `--no-fail-fast`: keep going after first failure.
    - `--list`: print discovered tests, do not run.
    - Output format: per-test PASS/FAIL with elapsed time, summary line at end.
5. Test result caching: tests skip rerun if neither sources nor testbench
   changed since last green run. Override with `--force`.
6. `examples/uvm-lite/` — a small testbench using cocotb for the testing path.
7. Integration tests for the runner: each example has a deterministic test set.

**Acceptance criteria:**

- [ ] `kiln test` on the hello-counter example runs at least one test and
      reports PASS.
- [ ] `kiln test --list` is stable (snapshot-tested).
- [ ] `kiln test does_not_exist` reports zero tests run, exits 0 (Cargo
      semantics).
- [ ] Parallel execution observably runs faster than `--jobs 1` on a multi-test
      example.

**Out of scope:**

- UVM (full) — later. cocotb covers most current open-source needs.
- SystemC interop.
- Coverage collection (M9).

---

### M6 — Format and lint UX

**Goal:** `kiln fmt` works via Verible. Lint configuration is unified across
Slang and Verible.

**Tasks:**

1. Wrap `verible-verilog-format`:
    - Detect on PATH.
    - Default flags pulled from `[fmt]` section of `Kiln.toml`.
    - `kiln fmt` formats in place; `kiln fmt --check` exits non-zero on diff,
      prints a unified diff.
2. Optional Verible lint backend in addition to Slang:
    ```toml
    [lint]
    backends = ["slang", "verible"]   # default ["slang"]
    ```
3. Pre-commit-friendly: `kiln fmt --check` and `kiln check` both have stable
   exit codes and machine-readable `--format json` output.

**Acceptance criteria:**

- [ ] `kiln fmt --check` integrated into CI for the `examples/`.
- [ ] JSON output schemas documented in `docs/json-output.md` and
      snapshot-tested.

**Out of scope:**

- Custom in-house formatter.
- Slang-based formatter (slang doesn't have a stable formatter; track upstream).

---

### M7 — Waveforms

**Goal:** `kiln wave` opens Surfer at the right file with sensible defaults.
Tests dump waves automatically.

**Tasks:**

1. `kiln build --trace` and `kiln test --trace` instruct Verilator to dump
   FST. Output path: `target/kiln/waves/<test_name>.fst`.
2. `kiln wave [<test>]`:
    - With a test name, opens the FST for that test.
    - Without, opens the most recent FST.
    - Detects Surfer on PATH; if missing, prints install instructions and
      offers `--print-path` for piping into other viewers.
3. `Kiln.toml` `[wave]` section:
    ```toml
    [wave]
    format = "fst"          # vcd | fst
    enabled_by_default = false
    ```
4. Surfer state file convention: per-test `.surfer.toml` saved next to the FST,
   so reopening picks up the same signal layout.

**Acceptance criteria:**

- [ ] `kiln test --trace && kiln wave` opens a Surfer window with the
      expected hierarchy (manual verification documented in `docs/manual-tests.md`;
      automated check verifies the FST file exists and is non-zero).

**Out of scope:**

- Embedded waveform viewer in `kiln` itself.
- VCD post-processing.

---

### M8 — Documentation generation

**Goal:** `kiln doc` produces a static HTML site documenting modules,
interfaces, packages, and parameters. Style modeled on rustdoc.

**Tasks:**

1. Doc-comment convention:
    - `///` above an item attaches to it (module, interface, package, param,
      port).
    - `//!` at the top of a file or inside a package attaches to the
      enclosing scope.
    - Markdown-rendered.
2. Extractor strategy: combine two passes since slang's `--ast-json` does not
   preserve comment trivia.
    - **Source pass:** scan source files line-by-line, collecting contiguous
      `///` and `//!` comment blocks and the token immediately following them.
      Lightweight; uses `slang-rs` only for the file list.
    - **AST pass:** use `slang-rs` to get the typed list of items (modules,
      interfaces, packages, parameters, ports) with their spans.
    - **Join:** for each item, find the comment block whose end line is
      immediately above the item's start line. Unattached comment blocks are
      dropped with a debug-level log.
    - Markdown-rendered via `pulldown-cmark`.
3. Static-site generator:
    - Index page lists packages, modules, interfaces.
    - Per-item page shows ports, parameters, attached doc, source link.
    - Cross-references: a port type that names a struct in a package becomes a
      hyperlink.
    - Output: `target/doc/`.
4. `kiln doc`:
    - Generates the site.
    - `--open` opens it in the browser.
    - `--no-deps` skips dependency docs (default true; flip with `--deps`).

**Acceptance criteria:**

- [ ] Generates a site for `examples/hello-counter` with a navigable index and
      at least one cross-reference link that resolves.
- [ ] HTML output validates as well-formed HTML5 (use `html5ever` or external
      tidy in CI).

**Out of scope:**

- Hosted central doc site (rustdoc.io equivalent).
- Search across multiple projects.
- Custom themes.

---

### M9 — Polish & stretch goals

**Goal:** Round out the experience. Ship 0.1.0.

**Tasks (ranked by ROI; pick top items based on M0–M8 learnings):**

1. **LSP integration:** `kiln lsp` shells out to `slang-server` with a
   generated `.slang` config derived from `Kiln.toml`. This makes editor
   integration "just work" in a kiln project.
2. **Coverage:** wrap Verilator's `--coverage` and produce an HTML report.
3. **Release engineering:**
    - GitHub Releases with prebuilt binaries (Linux x86_64 + aarch64, macOS
      arm64) via cargo-dist or matrix builds.
    - Homebrew tap.
    - `cargo install kiln` from crates.io.
4. **`kiln bench`:** wraps Verilator in `--threads N --prof-exec` mode,
   reports throughput.
5. **`kiln publish`:** explicitly punted — open an issue tagged
   `decision-needed` to decide registry vs. git-only.
6. **Workspace support:** multiple packages in one repo, like cargo workspaces.

**Acceptance criteria for 0.1.0 release:**

- [ ] All M0–M8 acceptance criteria still pass.
- [ ] `cargo install kiln` works from crates.io.
- [ ] Prebuilt binaries published for at least Linux x86_64.
- [ ] `kiln --help` is well-organized and complete.
- [ ] README shows the full happy path (`kiln new` → `kiln add` → `kiln test`
      → `kiln wave`) on a real example.

---

## 3. Open decisions / ADR queue

The agent should open these as ADR drafts in `docs/decisions/` with their
current status as "proposed":

1. **Manifest format:** `Kiln.toml` vs. compatibility shim that reads
   `Bender.yml` directly. Currently: own format + an importer (`kiln import-bender`).
2. **Slang version policy:** track upstream releases or pin? Currently: pin and
   bump explicitly.
3. **Registry:** the elephant. See M9 task 5.

---

## 4. Reference materials

- Slang: <https://github.com/MikePopoloski/slang>, docs at <https://sv-lang.com>.
- slang-server: <https://github.com/hudson-trading/slang-server>.
- Bender: <https://github.com/pulp-platform/bender>.
- Verilator: <https://www.veripool.org/verilator/>.
- Verible: <https://github.com/chipsalliance/verible>.
- Surfer: <https://surfer-project.org/>.
- cocotb: <https://www.cocotb.org/>.
- Cargo source (for reference patterns, *not* code copying):
  <https://github.com/rust-lang/cargo>.

---

## 5. Definition of "done" for the overnight session

When the agent stops:

1. All commits pushed to a feature branch per milestone (`milestone/m0-foundation`, etc.).
2. PRs opened against `main`, with descriptions linking to the relevant
   milestone section above and listing which acceptance criteria are met.
3. A short status report at `docs/status.md` summarizing:
    - Which milestones are complete.
    - Which acceptance criteria within each milestone passed/failed.
    - Any new ADRs filed.
    - Concrete blockers, if any, with a proposed next step for each.
