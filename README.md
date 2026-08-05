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

**Early, and honest about it.** The first rule — every package a change adds
must actually exist on its registry — has its components built and tested, but
they are not yet wired into a working command. `plumb --version` is the only
thing the binary does today.

What exists and is tested: git diff parsing, Python manifest parsing for
`requirements*.txt` and PEP 621 `pyproject.toml`, added-dependency computation,
findings with tier-based exit codes, and a PyPI client with an on-disk cache,
an expiry policy and an offline mode. 81 tests, no network access from any of
them.

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
to adopt. Dependencies track current releases, none is pinned by hand.

```bash
cargo test --workspace --locked
```

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Dependencies track current releases and none is pinned by hand. `Cargo.lock` is
committed and builds use `--locked`. Every third-party crate has a row in
[docs/dependencies.md](docs/dependencies.md) saying why the standard library is
not enough — Plumb's own Tier A rule, applied to Plumb.

## Network and privacy

Plumb makes exactly one class of outbound request: resolving package names
against public registries. Nothing else leaves your machine, and there is no
telemetry. Registry answers are cached on disk, and `--offline` disables
lookups entirely — unresolved names are then reported as unknown rather than as
violations.

An unreachable network is never a rule violation. A blocking gate that fails
when the network hiccups gets disabled within a week, and then catches nothing
at all.

## Contributing

A pull request that adds or changes a rule must cite its evidence: the source,
the metric, the effect size and the study's limits, plus whether the primary
source was verified or a secondary report is being relied on. Add the row to
the taxonomy in the same pull request.

A rule without a source is closed, however sensible it sounds. That is the only
thing separating this project from another opinionated linter.

## Mascot

Plumby. The friendly name belongs on the mascot, not on the command.

## License

Apache-2.0. See [LICENSE](LICENSE).
