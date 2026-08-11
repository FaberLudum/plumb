// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! pub.dev (Dart/Flutter): `pubspec.yaml`.

use super::EcosystemSpec;
use crate::manifest::{Ecosystem, Gap, Manifest};
use std::collections::BTreeSet;
use yaml_rust2::{Yaml, YamlLoader};

/// The three pubspec tables this module reads, in the order pub itself
/// documents them.
const DEPENDENCY_SECTIONS: [&str; 3] = ["dependencies", "dev_dependencies", "dependency_overrides"];

/// Whether `url` names pub.dev itself rather than a private registry.
///
/// Both the scheme-qualified and bare spellings are real, writable syntax —
/// `pub` accepts `hosted: pub.dev` exactly as it accepts
/// `hosted: https://pub.dev` — and `pub.dartlang.org` is the retired name
/// for the same registry, still honoured by the tool today. Compared
/// case-insensitively with any trailing slash trimmed, neither of which
/// changes which registry the URL names.
fn is_default_host(url: &str) -> bool {
    matches!(
        url.trim()
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .as_str(),
        "pub.dev" | "https://pub.dev" | "pub.dartlang.org" | "https://pub.dartlang.org"
    )
}

/// Reads one entry of a dependency table: `name` is the key, `value` is
/// whatever it maps to. Pushes a name or a gap, per the rule the module doc
/// comment states — the value's shape, not its presence, decides whether a
/// dependency is a registry lookup at all.
fn read_entry(
    section: &str,
    name: &str,
    value: &Yaml,
    names: &mut BTreeSet<String>,
    gaps: &mut Vec<Gap>,
) {
    // A scalar version constraint, or no value at all (pub allows
    // `some_pkg:` with nothing after the colon, meaning any version) — both
    // are registry-hosted.
    if value.is_null() || value.as_str().is_some() {
        names.insert(name.to_string());
        return;
    }

    let Some(map) = value.as_hash() else {
        // Not a scalar, not null, not a mapping either (an array, say) — a
        // declaration the reader cannot use.
        gaps.push(Gap::Malformed(format!("{section}.{name}")));
        return;
    };

    let has_key = |key: &str| map.contains_key(&Yaml::String(key.to_string()));

    // `sdk:`, `git:` and `path:` dependencies are never registry lookups,
    // and never a gap either — Flutter itself, a git checkout and a local
    // package are all correctly read as "not on pub.dev", not as something
    // this reader missed.
    if has_key("sdk") || has_key("git") || has_key("path") {
        return;
    }

    if let Some(hosted) = map.get(&Yaml::String("hosted".to_string())) {
        // `hosted:` takes either a bare URL scalar or a mapping with a
        // `url` key naming one — both real, documented pub syntax.
        let url = match hosted {
            Yaml::String(url) => Some(url.as_str()),
            Yaml::Hash(inner) => inner
                .get(&Yaml::String("url".to_string()))
                .and_then(Yaml::as_str),
            _ => None,
        };
        match url {
            Some(url) if is_default_host(url) => {
                names.insert(name.to_string());
            }
            Some(_) => gaps.push(Gap::HostedElsewhere(name.to_string())),
            None => gaps.push(Gap::Malformed(format!("{section}.{name}"))),
        }
        return;
    }

    // Everything left is a mapping without any of the special keys above.
    // A `version`-only mapping (or an empty one) is still registry-hosted;
    // any other key names a shape this reader does not recognise.
    let recognised = map.keys().all(|key| key.as_str() == Some("version"));
    if recognised {
        names.insert(name.to_string());
    } else {
        gaps.push(Gap::Malformed(format!("{section}.{name}")));
    }
}

/// Parses a `pubspec.yaml`: the `dependencies`, `dev_dependencies` and
/// `dependency_overrides` tables of the top-level mapping.
///
/// `YamlLoader::load_from_str` returns one `Yaml` per `---`-separated
/// document; a pubspec is always a single document, so only the first is
/// read. A load error discards any names a partial read might otherwise
/// have produced — the same rule `ecosystems::nuget` takes for a `.csproj`
/// left with an element open at end of input — because a file that cannot
/// be fully read must not present as partially clean.
fn parse_pubspec(text: &str) -> Manifest {
    let Ok(documents) = YamlLoader::load_from_str(text) else {
        return Manifest {
            names: BTreeSet::new(),
            gaps: vec![Gap::Unparseable],
            ecosystem: Ecosystem::Pub,
        };
    };

    let mut names = BTreeSet::new();
    let mut gaps = Vec::new();

    // An empty body, or a document with nothing but scalars at the top
    // level, has no dependency tables to read — the same non-signal an
    // absent section is, not a malformed one.
    if let Some(root) = documents.first().and_then(Yaml::as_hash) {
        for section in DEPENDENCY_SECTIONS {
            let Some(value) = root.get(&Yaml::String(section.to_string())) else {
                continue;
            };
            match value.as_hash() {
                Some(entries) => {
                    for (key, entry) in entries {
                        let Some(name) = key.as_str() else {
                            continue;
                        };
                        read_entry(section, name, entry, &mut names, &mut gaps);
                    }
                }
                None => gaps.push(Gap::Malformed(section.to_string())),
            }
        }
    }

    Manifest {
        names,
        gaps,
        ecosystem: Ecosystem::Pub,
    }
}

/// pub.dev: `pubspec.yaml`.
pub struct PubDev;

impl EcosystemSpec for PubDev {
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Pub
    }

    fn matches(&self, filename: &str) -> bool {
        filename.eq_ignore_ascii_case("pubspec.yaml")
    }

    /// pub.dev has one manifest syntax, so `filename` carries nothing
    /// `text` does not already say — the same reasoning `ecosystems::npm`
    /// gives for `package.json`.
    fn parse(&self, _filename: &str, text: &str) -> Manifest {
        parse_pubspec(text)
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
