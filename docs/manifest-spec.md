# `Kiln.toml` manifest specification

This document is the source of truth for the `Kiln.toml` schema. The Rust
implementation lives in `crates/kiln-core/src/manifest.rs` and is the
authoritative parser — if this document and the parser disagree, the parser
wins and this document is wrong.

## Editor autocomplete via JSON Schema

The current schema is published at
<https://tejasprabhune.github.io/kiln/kiln-schema.json>. Add a header
comment to your `Kiln.toml` and any TOML LSP that supports the
[`#:schema`](https://taplo.tamasfe.dev/configuration/directives.html)
directive (taplo, helix, Even Better TOML, etc.) will load it:

```toml
#:schema https://tejasprabhune.github.io/kiln/kiln-schema.json

[package]
name = "demo"
version = "0.1.0"
```

To inspect the schema locally without the network, run `kiln schema`.

## Top-level structure

```toml
[package]
name    = "my_soc"
version = "0.1.0"

[design]
top      = "soc_top"
sources  = ["src/**/*.sv"]
timescale = "1ns/1ps"
language  = "sv2017"

[lint]
width-trunc      = "error"
case-incomplete  = "warn"

[lint.slang]
relax-enum-conversions = "off"

[lint.verilator]
GENUNNAMED = "warn"

[tool.slang]
extra_args = ["--allow-hierarchical-const"]

[tool.verilator]
threads    = 4
trace      = "fst"
coverage   = false
extra_args = ["--x-assign", "0"]

[profile.release.tool.verilator]
extra_args = ["-O3"]

[profile.test.tool.verilator]
trace    = "fst"
coverage = true

[profile.test.lint]
unused = "error"

[wave]
format             = "fst"
enabled_by_default = false
```

Unknown keys at every structural level are rejected (`deny_unknown_fields`).
The exception is `[lint]`, where keys are user-defined rule names.

## `[package]`

| Key           | Type            | Required | Notes |
| ------------- | --------------- | -------- | ----- |
| `name`        | string          | yes      | Valid SystemVerilog simple identifier: starts with a letter or `_`, followed by letters, digits, or `_`. |
| `version`     | string (semver) | yes      | Must parse as semver per <https://semver.org/>. |
| `authors`     | list of strings | no       | Free-form. |
| `description` | string          | no       | Single-line description. |
| `license`     | SPDX expression | no       | Recommended: `MIT OR Apache-2.0`. |

## `[design]`

| Key            | Type                 | Required | Default                                          |
| -------------- | -------------------- | -------- | ------------------------------------------------ |
| `top`          | string               | yes      | —                                                |
| `aux_tops`     | list of strings      | no       | `[]`                                             |
| `sources`      | list of glob strings | no       | `["src/**/*.sv", "src/**/*.svh", "src/**/*.v"]` |
| `timescale`    | string               | no       | none                                             |
| `language`     | enum (see below)     | no       | none (slang/verilator default)                   |
| `include_dirs` | list of paths        | no       | `[]`                                             |
| `defines`      | string-to-string map | no       | `{}`                                             |
| `libraries`    | list of strings      | no       | `[]`                                             |
| `test_sources` | list of glob strings | no       | `[]` (falls back to `tests/*.sv` discovery)      |

`aux_tops` is the list of additional top modules that should be elaborated
alongside `top` for slang-driven commands (`kiln check`, `kiln doc`,
`kiln lsp`). Use it for non-instantiated helper modules — the Xilinx
`glbl` module is the canonical example. Verilator only supports a single
`--top-module`, so `aux_tops` is ignored by `kiln build`.

`language` values: `"sv2005"`, `"sv2009"`, `"sv2012"`, `"sv2017"`, `"sv2023"`.
Maps to `--std` for slang and `--default-language` for verilator.

`timescale` maps to `--timescale` for both slang and verilator.

`defines` entries with an empty string become valueless (`+define+FOO`); non-empty
values become `+define+FOO=bar`.

`include_dirs` entries must exist on disk when loading for an existing project;
the check is skipped during `kiln new` and `kiln init`.

`test_sources` glob patterns override the default `tests/*.sv` testbench discovery.

## `[dependencies]`

Each entry is one of:

```toml
[dependencies]
# Git dep with semver constraint (matched against repo tags).
axi          = { git = "https://github.com/pulp-platform/axi.git", version = "0.39" }
# Git dep pinned to a specific tag or commit SHA.
common_cells = { git = "https://github.com/pulp-platform/common_cells.git", rev = "v1.32.0" }
# Path dep relative to the project root, or absolute.
local_ip     = { path = "../local_ip" }
```

Resolved versions are written to `Kiln.lock`, which should be committed.
Use `kiln update` to refresh it. See `docs/lockfile-spec.md` for the lockfile schema.

## `[lint]`

The lint table has three layers:

1. **Canonical rules** (top-level keys) — kiln-defined names that map to both
   slang and verilator equivalents. Use these for cross-tool portability.
2. **`[lint.slang]`** — slang-specific option names, using slang's `optionName`
   strings. These overlay on top of canonical rules for slang invocations.
3. **`[lint.verilator]`** — verilator-specific warning codes (e.g. `WIDTHTRUNC`).
   These overlay on top of canonical rules for verilator invocations.

Severity values: `"error"` | `"warn"` | `"off"` | `"deny"`.

- `"error"` — promote to error; `kiln check`/`kiln build` fails.
- `"warn"` — emit as warning.
- `"off"` — drop the diagnostic entirely (post-filtering).
- `"deny"` — same as `"off"` for now (reserved for future treatment).

```toml
[lint]
width-trunc     = "error"
case-incomplete = "warn"
unused          = "warn"

[lint.slang]
relax-enum-conversions = "off"

[lint.verilator]
GENUNNAMED   = "warn"
DECLFILENAME = "off"
```

Setting a rule to `"error"` or `"warn"` also passes `-W<name>` to slang, surfacing
diagnostics that slang would normally suppress at the default level. For verilator,
it translates to the appropriate `-Wwarn-NAME` / `-Werror-NAME` flags.

If you're unsure of the name for a specific warning, `kiln check` prints it in
brackets after the message — e.g. `warning: ... [width-trunc]` — so you can copy
it directly into `[lint]`.

Use `kiln lint list` to see all known canonical rules. Use `kiln lint explain <name>`
for a description of a specific rule.

### Canonical lint rules

| Name               | Slang option           | Verilator code | Default |
| ------------------ | ---------------------- | -------------- | ------- |
| `width-trunc`      | `width-trunc`          | `WIDTHTRUNC`   | warn    |
| `case-incomplete`  | `case-incomplete`      | `CASEINCOMPLETE` | warn  |
| `unused`           | `unused-net`           | `UNUSEDSIGNAL` | warn    |
| `implicit-net`     | `implicit-net`         | `IMPLICITSTATIC` | warn  |
| `port-coercion`    | `port-coercion`        | `PINCONNECTEMPTY` | warn |

Rules not in this table must be specified directly under `[lint.slang]` or
`[lint.verilator]` using tool-native names.

## `[tool.<name>]`

Tool-specific configuration. Each tool section has a `path` override and
`extra_args` escape hatch; some have additional typed fields.

`extra_args` is permanent — it is not a deprecation target. The typed fields
cover the common cases; `extra_args` handles the rest.

### `[tool.slang]`

| Key          | Type           | Default | Notes |
| ------------ | -------------- | ------- | ----- |
| `path`       | path           | (PATH)  | Override the `slang` binary location. |
| `extra_args` | list of strings | `[]`   | Appended verbatim after all kiln-managed flags. |

### `[tool.verilator]`

| Key             | Type                                  | Default | Notes |
| --------------- | ------------------------------------- | ------- | ----- |
| `path`          | path                                  | (PATH)  | Override the `verilator` binary location. |
| `threads`       | integer                               | none    | Passed as `--threads N`. |
| `trace`         | `false` \| `"vcd"` \| `"fst"`        | `false` | Enables waveform dumping. |
| `trace_structs` | bool                                  | `false` | Adds `--trace-structs` (only when `trace` is enabled). |
| `trace_params`  | bool                                  | `false` | Adds `--trace-params` (only when `trace` is enabled). |
| `trace_depth`   | integer                               | none    | Adds `--trace-depth N` (only when `trace` is enabled). |
| `coverage`      | bool                                  | `false` | Enables coverage instrumentation (`--coverage`). |
| `timing`        | bool                                  | `false` | Adds `--timing` for designs that use delays / event control. |
| `x_assign`      | `"0"` \| `"1"` \| `"fast"` \| `"unique"` | none | Adds `--x-assign <value>`. When unset, the `release` profile still emits its historical `--x-assign 0` default. |
| `bbox_unsup`    | bool                                  | `false` | Adds `--bbox-unsup` to black-box unsupported constructs (e.g. vendor primitives). |
| `extra_args`    | list of strings                       | `[]`    | Appended verbatim after all kiln-managed flags. |

### `[tool.verible]`

| Key          | Type           | Default | Notes |
| ------------ | -------------- | ------- | ----- |
| `path`       | path           | (PATH)  | Override the `verible-verilog-format` binary location. |
| `extra_args` | list of strings | `[]`   | Appended verbatim after all kiln-managed flags. |

## `[profile.<name>]`

Profiles let you override design, lint, and tool settings per build context.
Built-in names: `dev` (default), `release`, `test`. You can also define custom profiles.

`kiln build` and `kiln check` default to `dev`. `kiln test` defaults to `test`.
Pass `--profile <name>` or `--release` to select a profile.

Profile overrides use replace semantics for `Vec` fields (not append) — this
makes it possible to remove a flag in one profile that exists in another.
Map fields merge with the overlay winning on key conflicts.

```toml
[profile.release.tool.verilator]
extra_args = ["-O3"]

[profile.test.tool.verilator]
trace    = "fst"
coverage = true

[profile.test.lint]
unused = "error"
```

A profile can override `[design]`, `[lint]`, `[lint.slang]`, `[lint.verilator]`,
`[tool.slang]`, `[tool.verilator]`, and `[tool.verible]`.

## `[features]`

Cargo-style conditional compilation toggles. Each named feature contributes
additional `+define+` flags and source globs when active:

```toml
[features]
default = ["sim"]

[features.sim]
defines = ["SIM"]

[features.debug]
defines = ["DEBUG=1", "VERBOSITY=2"]
sources = ["src/debug/**/*.sv"]
```

| Key       | Type             | Default | Notes |
| --------- | ---------------- | ------- | ----- |
| `default` | list of strings  | `[]`    | Features active when no `--features` flag is passed. Every name must appear in `[features.<name>]`. |

Each `[features.<name>]` block accepts:

| Key       | Type             | Default | Notes |
| --------- | ---------------- | ------- | ----- |
| `defines` | list of strings  | `[]`    | Each entry is either `NAME` (becomes `+define+NAME`) or `NAME=VALUE` (`+define+NAME=VALUE`). |
| `sources` | list of glob strings | `[]` | Additional source globs included only when this feature is active. |

Feature names must be valid SystemVerilog identifiers.

CLI surface (mirrors cargo):

- `--features <list>` — comma- or space-separated names; adds to the
  default set.
- `--all-features` — activate every defined feature.
- `--no-default-features` — start from an empty selection; only
  `--features` entries become active.

Active features run in selection order; later features overwrite earlier
ones on conflicting `defines` keys.

## `[vendor.<name>]`

Vendor library blocks. Each named block contributes sources to the
project's resolved source set and (optionally) declares modules that
verilator should treat as black boxes.

```toml
[vendor.xilinx]
sim_models       = ["hardware/sim_models/BUFG.sv", "hardware/sim_models/glbl.sv"]
stubs            = ["hardware/stubs/PLLE2_ADV.sv"]
blackbox_modules = ["MMCME2_ADV", "PLLE2_ADV"]
```

| Key                | Type                 | Default | Notes |
| ------------------ | -------------------- | ------- | ----- |
| `sim_models`       | list of glob strings | `[]`    | Appended to the resolved source set. Visible to slang, verilator, and doc generation. |
| `stubs`            | list of glob strings | `[]`    | Appended to the resolved source set today; future synthesis backends will keep these out of simulation. |
| `blackbox_modules` | list of strings      | `[]`    | Each name becomes `--bbox <name>` to verilator so the body is not compiled. Aggregated and deduplicated across all vendor blocks. |

Vendor names are free-form; e.g. `[vendor.xilinx]`, `[vendor.altera]`,
`[vendor.custom]`. Group by vendor to keep the manifest readable.

## `[[firmware]]`

Embedded firmware artifacts produced by an external build system and
consumed by RTL tests. `kiln test` runs every declared firmware build
once (deduped by `(path, build)`) before any per-test `prebuild`.

```toml
[[firmware]]
name = "isa_tests"
path = "software/riscv-isa-tests"
build = "make"
artifacts = "*.hex"
```

| Key         | Type   | Required | Notes |
| ----------- | ------ | -------- | ----- |
| `name`      | string | yes      | Free-form identifier, must be a valid SystemVerilog identifier. |
| `path`      | path   | yes      | Directory the build runs in, relative to the project root. |
| `build`     | string | yes      | Shell command run inside `path`. |
| `artifacts` | string | no       | Glob (relative to `path`) describing produced files. Documentary today; future tooling will surface artefact lists. |

## `[hooks]`

Project-level shell escapes for kiln subcommand lifecycle phases.
Each value is a single shell line, executed at the project root with
the system shell. Empty strings are treated as unset.

```toml
[hooks]
pre-check  = ""
pre-build  = "make -C ip/"
pre-test   = "git submodule update --init"
post-test  = "echo done"
```

| Phase       | Fires |
| ----------- | ----- |
| `pre-check` | Before slang elaboration in `kiln check`. |
| `pre-build` | Before verilator in `kiln build` (and the build phase of `kiln run` / `kiln test`). |
| `pre-test`  | Before any testbench is started by `kiln test`. |
| `post-test` | After `kiln test` finishes (regardless of pass/fail). |

Pre-* hook failures abort the parent subcommand. `post-test` failures
are logged but never change the test outcome.

## `[wave]`

| Key                  | Type                    | Default   | Notes |
| -------------------- | ----------------------- | --------- | ----- |
| `format`             | `"fst"` \| `"vcd"`     | `"fst"`   | Preferred trace format. FST is smaller and faster than VCD. |
| `enabled_by_default` | bool                    | `false`   | When `true`, every `kiln test` behaves as if `--trace` was passed. |

## Validation rules

The parser rejects manifests that violate any of:

- Invalid SystemVerilog identifier in `package.name`.
- Non-semver string in `package.version`.
- Unknown key at any structural level (`deny_unknown_fields`).
- Missing `[design]` section.
- (When loading from disk) any entry in `design.include_dirs` that does not
  exist relative to the manifest's parent directory.

## Full example

```toml
[package]
name        = "my_soc"
version     = "0.1.0"
authors     = ["Jane <jane@example.com>"]
description = "A parameterized SoC"
license     = "MIT OR Apache-2.0"

[design]
top          = "soc_top"
sources      = ["rtl/**/*.sv"]
timescale    = "1ns/1ps"
language     = "sv2017"
include_dirs = ["rtl/include"]
defines      = { SIMULATION = "", WIDTH = "8" }

[dependencies]
common_cells = { git = "https://github.com/pulp-platform/common_cells.git", version = "1.32" }

[lint]
width-trunc     = "error"
case-incomplete = "warn"

[lint.slang]
relax-enum-conversions = "off"

[tool.slang]
extra_args = ["--allow-hierarchical-const"]

[tool.verilator]
threads    = 4
trace      = false
extra_args = ["--x-assign", "0"]

[profile.release.tool.verilator]
extra_args = ["-O3"]

[profile.test.tool.verilator]
trace    = "fst"
coverage = true

[profile.test.lint]
unused = "error"

[wave]
format             = "fst"
enabled_by_default = false
```
