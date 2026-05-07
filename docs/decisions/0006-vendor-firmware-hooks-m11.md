# 0006. Vendor libraries, firmware artifacts, and project hooks (M11)

- Status: accepted
- Date: 2026-05-06

## Context

After M10 the manifest no longer needs `extra_args` for common verilator
flags. But the same EE 151 RISC-V FPGA project that motivated M10 still
abuses the schema in three other places:

1. **Vendor sim-models and stubs share `[design].sources`.** The user's
   manifest globs `hardware/sim_models/BUFG.sv`, `glbl.sv`,
   `div_gen_sim.sv` plus `hardware/stubs/PLLE2_ADV.sv` plus
   Vivado-generated `*_stub.v` into the design alongside the project's
   own RTL. There is no way to mark these as "vendor primitives, blackbox
   for synthesis" or to group them by vendor. They leak into every code
   path that consumes `design.sources`, including future synthesis
   backends that should not see them.
2. **Embedded firmware is built via shell escapes.** Every `[[test.matrix]]`
   that loads a `.hex` file declares
   `prebuild = "make -C software/c_tests/{stem}"`. The build is coupled
   to the test, executes serially in the test runner's prebuild dedup
   pass, and has no first-class concept of artifacts.
3. **Project-level setup has nowhere to live.** Vivado IP generation,
   filelist sync, license-server warmup — all of these belong before
   `kiln check` / `kiln build` / `kiln test`, and there is currently no
   declarative way to express that.

`extra_args` and per-test `prebuild` are escape hatches; they are not
the right home for these patterns.

## Decision

Three new top-level manifest tables, all backwards compatible.

### `[vendor.<name>]`

Group vendor libraries (Xilinx unisims, Altera megafunctions, custom
stub sets) under a named block. Each block owns three lists:

```toml
[vendor.xilinx]
sim_models       = ["hardware/sim_models/BUFG.sv", "hardware/sim_models/glbl.sv"]
stubs            = ["hardware/stubs/PLLE2_ADV.sv"]
blackbox_modules = ["MMCME2_ADV"]
```

| Field | Effect |
| ----- | ------ |
| `sim_models` | Glob patterns appended to the resolved source set. Visible to slang, verilator, and doc generation. |
| `stubs` | Glob patterns appended to the resolved source set; intended for synth-only stubs not present at sim time. |
| `blackbox_modules` | Module names. Each becomes `--bbox <name>` to verilator so the body is not compiled. |

Vendor names are free-form identifiers; `[vendor.xilinx]`,
`[vendor.altera]`, `[vendor.custom_a]` are all valid. The split between
`sim_models` and `stubs` is documentary (and forward-looking — synthesis
backends will treat them differently); for now both feed the same source
set.

### `[[firmware]]`

Array of firmware artifacts produced by an external build system and
consumed by RTL tests:

```toml
[[firmware]]
name = "isa-tests"
path = "software/riscv-isa-tests"
build = "make"
artifacts = "*.hex"
```

| Field | Type | Notes |
| ----- | ---- | ----- |
| `name` | string | Free-form identifier; surfaced in `kiln env` and used by future `kiln firmware build <name>`. Must be a valid SystemVerilog identifier. |
| `path` | string | Directory containing the firmware build, relative to the project root. |
| `build` | string | Shell command run inside `path` to build the firmware. |
| `artifacts` | string | Glob (relative to `path`) describing the produced files, used purely for documentation today; future tooling will expose `kiln firmware list <name>` artifacts. |

`kiln test` runs every declared firmware build (deduped by command) once
before the test pass, mirroring the existing per-test `prebuild`
deduplication. Tests can still declare per-test `prebuild` for one-off
hex regeneration; declared firmware blocks remove the boilerplate when
several tests share the same upstream build.

### `[hooks]`

Project-level shell escapes keyed by lifecycle phase:

```toml
[hooks]
pre-build = "make -C ip/"
pre-check = ""
pre-test  = "git submodule update --init"
post-test = "echo run complete"
```

Phases:

| Phase | Fires before/after |
| ----- | ------------------ |
| `pre-check` | Before slang elaboration in `kiln check`. |
| `pre-build` | Before verilator in `kiln build` (and the build phase of `kiln run` / `kiln test`). |
| `pre-test`  | Before any testbench is run by `kiln test`. |
| `post-test` | After `kiln test` finishes (whether pass or fail). |

Each value is a single shell line, executed at the project root with
the system shell. Empty strings are treated as unset. A non-zero exit
from any pre-* hook aborts the command; `post-test` failures are logged
but do not change the test outcome.

### Out of scope for M11

Documented for follow-up milestones:

- `kiln firmware build <name>` / `kiln firmware list` subcommands. M11
  ships only the auto-prebuild integration on `kiln test`.
- Per-vendor synth/sim source split (today both feed the same source
  set). Will matter once a synthesis backend lands.
- Hook composition (multiple commands, async hooks, hook order). One
  shell line per phase is enough for the current use cases; richer
  composition can wait for evidence.

## Consequences

**Easier:**
- The EE 151 manifest's vendor/sim-model layout becomes self-documenting
  instead of mixed into `design.sources`.
- Repeated `prebuild` shell strings shrink to a single firmware block.
- Project-wide setup steps stop polluting individual test cases.

**Harder:**
- Three more top-level tables to validate, document, and keep in sync
  with the website reference.
- `SourceSet::resolve` now consults vendor sources too, which means a
  small fan-out of the resolution logic into kiln-build.
- Hook execution is opt-in best-effort; failures need to surface
  clearly without leaking shell internals into the user-facing error.

**Tradeoffs:**
- We do not introduce a separate sim/synth source split inside vendor
  blocks (`sim_only` / `synth_only` flags, etc.). One forward-compatible
  decision: today both `sim_models` and `stubs` feed the same source
  set, but the field names already telegraph the intent so a future
  milestone can split them without renaming.
- `pre-test` runs once per `kiln test` invocation, not once per test.
  Per-test setup goes through `[[test.cases]] prebuild` exactly as
  before. This is the same dedupe rule the test runner already uses.
- Hooks are shell strings, not structured commands. Cargo-shaped
  toolchains usually avoid shelling out, but every hardware project
  with non-trivial vendor setup already has a Makefile to invoke; the
  schema's job is to expose that, not to reinvent it.
