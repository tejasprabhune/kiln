# 0003. Bender integration: subprocess wrapper, with a library-API path forward

- Status: accepted
- Date: 2026-05-02

## Context

`kiln` needs dependency resolution for SystemVerilog / Verilog packages.
The only mature open-source resolver is
[Bender](https://github.com/pulp-platform/bender) (PULP). The milestones
doc § M4 specifies that we wrap it rather than reimplement it.

Bender 0.31.0 is published on crates.io and *does* expose a Rust library
surface (`bender::config`, `bender::resolver`, `bender::sess`,
`bender::src`). A first pass at integrating against the library showed:

1. The `Session` API is large and tightly coupled to `bender`'s CLI
   logging and error rendering. Stripping that out for embedding is
   non-trivial.
2. The lockfile schema (`Bender.lock`) is the primary artifact we
   actually care about. It is well-defined, stable, and easy to read
   back without going through bender at all once written.
3. Bender's CLI subcommands (`update`, `sources`, `packages`, `path`)
   already give us everything M4 needs: lockfile generation, resolved
   source list, dependency graph, and per-package paths.

Following the same pattern as ADR 0001 (slang as subprocess) keeps the
two external-tool wrappers symmetric: one place per tool that calls
`Command::spawn`, captured stdout/stderr, structured errors.

## Decision

`kiln-deps` shells out to the `bender` binary as a subprocess wrapper.
The `bender` crate is included as a workspace-level dependency so we
can move to the library API later without changing the public surface
of `kiln-deps`.

Specifically:

- `kiln-deps::resolve(project_root) -> ResolvedSourceSet` writes a
  generated `Bender.yml` into `target/kiln/bender/` translating the
  `[dependencies]` table from `Kiln.toml`, runs `bender update` and
  `bender sources --flatten` against that working directory, and parses
  the JSON output back into a list of absolute source paths.
- `kiln-deps::tree(project_root) -> DependencyTree` runs `bender
  packages` and parses its output.
- `Kiln.lock` is a renamed copy of `target/kiln/bender/Bender.lock`,
  written into the project root after a successful resolve. We treat
  it as opaque (lockfile-spec.md documents the format pointer-style;
  the schema is bender's).
- All `Command::spawn` calls go through one helper
  (`kiln_deps::runner::run_bender`), mirroring `slang_rs::run_slang`.

Each call site that uses subprocess is marked with a `// TODO(M4 lib
migration)` comment referencing this ADR, so a future PR that drops to
the library API has a checklist.

## Consequences

Easier:
- Ships now. No detangling of bender's CLI-bound types.
- Bender minor-version bumps are absorbed by retesting the CLI surface,
  not by re-validating internal trait impls.
- `kiln-deps` carries one runtime dep (bender on PATH) the same way
  `slang-rs` carries one (slang on PATH). Predictable install story.

Harder:
- Per-call process startup. For dependency resolution, bender's startup
  time is dominated by file I/O (cloning git remotes, hashing) anyway,
  so the absolute overhead is invisible.
- Error messages come from bender's CLI, not structured error types.
  `kiln-deps` re-wraps them in `BenderError::Cli { stderr_tail, code }`
  which preserves the original output verbatim.

## Reversal

Reversing this decision (going library-API) requires:
- A new ADR superseding this one.
- A migration commit that keeps `kiln-deps`'s public surface stable
  (`resolve`, `tree`).
- Removal of the `// TODO(M4 lib migration)` comments at each call
  site.

This decision does not block the migration; it defers it until
the library API stabilises around bender 1.x or our needs justify
the engineering cost.
