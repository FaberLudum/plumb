// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! The ecosystem table: one file per package ecosystem, one registration
//! line here. Adding an ecosystem = implement `EcosystemSpec` in a new file,
//! add it to `ALL`.

pub(crate) mod npm;
pub(crate) mod nuget;
pub(crate) mod pypi;

/// One package ecosystem's manifest recognition and parsing.
pub trait EcosystemSpec {
    /// The registry this ecosystem's names resolve against.
    fn ecosystem(&self) -> crate::manifest::Ecosystem;
    /// Whether `filename` (final path segment, as-is) is a manifest of this
    /// ecosystem.
    fn matches(&self, filename: &str) -> bool;
    /// Parses a manifest already known (via [`matches`](Self::matches)) to
    /// belong to this ecosystem. `filename` is the same final path segment
    /// `matches` saw — an ecosystem with more than one manifest syntax needs
    /// it to tell them apart, since a malformed instance of one syntax can
    /// parse as a valid (if useless) instance of another when read from
    /// `text` alone.
    fn parse(&self, filename: &str, text: &str) -> crate::manifest::Manifest;
}

static ALL: [&(dyn EcosystemSpec + Sync); 2] = [&pypi::PyPi, &npm::Npm];

pub fn specs() -> &'static [&'static (dyn EcosystemSpec + Sync)] {
    &ALL
}

/// The spec claiming `path`'s manifest, if any.
///
/// Extracts the final path segment — the same split `manifest::parse`
/// performs on a path — before asking any spec whether it matches, so a
/// spec's `matches` only ever sees a bare filename and never a
/// directory-qualified path.
pub fn for_path(path: &str) -> Option<&'static (dyn EcosystemSpec + Sync)> {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    specs().iter().copied().find(|spec| spec.matches(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBES: [&str; 4] = [
        "requirements.txt",
        "requirements-dev.txt",
        "pyproject.toml",
        "package.json",
    ];

    #[test]
    fn no_probe_is_matched_by_two_specs() {
        for probe in PROBES {
            let matched = specs().iter().filter(|spec| spec.matches(probe)).count();
            assert!(matched <= 1, "{probe} matched by {matched} specs");
        }
    }

    #[test]
    fn every_spec_matches_at_least_one_probe() {
        for spec in specs() {
            assert!(
                PROBES.iter().any(|probe| spec.matches(probe)),
                "a spec matched none of the known probes"
            );
        }
    }

    /// A spec's `parse` must stamp the same `Ecosystem` its own `ecosystem()`
    /// declares. Nothing in the trait enforces this — `parse` builds a
    /// `Manifest` by hand and `ecosystem()` is a separate method — so a new
    /// spec (NuGet is queued) could set them inconsistently with no other
    /// test failing, and the first caller to route on `ecosystem()` (a
    /// tabled `RegistryRouter`) would then send lookups to the wrong
    /// registry.
    #[test]
    fn parse_stamps_the_ecosystem_the_spec_declares() {
        for spec in specs() {
            let probe = PROBES
                .iter()
                .find(|probe| spec.matches(probe))
                .unwrap_or_else(|| panic!("a spec matched none of the known probes"));
            let manifest = spec.parse(probe, "");
            assert_eq!(
                manifest.ecosystem,
                spec.ecosystem(),
                "{:?}: parse stamped a different ecosystem than ecosystem() declares",
                spec.ecosystem()
            );
        }
    }

    /// `for_path` must extract the final path segment before asking a spec
    /// to match, the same lesson `languages::mod`'s
    /// `for_extension_answers_for_every_declared_extension` pins for
    /// `extension_of`/`for_extension`: probing the dispatcher directly with a
    /// bare filename would pass even if `for_path` forgot the split and
    /// handed a spec the whole path, since none of the fixtures below has a
    /// directory component. Probing with a nested path closes that hole —
    /// `PyPi::matches` checks the filename by equality/prefix, and neither
    /// test holds against a path that still carries its directory.
    #[test]
    fn for_path_extracts_the_final_path_segment_before_matching() {
        assert!(for_path("a/b/pyproject.toml").is_some());
        assert!(for_path("packages/api/requirements.txt").is_some());
        // Pins the npm path through the same extraction: vacuous before the
        // ecosystem table carried an npm spec, since a directory-qualified
        // `package.json` matched nothing at all either way — see this
        // module's history.
        assert!(for_path("a/b/package.json").is_some());
    }
}
