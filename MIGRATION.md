# Migration guide

## 0.1.0 → 0.1.1

This release replaces the earlier ad-hoc manifest fields with a structured
three-tier model: tool-agnostic `[design]` fields, typed `[tool.<name>]`
sections, and `[lint.slang]` / `[lint.verilator]` subtables for tool-specific
overrides. Profile support and the `kiln lint` / `kiln schema` subcommands are
also new.

### `[design]` changes

**`slang_args` and `verilator_args` are removed.** Move them to the appropriate
`[tool.*].extra_args` field:

```toml
# Before
[design]
top        = "my_top"
slang_args = ["--allow-hierarchical-const"]
verilator_args = ["--x-assign", "0"]

# After
[design]
top = "my_top"

[tool.slang]
extra_args = ["--allow-hierarchical-const"]

[tool.verilator]
extra_args = ["--x-assign", "0"]
```

**`timescale` and `language` are now first-class fields** (they were previously
passed via `slang_args`):

```toml
# Before
[design]
slang_args = ["--timescale", "1ns/1ps", "--std", "sv2017"]

# After
[design]
timescale = "1ns/1ps"
language  = "sv2017"
```

### `[lint]` changes

**`"allow"` severity is renamed to `"off"`:**

```toml
# Before
[lint]
implicit-net = "allow"

# After
[lint]
implicit-net = "off"
```

**Slang-specific options now go under `[lint.slang]`**, and verilator-specific
warning codes go under `[lint.verilator]`. Cross-tool canonical names stay at
the top level:

```toml
# Before — slang option names at top level
[lint]
width-trunc            = "error"
relax-enum-conversions = "off"

# After
[lint]
width-trunc = "error"       # canonical, maps to both tools

[lint.slang]
relax-enum-conversions = "off"   # slang-only
```

### New: `[tool.*]` typed fields

`[tool.verilator]` now has typed fields for common options:

```toml
[tool.verilator]
threads    = 4
trace      = "fst"    # false | "vcd" | "fst"
coverage   = true
extra_args = ["--x-assign", "0"]
```

Previously these had to be passed through `verilator_args` as raw strings.

### New: `[profile.*]`

Per-context overrides for lint and tool settings:

```toml
[profile.release.tool.verilator]
extra_args = ["-O3"]

[profile.test.tool.verilator]
trace    = "fst"
coverage = true
```

Select with `--profile <name>` or `--release`. `kiln test` defaults to the
`test` profile; everything else defaults to `dev`.

### New subcommands

- `kiln lint list` — list all known canonical lint rules.
- `kiln lint explain <name>` — describe a specific rule.
- `kiln schema` — print a JSON Schema for `Kiln.toml` (useful with `taplo` or
  VS Code's Even Better TOML for completion and inline validation).
