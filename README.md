# Plumb

A plumb line checks whether a structure stands true. This tool does the same
for codebases written with AI assistance: it does not judge whether each
function is well made, it checks whether the system is still upright.

## Why this exists

The evidence says something counterintuitive.

At the **unit** level, AI-generated code is equal to or better than human code.
Cyclomatic complexity is 2.62 against 2.47 for humans — a negligible
difference. AI functions are *smaller* than human ones, 138.97 AST nodes
against 201.82. Humans put more functions in a file, 11.67 against 6.62.
Readability and maintainability both came out slightly ahead for AI-assisted
code in GitHub's own randomised trial — +3.62% and +2.47%.

At the **system** level, it degrades. 22.7% of AI-introduced technical debt is
still alive at the latest version of the projects that received it.
Dependencies loaded at runtime outnumber those declared by 13.5x. Architectural
decay tracks total code volume closely enough that volume alone predicts it.

Every quality tool in common use is calibrated on human failure modes. Human
code has a heavy tail — a few enormous functions among many ordinary ones — so
those tools hunt for peaks. AI code is uniformly moderate. The tools find
nothing while the system decays below their thresholds.

Plumb targets the failure modes those tools structurally cannot see.

## The one rule

**Every rule Plumb enforces traces back to a row in
[docs/research/taxonomy.md](docs/research/taxonomy.md). No source, no rule.**

That applies to the maintainers, not only to contributors. The taxonomy also
lists the defects Plumb must *not* fight — function length, cyclomatic
complexity, readability thresholds — because the evidence says AI already
matches or beats humans on them, and enforcement spent there is friction with
no return.

Rules are tiered by strength of evidence, not by severity:

| Tier | Enforcement | Evidence required |
|---|---|---|
| A | Blocks the build | Peer-reviewed, objective detection |
| B | Warns | Preprint, or thresholds needing calibration |
| C | Measures only | None — instrumentation for open hypotheses |
| Hygiene | Autofixed, excluded from defect counts | Style consistency, delegated to formatters |

Proposing a Tier A rule on a preprint is a rejection. Propose it as Tier B.

## Status

**Early, and honest about it.** One rule is built and runs: every package a
change adds must actually exist on its registry. `plumb check --diff` asks git
for the paths a change touched, reads each Python manifest among them as it
stands and as it stands at HEAD, looks up only the names the change adds, and
blocks on a name PyPI does not have. 142 tests, no network access from any of
them.

```console
$ plumb check --diff
blocking: package `reqeusts` does not exist on PyPI [package-exists]
  requirements.txt
  evidence: taxonomy §4.1
$ echo $?
1
```

`--json` emits the same findings as an array, `--offline` never contacts a
registry. Exit codes are a fixed contract: `0` clean, `1` blocking, `2`
warnings only. A run that could not happen — not a repository, no git — exits
outside that contract instead, so a broken gate can never be read as a clean
one.

Two more commands exist alongside the check: `plumb init` writes a fresh
project's `.plumb/` working agreement — `architecture.md`, `conventions.md`,
`invariants.md` and an empty `index.md` — from a short bootstrap Q&A.
`plumb adapters` renders that working agreement into the instruction files
each coding harness reads: `CLAUDE.md`, `AGENTS.md`, `.cursorrules` and
`.github/copilot-instructions.md`, refusing to overwrite a hand-written file
unless `--force` is given. This repository dogfoods both: its own
`CLAUDE.md`, `AGENTS.md`, `.cursorrules` and Copilot instructions are
generated output, not hand-maintained — edit `.plumb/` and run
`plumb adapters` to update them.

Two Tier B gates run alongside the manifest check. The first validates
imports against the module boundaries declared in `.plumb/architecture.md`:
each changed file is attributed to a module by path, its imports are
extracted with a heuristic, line-based matcher covering the Rust, Python and
JavaScript/TypeScript family of import syntaxes, and an import reaching a
module outside the file's declared `May depend on:` list warns rather than
blocks:

```console
warning: crates/plumb-core/src/lib.rs imports `plumb_cli`, but crates/plumb-core may not depend on crates/plumb-cli [boundary]
  crates/plumb-core/src/lib.rs
  evidence: taxonomy §4.5
```

The second warns when the generated instruction files have drifted from
`.plumb/` — the same four files `plumb adapters` writes, compared against
what re-rendering `.plumb/` right now would produce, byte-for-byte apart from
line endings:

```console
warning: CLAUDE.md does not match .plumb/; run `plumb adapters` [adapter-drift]
  CLAUDE.md
  evidence: taxonomy §11
```

### What `--diff` compares, and what that rules out

**`--diff` compares the working tree against HEAD** — it runs
`git diff --name-only HEAD`, and a path is listed only when the working tree
differs. Anything already committed is outside that comparison: there is
nothing left to compare, so the run checks nothing and exits `0`. The same
manifest blocks while it is uncommitted and passes the moment it is committed.

The consequence is worth stating plainly, because it is the opposite of what a
gate is usually assumed to do. **On a fresh clone or a CI checkout the tree is
clean, so `plumb check --diff` examines nothing and exits `0` whatever the
commit under test added.** Wiring this into a CI job that clones a branch gives
a permanently green check that is not checking anything. Checking a commit or a
commit range needs `plumb check --repo`, which the design spec reserves and
**which is not implemented yet**. Until it is, the useful place for `--diff` is
a developer's working copy.

The pre-commit story is not honest yet either, and for a reason worth stating
precisely: **`git commit` commits the index, and this gate never reads the
index.** Two consequences follow.

- An addition that is staged and then reverted on disk is **not listed at all**,
  so the manifest is never opened. `git diff --cached --name-only HEAD` prints
  the path; `git diff --name-only HEAD` prints nothing.
- A manifest staged one way and edited another **is** listed, and the after-side
  is read from the working tree — so the gate checks content that is not about
  to be committed, and misses the content that is.

Either way the commit can land a package the gate never looked up. Staging does
matter, but only in one narrower way than it sounds: it makes an untracked file
*tracked*, and an untracked file is not reported at all.

Fixing this means reading the after-side as the union of the index and the
working tree, which changes what "added" means. `git show :<path>` alone is not
the fix: an unstaged edit has an index copy identical to HEAD, so it would add
nothing and report nothing.

### Blind spots

What it does not do yet, listed because a gate that stays quiet about them is
worse than one that has none. The list is what has been found, not a proof that
nothing else is there:

- PyPI only. TypeScript and .NET manifests are the same shape of code, one
  ecosystem at a time.
- Three filenames, at any depth: `requirements.txt`, `requirements-*.txt` and
  `pyproject.toml`. From a `pyproject.toml` it reads PEP 621 `[project]`
  `dependencies` and `optional-dependencies`, nothing else.
  `requirements_dev.txt` and `requirements.dev.txt` are knowingly given up,
  because admitting `_` or `.` as the separator would also admit
  `requirements_notes.txt`, and prose read as a manifest manufactures blocking
  findings. Restricting it to `-` narrows that problem without solving it — see
  the first of the false positives below.
- Several declaration shapes inside a recognised manifest are still unread,
  but no longer silently. Each produces a Tier B `manifest-understood` warning
  naming the file and the shape that was missed, and the run exits `2`:
  - Poetry's tables, `[tool.poetry.dependencies]` and
    `[tool.poetry.group.<name>.dependencies]`.
  - PEP 735 `[dependency-groups]`.
  - An `-r`/`--requirement` include in a requirements file.
  - PEP 621 `dynamic` listing `dependencies` or `optional-dependencies` — the
    standard's own way of saying the dependencies are declared somewhere else.
  - Any key named `dependencies`, or ending in `-dependencies`, anywhere under
    `[tool]`, whose value is an array or a table. This is a rule about the
    shape of the key, not a list of tools, and it is what covers
    `[tool.pdm.dev-dependencies]`, `[tool.hatch.envs.<name>].dependencies` and
    `.extra-dependencies`, `[tool.uv].dev-dependencies` and
    `.extra-build-dependencies`, `[tool.pixi.pypi-dependencies]` and its conda
    tables, `[tool.setuptools.dynamic].dependencies` and the pre-1.2
    `[tool.poetry.dev-dependencies]` — none of which is named anywhere in the
    code.

    Four things keep it off ordinary configuration. A matched key is not
    descended into, so a package called `dependencies` inside a pixi or Poetry
    table is not read as a second declaration. Only an array or a table
    counts, which is what excludes hatch's boolean
    `require-runtime-dependencies`. Two exclusions are carried explicitly,
    both against a published schema: `[tool.ruff.analyze.include-dependencies]`
    maps file paths to file globs rather than to packages, and uv's
    `constraint-`, `build-constraint-`, `override-`, `exclude-` and rye's
    `excluded-dependencies` are documented as never installing anything, so a
    project using them is read in full and does not warn. And the conda
    namespaces are out of scope — see below.
  - A declaration whose value is not the shape the reader takes, such as
    `dependencies = "ruff"` instead of a list.

  On a Poetry manifest it prints `pyproject.toml was not fully read:
  dependencies are declared under [tool.poetry] — in
  [tool.poetry.dependencies] or a [tool.poetry.group.<name>.dependencies]
  table — which Plumb does not read`; on a PDM one, `pyproject.toml was not
  fully read: dependencies are declared in [tool.pdm.dev-dependencies], which
  Plumb does not read`. The dependencies in those shapes are still not looked
  up — the warning reports the gap, it does not close it.
- What is still silent, measured rather than assumed. Two kinds:

  **Files not recognised as manifests at all**, because recognition is by
  filename: `requirements.in` (the hand-edited pip-compile source — exactly
  where a hallucinated name is introduced, before it is ever compiled into a
  pinned `requirements.txt`), `requirements/base.txt` reached through an
  include (the include warns, the file it names is never read on its own),
  `requirements_dev.txt` and `requirements.dev.txt`, `setup.py`, `setup.cfg`
  and `Pipfile`.

  **Declarations inside a recognised `pyproject.toml` that are not spelled
  `dependencies`.** The structural rule above keys on that word, so a tool
  that calls its dependency list something else is not covered. Measured, each
  exiting `0` with `plumb: no findings` over a package that does not exist:
  `[tool.tox.env_run_base].deps`, `[tool.briefcase.app.<name>].requires`,
  `[tool.cibuildwheel].test-requires`, `[tool.flit.metadata].dev-requires`,
  PEP 725 `[external].dependencies` — which is a top-level table, not under
  `[tool]` — and `[build-system].requires`. The last is deliberate: it appears
  in nearly every `pyproject.toml`, and a warning that always fires is one
  nobody reads.

  These are silent false negatives on a blocking gate, which is the failure
  this project exists to catch. The list is what was measured, not what was
  assumed, and it is not a proof that nothing else is missing: any tool that
  spells its dependency list some third way is silent until someone finds it.
- **Conda dependencies are out of scope, and are not checked at all.** This is
  a decision rather than a gap, and it is stated here because the difference is
  invisible from the outside: a pixi or conda-lock project gets a clean run, and
  a clean run must not be read as "these dependencies were checked".

  Plumb checks one registry, PyPI. A hallucinated conda package is a real
  defect and Plumb does not look for it. Concretely, no warning and no lookup
  for `[tool.pixi.dependencies]`, `[tool.pixi.feature.<name>.dependencies]`,
  pixi's `build-`, `host-` and `run-dependencies` under `package`, its
  per-platform `[tool.pixi.target.<platform>.…]` variants, or
  `[tool.conda-lock.dependencies]`. The PyPI side of the same file *is*
  checked: `[tool.pixi.pypi-dependencies]` and its scoped forms warn like any
  other unread table.

  Excluded rather than warned about because this gap has no resolution path.
  Every other one ends when someone teaches Plumb the format; this one never
  would, since Plumb will not look a conda name up on PyPI — so it would be a
  permanent warning that no action could clear, landing on projects that did
  nothing wrong. Checking conda needs a conda rule with its own evidence.
- A manifest that is not valid UTF-8 is not checked, and says so the same way:
  one Latin-1 byte costs the whole file, nothing in it is looked up, and the
  run warns — `requirements.txt was not fully read: not valid UTF-8 at byte 5`
  — and exits `2`. Not a blocking `1`, because a file the tool could not
  decode is not evidence that a package does not exist.
- Three known false positives, each of which blocks a build that is fine. The
  worst is prose: a file named `requirements-and-design.txt` is read as a
  manifest, and the first word of every line becomes a name to look up — four
  lines of English about a service and its latency budget block the build on
  `latency`. Second, a requirement given as a direct URL (`name @ https://…`)
  yields a name the index does not have. Third, moving a manifest without
  editing it re-checks every name in it, because nothing stands at its new path
  in HEAD, so a package from a private index blocks a rename that changed no
  dependency at all. Only the third is a trade taken on purpose: before it, a
  manifest moved into place carried its additions past the gate unlooked-at.
- `git diff HEAD` reports tracked changes only, so a manifest that has never
  been staged is invisible.
- The boundary gate's import extraction is line-based, one `text.lines()`
  pass per file: an import split across multiple lines (a parenthesised
  Python `from x import (a, b)`, a multi-line Rust `use` group) is missed
  entirely, and so is one produced by macro expansion — a `use` a Rust macro
  generates never exists as a literal source line for the extractor to match.
  It also does not know about strings or comments, so an import-shaped line
  inside either can false-positive. Module attribution is
  path-prefix-based — the longest declared module path that prefixes the
  file's path wins — so a module boundary that does not line up with the
  directory layout is not seen. The drift comparison deliberately ignores
  line-ending differences: git normalises CRLF and LF at checkout, so
  comparing raw bytes would warn on every clean Windows clone for a
  difference nobody introduced.

What the first rule rests on: Spracklen et al. (USENIX Security 2025) measured
19.7% of packages referenced by code-generating models as non-existent, across
2.23 million samples. Churilov (2026) re-measured 4.62–6.10% on frontier
models — a three- to fourfold drop, and still not zero. 43% of hallucinated
names recur identically across runs, which is what makes slopsquatting a
practical attack rather than a theoretical one.

Not yet built: volume and coupling tracking, and the cross-session drift
instrumentation. Those are what distinguish Plumb from a linter, and they come
next.

## Building

Rust 1.97, edition 2024.

Those are two different things and the numbers invite confusion. `1.97` is the
compiler, released July 2026 — current stable. `2024` is the language edition,
of which exactly four exist: 2015, 2018, 2021 and 2024. There is no newer one
to adopt.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The version is not repeated in those commands or in CI: `rust-toolchain.toml`
names it, and `rustup toolchain install` installs what that file selects.

Dependencies track current releases and none is pinned by hand. `Cargo.lock` is
committed and builds use `--locked`. Every third-party crate has a row in
[docs/dependencies.md](docs/dependencies.md) saying why the standard library is
not enough — Plumb's own Tier A rule, applied to Plumb.

## Network and privacy

Plumb makes exactly one class of outbound request: resolving package names
against public registries. Nothing else leaves your machine, and there is no
telemetry. `--offline` disables lookups entirely.

Registry answers are cached on disk, **outside the repository being checked**:
under `%LOCALAPPDATA%\plumb\registry\` on Windows, and `$XDG_CACHE_HOME/plumb/`
or `~/.cache/plumb/` elsewhere, keyed per repository.

The location is a correctness property, not housekeeping. The cache is
unauthenticated and a fresh entry answers without contacting PyPI, so a cache
file that travels with a change can assert that a nonexistent package exists and
the gate will believe it — silently, and for up to thirty days. Earlier versions
kept it at `<repo>/.plumb/cache/registry.json`, where committing one file
disabled the gate. **Never keep a Plumb cache inside a repository.** If one is
still there from an earlier version, the run says so on stderr; it is not read,
and it can be deleted.

If no cache directory can be resolved at all — a container with no `HOME` — the
run does not cache. It says so and works from the network. There is deliberately
no fallback to the temporary directory: a predictable path in a world-writable
directory is a trust store any other local user can pre-create, which is the
same hole by a different route. A cold cache costs a round trip; a poisoned one
costs the gate.

`PLUMB_CACHE_FILE` names the cache file outright, overriding all of the above.
It exists because the test suite has to point the binary somewhere isolated to
stay off the network, and because a persistent worker or a developer with an
unusual home directory may need to say where the file goes. **It is not
containment-checked**: pointed inside a repository it reinstates exactly the
hole described above, and the run will warn on stderr and carry on. Note also
that it does not make a CI check meaningful — see
[what `--diff` compares](#what---diff-compares-and-what-that-rules-out); on a
clean checkout there is nothing to look up, so there is nothing to cache.

An unreachable network is never a rule violation. A blocking gate that fails
when the network hiccups gets disabled within a week, and then catches nothing
at all. So a name that cannot be resolved yields no finding — including every
name in an offline run with a cold cache, which will report nothing and exit
`0`. An offline run that answered from cache entries past their lifetime says
so on stderr; one that could not answer at all does not, and that is a gap
worth knowing about before wiring `--offline` into CI.

## Contributing

A pull request that adds or changes a rule must cite its evidence: the source,
the metric, the effect size and the study's limits, plus whether the primary
source was verified or a secondary report is being relied on. Add the row to
the taxonomy in the same pull request.

A rule without a source is closed, however sensible it sounds. That is the only
thing separating this project from another opinionated linter.

The rest of it — tiers, where code goes, why no test may reach the network — is
in [CONTRIBUTING.md](CONTRIBUTING.md).

## Mascot

Plumby. The friendly name belongs on the mascot, not on the command.

## License

Apache-2.0. See [LICENSE](LICENSE).
