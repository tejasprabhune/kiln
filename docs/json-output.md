# JSON output schemas

`kiln` commands that surface check / format / lint results expose a
machine-readable JSON form via `--format json`. This document is the
authoritative schema reference. Snapshot tests pin each shape against
example projects so changes have to land deliberately.

## `kiln fmt --check --format json`

```json
{
  "results": [
    {
      "file": "<absolute path>",
      "ok": true,
      "diff": ""
    },
    {
      "file": "<absolute path>",
      "ok": false,
      "diff": "--- <path>\n+++ <path> (formatted)\n …"
    }
  ],
  "summary": {
    "total": 2,
    "needs_formatting": 1
  }
}
```

Exit code is `0` when all files are canonical; `1` otherwise. The
`diff` field is always populated (empty when `ok` is true).

## `kiln fmt --format json` (no `--check`)

```json
{
  "formatted": ["<path>", ...],
  "unchanged": ["<path>", ...]
}
```

Exit code is always `0` if no I/O error occurred — the command
mutates the workspace, so "succeeded" is the right exit story.

## `kiln check --format json`

Reserved. M3 ships `kiln check` with plain-text rendering only; the
JSON shape will be added in a follow-up that documents the schema
here. For now, parse stderr/stdout text or wait for the JSON path.

## Stability

Adding fields to existing objects is permitted without a breaking-
change bump. Renaming or removing fields requires:

1. A documentation update here.
2. A snapshot-test update at the same time.
3. A note in the release notes calling it out.

Consumers should ignore unknown fields.
