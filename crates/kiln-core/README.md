# kiln-core

Manifest, project model, and shared error types for `kiln`.

This crate is library-only. It exposes:

- `Manifest`: the parsed `Kiln.toml` schema.
- `ManifestError`: typed errors for manifest loading and validation.
- `ProjectLayout`: helpers for resolving the manifest path from a working
  directory.

The runtime CLI lives in `kiln-cli`.
