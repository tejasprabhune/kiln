# `Kiln.toml` manifest specification

This document is the source of truth for the `Kiln.toml` schema. The Rust
implementation lives in `crates/kiln-core/src/manifest.rs` and is the
authoritative parser — if this document and the parser disagree, the parser
wins and this document is wrong.

## Top-level structure

```toml
[package]
# required
name = "my_design"
version = "0.1.0"

# optional
authors = ["Jane <jane@example.com>"]
description = "A widget"
license = "MIT OR Apache-2.0"

[design]
# required
top = "my_design_top"

# optional, with defaults
sources      = ["src/**/*.sv", "src/**/*.svh", "src/**/*.v"]
include_dirs = []
defines      = {}

[dependencies]
# Populated in M4. Empty in M0.
```

Unknown keys at every level are rejected (`deny_unknown_fields`).

## `[package]`

| Key           | Type            | Required | Notes |
| ------------- | --------------- | -------- | ----- |
| `name`        | string          | yes      | Must be a valid SystemVerilog simple identifier: starts with a letter or `_`, followed by letters / digits / `_`. |
| `version`     | string (semver) | yes      | Must parse as semver per <https://semver.org/>. |
| `authors`     | list of strings | no       | Free-form. |
| `description` | string          | no       | Single-line description. |
| `license`     | SPDX expression | no       | Recommended: `MIT OR Apache-2.0`. |

## `[design]`

| Key            | Type                 | Required | Default                                                  |
| -------------- | -------------------- | -------- | -------------------------------------------------------- |
| `top`          | string               | yes      | —                                                        |
| `sources`      | list of glob strings | no       | `["src/**/*.sv", "src/**/*.svh", "src/**/*.v"]`         |
| `include_dirs` | list of paths        | no       | `[]`                                                     |
| `defines`      | string-to-string map | no       | `{}`                                                     |

Globs are evaluated relative to the directory containing `Kiln.toml`.
Include directories must exist on disk when `Kiln.toml` is loaded for an
existing project; the check is skipped during `kiln new` and `kiln init`
because the project has not yet been created.

## `[dependencies]`

Empty in M0. Populated in M4 with git, path, and version-based dependencies.
See `kiln-milestones.md` §M4 for the planned schema.

## `[lint]`

Per-diagnostic severity overrides. Keys are slang's `optionName` strings
(e.g. `width-trunc`, `unused-net`). Values are one of:

- `"error"` — promote to `Error`. `kiln check` fails (exit 2).
- `"warn"`  — emit as `Warning`.
- `"allow"` — drop the diagnostic entirely.

```toml
[lint]
width-trunc  = "error"
unused-net   = "warn"
implicit-net = "allow"
```

Setting any rule to `"error"` or `"warn"` also passes the corresponding
`-W<id>` to slang, so warnings slang would otherwise silence at the
default level surface and can be acted on. `"allow"` is post-filtering;
the diagnostic still travels from slang to `kiln-lint` but is dropped
before rendering.

## Validation rules (M0)

The parser rejects manifests that violate any of:

- Invalid SystemVerilog identifier in `package.name`.
- Non-semver string in `package.version`.
- Unknown top-level or nested key.
- Missing `[design]` section.
- (When loading from disk) any entry in `design.include_dirs` that does not
  exist relative to the manifest's parent directory.

## Examples

The `crates/kiln-core/src/manifest.rs` snapshot tests cover the canonical
valid and invalid cases. See `crates/kiln-core/src/snapshots/` for the
recorded outputs.
