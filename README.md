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
Readability, maintainability and conciseness all came out slightly ahead for
AI-assisted code in GitHub's own randomised trial.

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
blocks on a name PyPI does not have. 124 tests, no network access from any of
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

What it does not do yet — the blind spots known today, listed because a gate
that stays quiet about them is worse than one that has none. The list is what
has been found, not a proof that nothing else is there:

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
- Two of the formats it does not read are worse than unread. A Poetry
  `pyproject.toml` with no `[project]` table is recognised as a manifest and
  read as declaring nothing, so every dependency it gains passes unexamined and
  unreported; a `requirements/base.txt` layout is not recognised at all,
  equally quietly. Both are silent false negatives on a blocking gate, which is
  the failure this project exists to catch, and they are the next thing to fix.
  PEP 735 `[dependency-groups]` and `[build-system].requires` are simply
  unread.
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

What the first rule rests on: Spracklen et al. (USENIX Security 2025) measured
19.7% of packages referenced by code-generating models as non-existent, across
2.23 million samples. Churilov (2026) re-measured 4.62–6.10% on frontier
models — a three- to fourfold drop, and still not zero. 43% of hallucinated
names recur identically across runs, which is what makes slopsquatting a
practical attack rather than a theoretical one.

Not yet built: the boundary check that makes an architecture document
executable, volume and coupling tracking, and the cross-session drift
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
telemetry. Registry answers are cached on disk under `.plumb/cache/`, and
`--offline` disables lookups entirely.

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
