# Third-party dependencies

Plumb's own Tier A rule applies to Plumb: every external crate needs a reason
the standard library is not enough.

| Crate | Why not std |
|---|---|
| `anyhow` | Error context chains across module boundaries. std has no equivalent for propagating human-readable context. |
| `clap` | Argument parsing with subcommands, help text and version. Hand-rolling this is a known source of bugs and no faster to write. |
| `serde` / `serde_json` | JSON report output and cache persistence. No serialisation in std. |
| `toml` | `pyproject.toml` parsing. No TOML parser in std. |
| `ureq` | Blocking HTTP for registry lookups. No HTTP client in std. Chosen over `reqwest` because it is blocking, has no async runtime, and pulls a far smaller tree. |
| `assert_cmd`, `predicates`, `tempfile` | Test-only. Integration testing of a binary and temporary git repositories. |
