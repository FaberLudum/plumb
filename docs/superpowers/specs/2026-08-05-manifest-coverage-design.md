# Manifest coverage — Design Spec

**Date:** 2026-08-05
**Status:** approved
**Depends on:** the package existence gate (`feat/core-package-gate`)

---

## 1. Problem

`plumb check --diff` reports success on manifests it cannot read.

A Poetry `pyproject.toml` with no `[project]` table parses cleanly and yields an
empty dependency set, so `deps::added` sees nothing added and the run exits 0.
A `requirements.txt` whose only content is `-r requirements/base.txt` does the
same, because every `-`-prefixed line is dropped. A malformed `pyproject.toml`
also yields an empty set, which additionally makes every current dependency at
HEAD look newly added.

In each case the gate does not fail. It reports that it checked the manifest and
found nothing wrong. That is a silent false negative on a blocking gate — the
failure class this project exists to catch, in the project's own code.

The defect is not the missing parsers. It is that `manifest::parse` has no way
to say "recognised, not understood", so its two outcomes force a lie.

## 2. Goals

- Make the gate incapable of reporting success on a manifest it did not read.
- Say what was not read, specifically enough to act on.
- Leave the unread formats unread. Adding parsers is separate work.

## 3. Non-goals

- **Parsing Poetry, PEP 735 or `-r` includes.** Each is a real parser with its
  own syntax, not a branch. Adding them shrinks the surface; it never removes
  it, because the next unread format lies again without this change.
- **Build-time dependencies.** `[build-system].requires` is a different rule
  with different evidence, not an unread part of the runtime set. See §5.
- **Blocking on what is not understood.** A gate that fails on every Poetry
  project is uninstalled before it catches anything.

## 4. The type

```rust
pub struct Manifest {
    pub names: BTreeSet<String>,
    pub gaps: Vec<Gap>,
}

pub enum Gap {
    Unparseable,
    PoetryDependencies,
    DependencyGroups,
    Include(String),
}

pub fn parse(path: &str, text: &str) -> Option<Manifest>
```

`None` keeps its meaning: this path is not a manifest. **An empty `gaps` is the
only state that entitles the gate to say it checked the file.**

Names and gaps coexist because a manifest can be partly understood. A
`requirements.txt` holding both direct requirements and an `-r` include yields
the direct names *and* a gap. An enum of mutually exclusive outcomes would have
to discard names that were read correctly, trading a silent false negative for a
loud loss of coverage — worse in a different direction.

`Gap` names what was missed rather than carrying a boolean, because the warning
has to tell a reader what to do about it. "This file may declare more
dependencies" is true, useless, and the kind of message people learn to skip.

`Unparseable` also closes a hole recorded during the previous plan and left
open: a malformed `pyproject.toml` at HEAD makes every current dependency read
as newly added, silently. It now says so.

## 5. Warn on evidence, never on absence

A `pyproject.toml` containing only `[build-system]` and `[tool.ruff]` is a
legitimate file that declares no runtime dependencies. It must produce no gap.

A gap requires positive evidence that a dependency declaration exists and was
not read:

| Gap | Trigger |
|---|---|
| `Unparseable` | The TOML does not parse |
| `PoetryDependencies` | `[tool.poetry.dependencies]` exists and `[project].dependencies` does not |
| `DependencyGroups` | `[dependency-groups]` exists |
| `Include(path)` | A `-r` or `--requirement` line in a requirements file |

`[tool.poetry.dependencies]` alongside `[project].dependencies` is not a gap:
PEP 621 is authoritative there and the Poetry table mirrors it.

**`[build-system].requires` is deliberately not a gap.** It appears in nearly
every `pyproject.toml`, so treating it as one would fire the warning universally
— and a warning that always fires is a warning nobody reads. That would not make
the gate more honest; it would destroy the mechanism this design exists to
build.

## 6. How a gap reaches the user

`deps::added` and `check::run` change signature to carry gaps alongside added
dependencies. A gap becomes a Tier B finding, so a run with gaps and no missing
packages exits 2. A missing package remains Tier A and exits 1, and Tier A wins
when both are present — `exit_code` already resolves that.

One file can produce both: names looked up and checked, plus a warning about
what was not read. That is the partial case, and it is why the type has this
shape.

## 7. The `source` problem

`Finding::source` must name a taxonomy section, and a test added in the previous
plan reads `docs/research/taxonomy.md` and requires the cited heading to exist.

These findings have no study behind them. They are not claims about
AI-generated code; they are the tool reporting on itself. Filing them among the
evidence rows would break the project's first rule in the act of honouring it.

`taxonomy.md` gains a section `## 10. Coverage reporting` that states plainly
what these findings are: self-reports about what Plumb did not check, not
derived from evidence, existing to make the gate's silence visible. The test
passes because the heading exists; the taxonomy stays honest because the section
says outright that it is not evidence.

## 8. Testing

Every gap trigger needs a test that fails against an implementation that does
not detect it, and every non-trigger needs one that fails against an
implementation that over-detects. The second half matters more: this design's
main risk is a warning that fires on ordinary files, and the `[build-system]`
decision is the specific case to pin.

End to end, three verdicts must be distinguishable: exit 0 on a fully understood
manifest with no missing packages, exit 2 on a manifest with gaps and no missing
packages, exit 1 when a package is missing regardless of gaps.

Two properties this project has repeatedly failed, to check explicitly: no test
may pass against an implementation that reports nothing, and no comment may
describe behaviour the code does not have.

## 9. Success criteria

The gate cannot report success on a manifest it did not read. Demonstrated by a
Poetry project, a `-r` include and a malformed `pyproject.toml` each exiting 2
with a finding that names the file and the format.

And the negative half, which is what makes it survivable: a repository of
ordinary `pyproject.toml` files that declare dependencies the normal way
produces no warnings at all.

## 10. Open questions

- Whether `Include(path)` should eventually resolve the include rather than
  report it. Resolving needs filesystem access, which `plumb-core` does not
  have, so it belongs in the wiring layer if it happens at all.
- Whether a gap should suppress the "every dependency looks new" effect of an
  unparseable manifest at HEAD, or whether reporting both is clearer. The
  previous plan judged suppression wrong because it would also suppress the
  first dependency added to an empty project; that reasoning still applies, but
  it deserves re-examination now that the gap is visible.
