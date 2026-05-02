# `Kiln.lock` lockfile specification

`Kiln.lock` is a verbatim copy of bender's `Bender.lock`, written to the
project root after a successful `kiln update` (and on every `kiln build`
that resolves a non-empty `[dependencies]` table).

## Why a renamed file rather than a kiln-native format?

We delegate dependency resolution to bender (see ADR
`docs/decisions/0003-bender-integration.md`). bender's lockfile is
deterministic, well-documented, and stable. Round-tripping through a
kiln-specific format would gain nothing and lose interoperability with
the rest of the PULP toolchain.

The file is renamed to `Kiln.lock` (not `Bender.lock`) so users see one
consistent set of `Kiln.*` artifacts and so a future migration to a
different resolver doesn't require touching every kiln user's `.gitignore`.

## Schema

```yaml
packages:
  <name>:
    revision: <git sha or null>
    version: <semver string or null>
    source:
      Git: <url>          # for git deps
      # or
      Path: <abs path>    # for path deps
    dependencies:
      - <transitive dep name>
      - ...
```

The exact shape is defined by bender. See
<https://github.com/pulp-platform/bender> for upstream documentation.

## When to commit it

`Kiln.lock` should be committed alongside `Kiln.toml`. It gives
reproducible builds: a fresh `kiln build` on a different machine resolves
to byte-identical source content as long as the lockfile is intact.

When you change `[dependencies]` in `Kiln.toml`, run `kiln update` to
refresh the lockfile, then commit both files together.

## When to delete it

Almost never. Specifically:

- **Adding a new dep**: don't manually edit `Kiln.lock`; `kiln add` does
  the right thing.
- **Bumping a transitive dep**: `kiln update` re-resolves.
- **Catastrophic corruption**: `rm Kiln.lock && kiln update` is the
  reset.

Any divergence between `Kiln.toml` and `Kiln.lock` is detected by
`kiln update` and surfaced as a bender error.
