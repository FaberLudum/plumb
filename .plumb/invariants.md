# Invariants — Plumb

Project-specific non-negotiables, one per heading, written as they emerge.
Empty is honest; aspirational is not.

## The one rule

Every rule Plumb enforces must trace back to a row in
[docs/research/taxonomy.md](docs/research/taxonomy.md). No source, no
rule. This applies to the maintainers, not only to contributors — a check
that seems sensible but has no row does not get added, because that is the
only thing separating this project from another opinionated linter.

Taxonomy §5 lists defects Plumb must *not* fight: function length,
cyclomatic complexity, readability thresholds. The evidence says AI already
matches or beats humans on those; do not reintroduce them.

## Tiers are evidence-graded, not severity-graded

| Tier | Enforcement | Evidence required |
|---|---|---|
| A | Blocks the build | Peer-reviewed, objective detection |
| B | Warns | Preprint, or thresholds needing calibration |
| C | Measures only | None — instrumentation for open hypotheses |
| Hygiene | Autofixed, excluded from defect counts | Style consistency, delegated to formatters |

Proposing a Tier A rule on a preprint is a rejection. Propose it as Tier B.

## Exit codes are a fixed contract

`0` clean, `1` a Tier A (blocking) violation, `2` warnings only. A run that
could not happen at all — not a repository, no git — exits outside that
contract, so a broken gate can never be read as a clean one.

## No test may contact the network

`plumb-core` has no network access by construction. `plumb-cli`'s registry
lookups are the only outbound calls Plumb makes, and the test suite never
exercises them against the real network — tests stub or fake the registry
boundary instead.
