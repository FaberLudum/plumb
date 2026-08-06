# Architecture — Plumb

Modules and the dependencies they are permitted. An import that crosses a
boundary not declared here is a finding, not a style choice.

## crates/plumb-core

Responsibility: pure logic — parsing manifests, evaluating rules, rendering
Layer 1 artefacts. No network, no filesystem, no subprocess calls. This
boundary is load-bearing: anything that touches the outside world belongs in
`plumb-cli`, behind a trait so tests can stub it. Tests in this crate must
never contact the network.

May depend on: (none) — plumb-core has no internal dependencies.

## crates/plumb-cli

Responsibility: binary entry point — argument parsing, git invocation,
registry lookups, and the command implementations (`check`, `init`,
`adapters`, and future commands) that wire plumb-core's pure logic to the
outside world.

May depend on: plumb-core.

## docs/research/

Responsibility: the evidence base. `taxonomy.md` is the row-per-rule ledger
every enforced rule must trace back to (see .plumb/invariants.md, "the one
rule"); `sources.md` backs each row. Not code, and nothing in the crates imports it,
but every check the crates run and every rule in this working agreement
traces back to a row here.

May depend on: (none).

## docs/superpowers/

Responsibility: specs and implementation plans — the design record for how a
feature came to be, kept separate from the taxonomy so evidence and design
decisions do not get mixed together.

May depend on: (none).
