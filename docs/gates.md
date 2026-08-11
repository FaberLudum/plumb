# The gates in detail

Reference for what each check actually reads, what it rules out, and every
known gap. The short version is in the [README](../README.md); this file is
the long one, and the Blind spots section is the part worth reading before
trusting a clean run.

## The package gate

One rule blocks: every package a change adds must actually exist on its
registry. `plumb check --diff` asks git for the paths a change touched,
reads each manifest among them — `requirements.txt`, `requirements-*.txt`
and `pyproject.toml` on the PyPI side, `package.json` on the npm side — as
it stands and as it stands at HEAD, looks up only the names the change
adds, and blocks on a name its own registry does not have.

```console
$ plumb check --diff
blocking: package `reqeusts` does not exist on PyPI [package-exists]
  requirements.txt
  evidence: taxonomy §4.1
$ echo $?
1
```

npm reads the same way, against `package.json`'s four dependency tables —
`dependencies`, `devDependencies`, `peerDependencies`,
`optionalDependencies` — and blocks on the same shape of finding, `npm` in
place of `PyPI`:

```console
$ plumb check --diff
blocking: package `plumb-nonexistent-calibration-xyz` does not exist on npm [package-exists]
  package.json
  evidence: taxonomy §4.1
$ echo $?
1
```

A name already in the manifest is not re-checked and does not appear in
that output at all — only a name the change actually adds is looked up, on
either registry.

`--json` emits the same findings as an array, `--offline` never contacts a
registry. Exit codes are a fixed contract: `0` clean, `1` blocking, `2`
warnings only. A run that could not happen — not a repository, no git —
exits outside that contract instead, so a broken gate can never be read as
a clean one.

What the rule rests on: Spracklen et al. (USENIX Security 2025) measured
19.7% of packages referenced by code-generating models as non-existent,
across 2.23 million samples. Churilov (2026) re-measured 4.62–6.10% on
frontier models — a three- to fourfold drop, and still not zero. 43% of
hallucinated names recur identically across runs, which is what makes
slopsquatting a practical attack rather than a theoretical one.

## The Tier B gates

Three warning gates run alongside the manifest check.

**Boundary.** Imports are validated against the module boundaries declared
in `.plumb/architecture.md`: each changed file is attributed to a module by
path, its imports are extracted with a heuristic, line-based matcher
covering the Rust, Python and JavaScript/TypeScript family of import
syntaxes, and an import reaching a module outside the file's declared
`May depend on:` list warns rather than blocks. The comparison is
before-versus-after, the same shape as the manifest check's: **the gate
reports only violations the change introduces.** A violation already
present at HEAD is legacy, not something the change did, so a comment-only
edit to a file that has always had an undeclared import does not re-warn
on it.

```console
warning: crates/plumb-core/src/lib.rs imports `plumb_cli`, but crates/plumb-core may not depend on crates/plumb-cli [boundary]
  crates/plumb-core/src/lib.rs
  evidence: taxonomy §4.5
```

**Adapter drift.** Warns when the generated instruction files have drifted
from `.plumb/` — the same four files `plumb adapters` writes, compared
against what re-rendering `.plumb/` right now would produce, byte-for-byte
apart from line endings:

```console
warning: CLAUDE.md does not match .plumb/; run `plumb adapters` [adapter-drift]
  CLAUDE.md
  evidence: taxonomy §11
```

**Index staleness.** Warns when `.plumb/index.md` has drifted from the
tracked source files it claims to describe — a fresh scan of every tracked
source, compared against what is committed, on every invocation rather
than only over changed paths:

```console
warning: .plumb/index.md does not match the tracked sources; run `plumb index` [index-stale]
  .plumb/index.md
  evidence: taxonomy §11
```

## plumb report

`plumb report --commits <n>` (default 30, first-parent from HEAD) prints
three quantities per commit — **volume** (lines added and removed, from
`git show --numstat`), **deletion ratio** (removed over added), and
**coupling delta** (the change in cross-module import edges the commit's
touched files resolve to, read against `.plumb/architecture.md`) — and a
commit where volume and coupling delta both rise is flagged with a
**decay signature**, the pattern taxonomy §4.5 names straight off: volume
and coupling growing together.

```console
$ plumb report --commits 20
commits: 20, added: 20896, removed: 462
mean deletion ratio: 0.06
decay signature: 4 of 20 (c8e497d, 1ed73ca, d4e8fc0, 46b0918)
```

The signature is uncalibrated: `boundaries::edges` counts a permitted edge
exactly like a violation, and counts it per file, so any commit adding a
file that uses a declared dependency scores coupling +1 by construction —
ordinary growth against a boundary the project itself declared, not the
§4.5 decay pattern the signature is named for. It needs calibration
against real history before it can tell the two apart.

Coupling reads at module granularity, and it resolves sub-package
boundaries once they are declared: the resolver matches an import against
a module's *sub-package* headings, not only its top-level one, so a
repository whose real boundaries sit between sub-packages of one top-level
package resolves those boundaries after `.plumb/architecture.md` declares
the sub-packages as modules in their own right. An architecture that still
declares only the top-level package sees an inert `0`, because there is
nothing deeper to resolve against.

**It is not a gate.** No finding, no threshold, no exit-code effect —
`plumb report` always exits `0` regardless of what the numbers say, Tier C
instrumentation for an open hypothesis rather than a calibrated rule.

## What `--diff` compares, and what that rules out

**`--diff` compares the working tree against HEAD** — it runs
`git diff --name-only HEAD`, and a path is listed only when the working tree
differs. Anything already committed is outside that comparison: there is
nothing left to compare, so the run checks nothing and exits `0`. The same
manifest blocks while it is uncommitted and passes the moment it is committed.

The consequence is worth stating plainly, because it is the opposite of what a
gate is usually assumed to do. **On a fresh clone or a CI checkout the tree is
clean, so the manifest and boundary checks examine nothing and exit `0`
whatever the commit under test added.** Wiring this into a CI job that clones
a branch gives those two checks a permanently green result that is not
checking anything. Checking a commit or a commit range needs
`plumb check --repo`, which the design spec reserves and **which is not
implemented yet**. Until it is, the useful place for the manifest and
boundary checks is a developer's working copy.

The adapter-drift and index-stale checks are the exception to that promise,
by design: neither reads `git diff` at all, and both run once per invocation
over the repository's current state rather than over changed paths. So a
fresh clone of a repository whose committed generated files or index have
already drifted still exits `2` on the very first, otherwise-untouched run.

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

## Blind spots

What it does not do yet, listed because a gate that stays quiet about them is
worse than one that has none. The list is what has been found, not a proof that
nothing else is there:

- PyPI and npm, two ecosystems, added one at a time. .NET manifests are the
  same shape of code and are still not read.
- npm's coverage: one filename, `package.json`, at any depth, with any path
  segment named `node_modules` excluded entirely — a vendored `package.json`
  belongs to a dependency, not to this project, and checking it would block
  a build over a package nobody here added; npm nests dependencies, so
  `node_modules/foo/node_modules/bar/package.json` is excluded exactly like
  the top-level case. Four tables are read — `dependencies`,
  `devDependencies`, `peerDependencies`, `optionalDependencies` — and only
  their keys: a table's values are version *ranges*, and this gate checks
  existence, not whether a range is satisfiable, so `"left-pad": "^999.0.0"`
  passes exactly as cleanly as `"left-pad": "^1.0.0"` does. A table present
  but not an object (`"dependencies": []`) is a `manifest-understood`
  warning naming the table, the JSON sibling of the PEP 621 shape-mismatch
  warning below.
- **Lockfiles are not read.** `package-lock.json`, `yarn.lock` and
  `pnpm-lock.yaml` are invisible to Plumb; a name that exists only inside
  one of them — never declared in `package.json` itself — is never looked
  up in either direction. It does not block if the name is hallucinated,
  and it does not confirm if the name is real.
- **A private npm registry false-positives.** A name published only to a
  company's private registry or scope resolves against
  `https://registry.npmjs.org` alone, the public registry — the same gap a
  package that exists only on a private PyPI index has (see the third false
  positive below): Plumb cannot tell "does not exist anywhere" from "does
  not exist on the one registry it asked."
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

  Plumb checks PyPI and npm, not conda. A hallucinated conda package is a
  real defect and Plumb does not look for it. Concretely, no warning and no
  lookup
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
- Boundaries reach sub-packages now, not only top-level packages: a
  dotted or scoped import (`ai_energy_manager.documents`) is split into
  segments and resolved against every declared module's path, also split
  into segments — the import matches a module when the module path's
  *trailing* segments equal the import's *leading* segments, and the module
  scoring the longest such run wins. A heading `## src/pkg/documents`
  therefore beats `## src/pkg` for the import `pkg.documents.domain`,
  because it matches two segments instead of one; a plain top-level import
  still resolves exactly as before, since matching one segment is what the
  old top-segment rule did. Two things a declared module still needs to be
  reachable at all:
  - **The filesystem layout has to mirror the import path.** The resolver
    matches on the module's declared *path*, not on where its files
    physically live, so a Python package (`src/pkg/facts/` importable as
    `pkg.facts`) or a JS package under a matching directory name resolves;
    a project whose import name and directory name diverge — an exotic
    source root, a package renamed at publish time — does not, because
    nothing here reads `sys.path` or a bundler's alias config.
  - **A tie between equally-scoring modules goes to the first one declared**
    in `.plumb/architecture.md`, not the more specific or more recently
    added one — two modules named `api` at different depths (`apps/api` and
    `services/api`) both score 1 against a bare `import api`, and whichever
    heading comes first in the file wins.
  - **A bare `k = 1` match is still just a name match**, unchanged from the
    old top-segment rule: nothing here checks that a resolved module and the
    package the import actually names are the same thing, only that their
    text lines up. Splitting one top-level package into several
    finer-grained sub-packages multiplies how many short, common leaf names
    (`utils`, `core`, `models`) a declared module can carry, and each is
    exactly as exposed as a top-level module name always was to colliding
    with an unrelated third-party package of the same name. This is not the
    whole exposure, though: at `k ≥ 2` a declared module path that happens to
    *mirror* a third-party package's own path can collide, too — a module
    heading `## services/google/cloud` scores 2 against `from google.cloud
    import storage`, exactly as if the project depended on `google.cloud`,
    which the old top-segment rule could never do since it only ever
    compared one segment.

  What is still true regardless: Rust stays crate-level only. A Rust import
  never spells the `src` directory (`use plumb_core::manifest`, not
  `use plumb_core::src::manifest`), but every module file still sits under
  it, so `src` interposes between the crate directory a module heading
  names and the module file inside it — a crate-qualified import can reach
  a heading like `## crates/plumb-core` but never one shaped
  `## crates/plumb-core/src/manifest`, because no import segment will ever
  equal the literal `src`.
- The index scanner's symbol extraction is line-based, the same shape of
  matcher as the boundary gate's imports: a macro-generated declaration is
  missed because it never exists as a literal source line, and a multi-line
  declaration is missed past its first line. It also does not know about
  strings or comments, so a `pub fn` spelled out inside either can
  false-positive — dogfooding this on Plumb's own source caught it directly:
  `crates/plumb-core/src/index.rs` has tests whose expected-output strings
  contain the literal text `pub fn run`, `pub struct Outcome`, `pub fn
  method`, `pub const MARKER`, `pub const fn new`, `pub async fn go`, `pub
  unsafe fn raw` and `pub const unsafe fn x`, and every one of them is listed
  in `.plumb/index.md` as if it were a real public item of that module,
  alongside the ones that actually are. And Rust's own `pub` is
  over-inclusive on purpose here: `pub` inside a private (non-`pub`) module
  is still counted as public surface, because the scanner reads
  declarations, not effective visibility — so it over-reports rather than
  staying silent on re-exports it cannot trace. Rust's `const`, `async` and
  `unsafe` modifiers on a `pub fn` are recognised: `pub const fn`, `pub
  async fn`, `pub unsafe fn` and any combination of the three extract as
  kind `fn`, named after the last modifier, not as a fabricated `const`
  entry named `fn`; a plain `pub const NAME: …` or `pub static NAME: …`
  keeps `const`/`static` as its kind. Python's `async def` and JavaScript's
  `export default async function` are recognised the same way, each keeping
  its non-async kind (`def`, `function`).
- `plumb report`'s coupling delta resolves every commit's touched files
  against **today's** `.plumb/architecture.md`, not the architecture that
  commit's author actually saw — a module renamed or split afterward reads
  older commits under boundaries that did not exist yet at the time.
- The commit walk is `git log --first-parent`, the same reference `numstat`
  reads a commit's diff against: a pull request landed as a merge commit
  counts once, as the merge's own diff, not once per inner commit it
  carried. The two agree with each other, but a history of many small
  merges reports very differently from a rebase-and-fast-forward one over
  the same work.
- `numstat` runs `git show --first-parent --numstat`, and `--first-parent`
  on `git show` requires git 2.31 or newer; on an older git a merge
  commit's diff comes back empty, so numstat reads `+0/-0` for the whole
  commit and a merged pull request silently vanishes from both volume and
  coupling.
- `git show --numstat` quotes a path containing unusual characters (tabs,
  quotes, non-ASCII control bytes); that quoted form is still summed into
  volume, but coupling resolution matches paths as literal text, so a
  quoted path's line counts land in the volume total while its edges can
  go uncounted.
- A renamed file appears in `numstat` as `{a => b}/path.rs`; the scan
  rejects that literal string as a path, so its line counts still enter
  volume (rename-reduced, as git reports them) but its coupling edges
  silently vanish — a commit that renames a file and adds a new
  cross-module import in the same change reports coupling `0`.
- Coupling renders as `-` for every row, and the report falls back to
  volume and deletion ratio alone, whenever `.plumb/architecture.md` is
  absent — there is nothing to resolve an import edge against, and one
  stderr line says so once, not per commit.
- Coupling delta is `child_edges − parent_edges`, a plain count. A commit
  that swaps one cross-module edge for a different one nets to zero:
  replacing an undeclared dependency with an equally undeclared one shows
  no coupling change at all.
- Coupling delta counts a permitted edge exactly like a violation —
  `plumb report` measures coupling, it does not judge it — and edges are
  counted per file, so coupling delta tracks how many touched files import
  across a module boundary, not whether the import is allowed.

## Network and privacy

Plumb makes exactly one class of outbound request: resolving package names
against public registries. Nothing else leaves your machine, and there is no
telemetry. `--offline` disables lookups entirely.

Registry answers are cached on disk, **outside the repository being checked**:
under `%LOCALAPPDATA%\plumb\registry\` on Windows, and `$XDG_CACHE_HOME/plumb/`
or `~/.cache/plumb/` elsewhere, keyed per repository.

The location is a correctness property, not housekeeping. The cache is
unauthenticated and a fresh entry answers without contacting the registry, so
a cache file that travels with a change can assert that a nonexistent package
exists and
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
