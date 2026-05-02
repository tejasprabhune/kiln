# kiln-cli

The `kiln` binary. Builds a single executable named `kiln` from this crate.

This crate is the only place in the workspace where `println!`/`eprintln!`
are allowed. Everywhere else uses `tracing`.

See the top-level `README.md` for install instructions and the project
roadmap in `kiln-milestones.md` for which subcommands are wired up at any
given milestone.
