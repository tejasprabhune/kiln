# 0005. Schema extensibility for real-world projects (M10)

- Status: accepted
- Date: 2026-05-06

## Context

After M0–M9 the manifest schema covers the cargo-style happy path (one top,
one source set, generic `extra_args` escape hatches). Real projects in the
wild push back on this: a user-supplied `Kiln.toml` from a course-grade RISC-V
FPGA project (EE 151) routes five common Verilator flags, an auxiliary top
module, and per-build conditional defines through `[tool.*].extra_args`. Every
`extra_args` entry there is a feature request in disguise.

Concretely, the same manifest contains:

- `[tool.slang] extra_args = ["--top", "glbl"]` — a second top module so slang
  elaborates Xilinx unisim helpers.
- `[tool.verilator] extra_args = ["--timing", "--trace-structs",
  "--trace-params", "--x-assign", "0", "--bbox-unsup"]` — five very common
  Verilator knobs.
- A web of `+define+SIM`-style switches that are managed by hand because
  there is no `[features]` table.

`extra_args` is a permanent escape hatch (kept that way per
`docs/manifest-spec.md`), but it is currently load-bearing for *normal*
hardware workflows. That is the gap M10 closes.

## Decision

Add three independent schema extensions, all backwards compatible:

### 1. First-class Verilator knobs in `[tool.verilator]`

Promote five frequently-used Verilator flags to typed fields:

| Field | Type | Verilator flag |
| ----- | ---- | -------------- |
| `timing` | bool | `--timing` |
| `x_assign` | enum `"0" \| "1" \| "fast" \| "unique"` | `--x-assign N` |
| `bbox_unsup` | bool | `--bbox-unsup` |
| `trace_structs` | bool | `--trace-structs` (gated on `trace`) |
| `trace_params` | bool | `--trace-params` (gated on `trace`) |
| `trace_depth` | optional u32 | `--trace-depth N` (gated on `trace`) |

`extra_args` remains the escape hatch for everything else.

Conflict resolution with the existing release-profile default
(`--x-assign 0`): if the user sets `x_assign` explicitly, that value wins;
otherwise the `release` profile keeps its current `--x-assign 0` default for
back-compat.

Profile overrides for these fields use the same merge semantics as
`threads`/`coverage`: scalar fields prefer the overlay value when set.

### 2. `aux_tops` in `[design]`

Add `aux_tops: Vec<String>` to `[design]`. Passed as additional `--top` flags
to slang (which accepts multiple). Verilator only supports one
`--top-module`, so `aux_tops` is informational for Verilator and consumed
only by slang-based commands (`kiln check`, `kiln lsp`, `kiln doc`).

This directly removes the `["--top", "glbl"]` `extra_args` pattern.

### 3. `[features]` for conditional compilation

Cargo-shaped feature toggles that compose `+define+` flags and additional
source globs:

```toml
[features]
default = ["sim"]

[features.sim]
defines = ["SIM"]
sources = []

[features.debug]
defines = ["DEBUG=1"]
sources = ["src/debug/**/*.sv"]
```

CLI surface mirrors cargo:

- `--features <list>` — comma- or space-separated feature names. Replaces
  default-feature selection.
- `--all-features` — enable every defined feature.
- `--no-default-features` — disable the `default` set.

Active features contribute their defines (merged into `design.defines`) and
their sources (appended to `design.sources`). Feature names must be valid
SystemVerilog identifiers.

### Out of scope for this milestone

The audit identified these as future work, *not* part of M10:

- Vendor / sim-model libraries (`[vendor.<name>]`).
- `[[firmware]]` for embedded software prebuild.
- Project-level `[hooks]`.
- Per-test `working_dir` (already present in TestCase but not promoted).
- Workspaces, watch mode, `kiln doctor`, JSON-everywhere.

Each of these will land in its own milestone with its own ADR.

## Consequences

**Easier:**
- The EE151 manifest can drop most of `[tool.verilator].extra_args` and the
  `[tool.slang].extra_args = ["--top", "glbl"]` entry.
- Feature-gated builds become first-class instead of being managed through
  `[design].defines` swaps.
- New users see a manifest that names what they want to do, not raw flags.

**Harder:**
- Three new schema entry points to validate, test, and document.
- Profile resolution gets four new fields to merge for `[tool.verilator]`.
- The release profile's hardcoded `--x-assign 0` now has a precedence rule
  to remember (user `x_assign` wins).

**Tradeoffs:**
- We do not unify `trace`, `trace_structs`, `trace_params`, `trace_depth`
  into a nested struct (e.g. `trace = { format = "fst", structs = true }`).
  Flat siblings keep the TOML simple and avoid breaking the existing
  `trace = "fst"` form, at the cost of a loose grouping.
- `[features]` does not yet support per-feature dependency activation
  (Cargo's `["dep:foo"]`). Hardware deps via Bender don't have optional
  features today; if/when they do, we extend the `Feature` struct.
