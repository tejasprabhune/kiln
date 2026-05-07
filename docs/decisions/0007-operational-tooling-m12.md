# 0007. Operational tooling: watch, JUnit, frozen/locked (M12)

- Status: accepted
- Date: 2026-05-07

## Context

M10–M11 closed the manifest schema gaps that real hardware projects had
been working around with `extra_args`. The remaining audit items split
into "operational" (CI, dev loop, reporting) and "adoption" (templates,
imports, docs polish). M12 picks the operational subset.

Concretely:

1. **No watch loop.** `kiln check` / `kiln test` / `kiln build` are one-shot
   commands. The tight edit-test feedback loop a hardware engineer wants
   today requires shelling out to `entr`, `watchexec`, or `cargo-watch`.
2. **No machine-readable test results.** `kiln test` prints human output
   only. CI dashboards (Buildkite, GitLab, GitHub Actions test reporters,
   Jenkins) consume JUnit XML by default, and there is no way to feed
   them.
3. **No CI determinism gate.** `kiln update` / `kiln add` mutate
   `Kiln.lock` freely. CI cannot say "fail if anyone changed deps without
   committing the lockfile" without ad-hoc shell checks. Cargo and uv
   solve this with `--locked` (lockfile must match manifest) and
   `--frozen` (lockfile + offline; no network).

## Decision

Three additions, scoped tight, each with its own escape hatch.

### `kiln watch <subcommand>`

A new top-level subcommand that watches the project's source tree and
re-runs `<subcommand>` (one of `check`, `build`, `test`, `fmt`) whenever
a relevant file changes. Implementation rides
[`notify`](https://crates.io/crates/notify) for filesystem events plus a
short debounce so a single editor save doesn't fire twice.

```text
kiln watch check
kiln watch test --filter alu
kiln watch build --release
```

Watched roots: project root excluding `target/`, `.git/`, and any
directory matching `target/kiln/*`. File extensions that trigger a
re-run: `.sv`, `.svh`, `.v`, `.toml`, `.lock`, `.hex` (so firmware
artefact regeneration also triggers).

Debounce: 200 ms. The debounce timer resets while events keep
arriving — typical of save-on-build cadence in editors.

The watch loop is interrupt-driven and exits cleanly on Ctrl-C. It
treats subcommand failures the same way `cargo watch` does: report and
keep watching. The escape is just to not invoke `watch`.

### `kiln test --reporter junit`

Adds a `--reporter` flag to `kiln test` with values `human` (default)
and `junit`. When `junit` is selected, `kiln test` writes
`target/kiln/junit.xml` in the standard JUnit XML format expected by
GitHub Actions / GitLab / Jenkins / Buildkite consumers. Human output
is suppressed entirely so CI logs stay terse; the file is the source of
truth.

The schema is the de-facto Jenkins/GitLab dialect: a `<testsuites>`
root, one `<testsuite>` per `kiln test` invocation (named after the
project), and one `<testcase>` per discovered test. Failures emit
`<failure>` with the captured stderr; timeouts emit `<failure
type="timeout">`; passes have an empty body. Time is wallclock seconds.

### `--frozen` / `--locked`

Two CI-determinism flags accepted by every dep-touching command
(`kiln add`, `kiln remove`, `kiln update`, `kiln tree`, `kiln build`,
`kiln check`, `kiln test`, `kiln run`, `kiln doc`):

- `--locked`: error out if `Kiln.lock` would need to be regenerated to
  match `Kiln.toml`. Equivalent to a no-op `kiln update` followed by a
  diff-clean check.
- `--frozen`: implies `--locked`, plus refuses to make any network
  request during dependency resolution. The bender wrapper already
  reads from a local checkout cache; this flag just gates the cases
  where it would otherwise fetch.

Both flags compose with `--features` and `--profile`. `kiln update`
under `--locked` is a no-op (since by definition there's nothing to
update); under `--frozen` it errors with a clear message.

### Out of scope for M12

- `--format json` for `kiln check` / `kiln build` / `kiln tree`. This is
  big enough to need its own schema design and `docs/json-output.md`
  expansion; future milestone.
- TAP, TeamCity, Bazel, or other test-result formats. JUnit covers
  >90% of CI consumers; add others on demand.
- A full `cargo-watch`-style command DSL (delays, clear screen,
  pre/post commands). One subcommand per invocation is enough for now.
- `kiln self update`, `kiln package`, multi-simulator backend
  abstraction — separate milestones, listed in `docs/status.md`.

## Consequences

**Easier:**
- Edit-test loop drops from "tab to terminal, hit up-arrow, return" to
  zero.
- CI reporters light up automatically once `--reporter junit` is in the
  test invocation.
- "Did the lockfile drift?" is a one-flag check instead of a custom
  shell script.

**Harder:**
- `notify` is a non-trivial new runtime dependency. Adds ~200 KiB to
  release binaries; acceptable.
- JUnit schema needs careful escaping (XML-quote every captured stdout
  line). One round of fuzz-style testing in unit tests covers this.
- `--frozen` semantics depend on what the bender wrapper does
  internally for network access. We treat it as best-effort: kiln
  doesn't itself reach the network, but we can't guarantee an opaque
  subprocess won't.

**Tradeoffs:**
- We do not introduce per-subcommand watch flags (e.g.
  `kiln check --watch`). `kiln watch <subcommand>` keeps the watch
  knowledge in one place. Cargo settled on the same shape with
  `cargo watch`.
- JUnit's `<system-out>` per testcase is omitted by default to keep
  CI report sizes manageable. Pass `-v` to include captured stdout in
  the XML.
- `--frozen` is best-effort, not a sandbox. Documented as such.
