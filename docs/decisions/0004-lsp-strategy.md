# 0004. LSP strategy: wrap hudson-trading/slang-server as a subprocess

- Status: accepted
- Date: 2026-05-02

## Context

`kiln lsp` is the entry point an editor (Neovim, VS Code, Helix, …) spawns
to get diagnostics, hover, goto-def, completions, and other live-feedback
features for SystemVerilog. It runs for the lifetime of an editor session
and speaks Language Server Protocol (LSP) over stdio.

Three SystemVerilog LSPs exist in the wild today:

1. **`hudson-trading/slang-server`** — built on the slang library. Provides
   diagnostics + linting, hover, goto-def, completions, references,
   inlay hints. Backed by HRT, active development, releases on a regular
   cadence (latest v0.2.5 as of 2026-04). Ships **prebuilt tarballs** for
   Linux x64 (clang and gcc variants), macOS, and Windows on each GitHub
   release.
2. **`verible-verilog-ls`** — ships in the verible bundle that
   `kiln install-tools` already downloads. Mature parser, but the LSP
   surface is narrower (no semantic-aware features like elaborated
   goto-def into instantiated modules).
3. **`svls`** — third-party Rust SV LSP. Lighter feature set, separate
   project not in our existing tool wrapping.

We also evaluated implementing the LSP ourselves on top of `slang-rs` (the
subprocess wrapper from M1). Rejected: `slang-rs` is request/response, not
a long-lived elaboration session. Building the AST cache and incremental
update story is a separate project — `slang-server` already has it.

## Decision

`kiln lsp` wraps `hudson-trading/slang-server` as a subprocess.

The wrapper does three things:

1. **Locate** the `slang-server` binary on `PATH` (or via `KILN_SLANG_SERVER_PATH`).
   Clear, actionable error when missing — points at `kiln install-tools`.
2. **Generate** `.slang/server.json` at the project root from `Kiln.toml`:
   - `flags` is a slang argument string: `-I <dir>` per `[design].include_dirs`
     and per dep-resolved include dir, `+define+NAME=VALUE` per
     `[design].defines`, `--top <name>` from `[design].top`, and `-W<id>`
     for each `[lint]` rule (matching `kiln check`'s logic).
   - `index.dirs` is the union of `[design].sources` glob roots plus every
     bender-resolved dependency package's directory, so jumping to a
     symbol from a dep resolves into the cloned bender cache.
   - A `_generator: "kiln lsp"` field marks the file as kiln-managed; if
     the file exists without that marker the wrapper refuses to clobber
     and tells the user to move their hand-written config.
3. **Exec** `slang-server` (replacing the kiln process via `exec(2)` on
   Unix). The editor's stdin/stdout pass through to slang-server with
   no buffering, no bridging.

`kiln install-tools` gets a fourth easy tool added to its default set:
`slang-server` is fetched as a prebuilt tarball from the upstream GitHub
releases, parallel to the existing `verible` install path.

We are **not** wrapping verible's LSP. The two have non-overlapping
feature sets (verible-verilog-ls is heavier on syntactic operations like
formatting / token-level navigation; slang-server is heavier on semantic
operations like elaborated goto-def). Picking one keeps the editor
config simple. Users who want verible-verilog-ls can run it alongside
`kiln lsp` by registering a second LSP for the same filetype — that's
the editor's job to multiplex, not kiln's.

## Consequences

Easier:

- The hard work (LSP protocol implementation, slang elaboration cache,
  incremental update) lives upstream. `kiln lsp` is a thin config
  translator + `exec()`.
- One install command for end users: `kiln install-tools` fetches the
  prebuilt slang-server alongside bender, verible, and surfer.
- `[lint]` severity overrides flow naturally — slang-server consumes
  the same `-W<id>` flags `kiln check` already passes.
- Bender-resolved dep sources flow into `index.dirs` via the same
  `kiln_deps::resolve` call the build pipeline uses.

Harder:

- **`Kiln.toml` changes don't auto-propagate.** When the user edits
  `[lint]` or adds a `[dependencies]` entry, the LSP keeps the old
  config until `kiln lsp` is restarted. The editor's `:LspRestart`
  command (or equivalent) handles this. v2 could watch `Kiln.toml`
  and regenerate, but watching files from inside an LSP wrapper is a
  separate complexity worth deferring.
- **`.slang/server.json` is kiln-managed**, which surprises a user
  with a hand-written config there. Mitigation: `_generator` marker +
  refuse-to-clobber check + clear error message.
- **slang-server is C++** and pinned by version. We pin to a known-good
  release (v0.2.5 today) and bump explicitly. Behavior changes between
  releases get caught by integration tests.

## Reversal

Reversing this decision (going to verible-verilog-ls or our own LSP)
requires a new ADR superseding this one. The wrapper layer
(`kiln-cli/src/commands/lsp.rs`) is small enough that swapping the
underlying server binary is mostly: change which binary is `exec()`'d
and which config schema we generate. The translation logic
(`Kiln.toml` → flags + index dirs) is reusable.
