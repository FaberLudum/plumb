// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! The ecosystem table: one file per package ecosystem, one registration
//! line here. Adding an ecosystem = implement `EcosystemSpec` in a new file,
//! add it to `ALL`.

mod pypi;

/// One package ecosystem's manifest recognition and parsing.
pub trait EcosystemSpec {
    /// The registry this ecosystem's names resolve against.
    fn ecosystem(&self) -> crate::manifest::Ecosystem;
    /// Whether `filename` (final path segment, as-is) is a manifest of this
    /// ecosystem.
    fn matches(&self, filename: &str) -> bool;
    fn parse(&self, text: &str) -> crate::manifest::Manifest;
}

static ALL: [&(dyn EcosystemSpec + Sync); 1] = [&pypi::PyPi];

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
    }
}
