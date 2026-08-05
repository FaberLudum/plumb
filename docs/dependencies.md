# Third-party dependencies

Plumb's own Tier A rule applies to Plumb: every external crate needs a reason
the standard library is not enough.

| Crate | Why not std |
|---|---|
| `anyhow` | `plumb-cli` only. Error context chains across module boundaries. std has no equivalent for propagating human-readable context. `plumb-core` is pure and fallible in no way that needs context, so it does not declare this. |
| `clap` | Argument parsing with subcommands, help text and version. Hand-rolling this is a known source of bugs and no faster to write. |
| `serde` / `serde_json` | JSON report output and cache persistence. No serialisation in std. |
| `toml` | `pyproject.toml` parsing. No TOML parser in std. |
| `ureq` | Blocking HTTP for registry lookups. No HTTP client in std. Chosen over `reqwest` because it is blocking, has no async runtime, and pulls a far smaller tree. |
| `assert_cmd`, `predicates`, `tempfile` | Test-only. Integration testing of a binary and temporary git repositories. |

## Lockfile policy

`Cargo.lock` is committed to the repository and tracks the exact
dependency versions the workspace builds against. No version is
pinned by hand: the lockfile is always the output of a plain `cargo
generate-lockfile` (or an ordinary `cargo update`) against the
ranges in `Cargo.toml`, never edited or hand-adjusted to work around
a build failure. Local builds and CI both run with `--locked`, so a
build fails loudly if the lockfile and manifests ever drift apart
instead of silently re-resolving.
