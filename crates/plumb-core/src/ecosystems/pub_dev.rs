// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! pub.dev (Dart/Flutter): `pubspec.yaml`.

use super::EcosystemSpec;
use crate::manifest::{Ecosystem, Gap, Manifest};

/// pub.dev: `pubspec.yaml`.
pub struct PubDev;

impl EcosystemSpec for PubDev {
    fn ecosystem(&self) -> Ecosystem {
        todo!()
    }

    fn matches(&self, _filename: &str) -> bool {
        todo!()
    }

    fn parse(&self, _filename: &str, _text: &str) -> Manifest {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // matches

    #[test]
    fn pubspec_yaml_is_recognised_case_insensitively() {
        assert!(PubDev.matches("pubspec.yaml"));
        assert!(PubDev.matches("PUBSPEC.YAML"));
        assert!(!PubDev.matches("pubspec.yml"));
        assert!(!PubDev.matches("pubspec.lock"));
    }

    // parse: scalar and null values

    #[test]
    fn scalar_versions_yield_names() {
        let text = "dependencies:\n  dio: ^5.9.2\n  cupertino_icons: ^1.0.8\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert_eq!(manifest.names, set(&["dio", "cupertino_icons"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn a_null_valued_dependency_is_a_name() {
        let text = "dependencies:\n  some_pkg:\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert_eq!(manifest.names, set(&["some_pkg"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // parse: never-looked-up shapes

    #[test]
    fn sdk_dependencies_are_never_looked_up() {
        let text = "dependencies:\n  flutter:\n    sdk: flutter\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn git_and_path_dependencies_are_never_looked_up() {
        let text = "dependencies:\n\
                     \x20 from_git:\n\
                     \x20   git: https://example.com/from_git.git\n\
                     \x20 from_path:\n\
                     \x20   path: ../from_path\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // parse: hosted

    #[test]
    fn a_custom_hosted_dependency_is_a_gap() {
        let text = "dependencies:\n\
                     \x20 private_pkg:\n\
                     \x20   hosted:\n\
                     \x20     name: private_pkg\n\
                     \x20     url: https://private.host\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert_eq!(
            manifest.gaps,
            vec![Gap::HostedElsewhere("private_pkg".to_string())]
        );
    }

    #[test]
    fn a_version_only_mapping_is_a_name() {
        let text = "dependencies:\n  dio:\n    version: ^5.9.2\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert_eq!(manifest.names, set(&["dio"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // parse: the other two sections

    #[test]
    fn dev_dependencies_are_read() {
        let text = "dev_dependencies:\n  mocktail: ^1.0.0\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert_eq!(manifest.names, set(&["mocktail"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn dependency_overrides_are_read() {
        let text = "dependency_overrides:\n  pinned_pkg: ^2.0.0\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert_eq!(manifest.names, set(&["pinned_pkg"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn the_flutter_section_is_not_dependencies() {
        let text = "flutter:\n  assets:\n    - images/logo.png\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn comments_are_invisible() {
        let text = "dependencies:\n  # fake_pkg: ^1.0.0\n  dio: ^5.9.2\n";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert_eq!(manifest.names, set(&["dio"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // parse: malformed and empty

    #[test]
    fn malformed_yaml_is_unparseable() {
        let text = "dependencies:\n  bad: [unclosed";
        let manifest = PubDev.parse("pubspec.yaml", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert_eq!(manifest.gaps, vec![Gap::Unparseable]);
    }

    #[test]
    fn an_empty_pubspec_yields_an_empty_manifest() {
        let manifest = PubDev.parse("pubspec.yaml", "");
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);

        let manifest = PubDev.parse("pubspec.yaml", "name: x\n");
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // ecosystem stamp

    #[test]
    fn parse_stamps_pub() {
        let probes: [&str; 12] = [
            "dependencies:\n  dio: ^5.9.2\n",
            "dependencies:\n  some_pkg:\n",
            "dependencies:\n  flutter:\n    sdk: flutter\n",
            "dependencies:\n  from_git:\n    git: https://example.com/from_git.git\n",
            "dependencies:\n  private_pkg:\n    hosted:\n      url: https://private.host\n",
            "dependencies:\n  dio:\n    version: ^5.9.2\n",
            "dev_dependencies:\n  mocktail: ^1.0.0\n",
            "dependency_overrides:\n  pinned_pkg: ^2.0.0\n",
            "flutter:\n  assets:\n    - images/logo.png\n",
            "dependencies:\n  # fake_pkg: ^1.0.0\n  dio: ^5.9.2\n",
            "",
            "name: x\n",
        ];
        for text in probes {
            assert_eq!(
                PubDev.parse("pubspec.yaml", text).ecosystem,
                Ecosystem::Pub,
                "{text}"
            );
        }
    }
}
