# Editor / LSP setup

`kiln lsp` is a thin wrapper around
[hudson-trading/slang-server](https://github.com/hudson-trading/slang-server).
Editors spawn it over stdio. It generates `.slang/server.json` from
`Kiln.toml` on every launch and exec's slang-server. Features that work
out of the box:

- Diagnostics + linting (with `[lint]` severity overrides from `Kiln.toml`)
- Hover
- Goto-definition (including into bender-resolved git deps)
- Completions
- References
- Inlay hints

## Install

```bash
cargo install kiln                                     # the kiln binary
kiln install-tools --tools slang-server                # the LSP backend
```

The slang-server binary lands at `~/.local/share/kiln/bin/slang-server`
(or wherever you set `--prefix` / `$KILN_TOOLS_DIR`). Make sure that
directory is on your `PATH`. Alternatively, point at a binary with
`KILN_SLANG_SERVER_PATH=/path/to/slang-server`.

## Neovim (≥ 0.11)

```lua
-- ~/.config/nvim/lsp/kiln.lua
return {
  cmd = { 'kiln', 'lsp' },
  filetypes = { 'systemverilog', 'verilog' },
  root_markers = { 'Kiln.toml' },
}
```

```lua
-- ~/.config/nvim/init.lua
vim.lsp.enable('kiln')
```

Open any `.sv` file under a tree containing `Kiln.toml`. Neovim launches
`kiln lsp` in the project root; diagnostics flow within ~1s of opening.

For older Neovim with `nvim-lspconfig`:

```lua
require('lspconfig.configs').kiln = {
  default_config = {
    cmd = { 'kiln', 'lsp' },
    filetypes = { 'systemverilog', 'verilog' },
    root_dir = require('lspconfig.util').root_pattern('Kiln.toml'),
    settings = {},
  },
}
require('lspconfig').kiln.setup {}
```

## VS Code

There's no kiln VS Code extension yet. Use the slang-server VS Code
extension directly (it's on the marketplace) — point it at a
`.slang/server.json` you generate by running `kiln lsp` once in your
project (it'll generate the config and wait for an LSP request, which
you can `Ctrl-C`).

A first-class `kiln-vscode` extension is M9 follow-up work.

## Helix

```toml
# ~/.config/helix/languages.toml

[language-server.kiln]
command = "kiln"
args = ["lsp"]

[[language]]
name = "verilog"
language-servers = ["kiln"]
```

## What kiln writes

`kiln lsp` regenerates `<project>/.slang/server.json` on every launch.
Example for a project with one git dep and a width-trunc lint promotion:

```json
{
  "_generator": "kiln lsp",
  "flags": "--top tb -I /proj/inc -I /home/.bender/axi-v0.39/include +define+DEBUG -Wwidth-trunc",
  "index": [
    {
      "dirs": [
        "/proj/src",
        "/home/.bender/axi-v0.39/src"
      ]
    }
  ]
}
```

The `_generator` field marks the file as kiln-managed. If you write your
own `.slang/server.json`, kiln refuses to clobber it and tells you to
move it aside.

Add `.slang/` to your project's `.gitignore` (`kiln new` does this for
you on new projects).

## Reload after editing `Kiln.toml`

`kiln lsp` only generates the config at startup. When you change
`[lint]`, add a dep, or edit `[design]`, the running LSP keeps the old
config until restarted. Most editors expose this:

- Neovim: `:LspRestart`
- Helix: `:lsp-restart`
- VS Code: `Developer: Restart Extension Host` (slang-server extension)

Auto-reload on `Kiln.toml` changes is planned for a follow-up.

## Falling back

If slang-server is missing or fails to start, `kiln lsp` exits with a
clear error. Workarounds:

- `kiln install-tools --tools slang-server` to install it.
- `KILN_SLANG_SERVER_PATH=/path/to/slang-server kiln lsp` to override
  binary discovery.
- Run slang-server directly without kiln; you'll need to write
  `.slang/server.json` yourself. See
  <https://hudson-trading.github.io/slang-server/start/config/> for
  the schema.
