// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! NuGet: `.csproj`/`.fsproj`/`.vbproj`, `Directory.Packages.props`, and
//! `packages.config` (recognised by filename, never parsed).

use super::EcosystemSpec;
use crate::manifest::{Ecosystem, Gap, Manifest};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Decoder, Reader, XmlVersion};
use std::collections::BTreeSet;

/// The element names whose `Include` attribute names a NuGet package.
///
/// `PackageReference` is the ordinary way a project declares one.
/// `PackageVersion` and `GlobalPackageReference` are Central Package
/// Management's counterparts in `Directory.Packages.props` — the former
/// pins a version centrally, the latter declares an analyzer/build-tooling
/// package every project should reference. All three carry the package name
/// the same way, in an `Include` attribute, so one reader serves both files
/// this module parses: a `.csproj` and a `Directory.Packages.props` differ
/// in which of the three they conventionally use, not in the shape.
const PACKAGE_ELEMENTS: [&[u8]; 3] = [
    b"PackageReference",
    b"PackageVersion",
    b"GlobalPackageReference",
];

/// Builds the [`Gap::Unparseable`] manifest, discarding any names already
/// read.
///
/// A file that cannot be fully read must not present as partially clean —
/// the same rule `pypi::parse_pyproject` and `npm::parse_package_json`
/// already take for a document that fails to parse at all. Here it also
/// covers a document left with an element open at end of input, which
/// `quick_xml::Reader` does not treat as an error on its own: a streaming
/// reader has no obligation to demand a complete document, so [`parse_project`]
/// tracks open-element depth itself and calls this when it is not back to
/// zero at [`Event::Eof`].
fn unparseable() -> Manifest {
    Manifest {
        names: BTreeSet::new(),
        gaps: vec![Gap::Unparseable],
        ecosystem: Ecosystem::NuGet,
    }
}

/// Reads the `Include` attribute off a `PackageReference`, `PackageVersion`
/// or `GlobalPackageReference` start/empty tag, and records it: a literal
/// value becomes a name, an MSBuild property expression (`$(...)`) becomes a
/// [`Gap::MsBuildExpression`], and any other element name is left alone —
/// notably `ProjectReference`, which shares the `Include` convention but
/// names a sibling project rather than a NuGet package.
///
/// The value is trimmed before either check. MSBuild itself trims an
/// `Include` before using it — XML attribute-value normalisation only
/// touches line endings, not spaces — so `"  Newtonsoft.Json  "` is real,
/// writable syntax naming an ordinary package, not a different one with
/// whitespace in its name. Trimming ahead of the `$(` check and storing the
/// trimmed text in the gap (rather than trimming only the name path) keeps
/// both outcomes consistent: the leading/trailing whitespace is MSBuild
/// noise either way, not part of the property expression.
fn read_package(
    start: &BytesStart,
    decoder: Decoder,
    names: &mut BTreeSet<String>,
    gaps: &mut Vec<Gap>,
) {
    if !PACKAGE_ELEMENTS.contains(&start.name().local_name().as_ref()) {
        return;
    }
    let Some(include) = start
        .attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == b"Include")
        .and_then(|attr| {
            attr.decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                .ok()
        })
        .map(|value| value.trim().to_string())
    else {
        return;
    };
    // An `Include` that is empty, or only whitespace, names nothing — not a
    // package, and not an unresolved reference the way an MSBuild expression
    // is. Silently skipped rather than inserted as an empty-string name,
    // which would reach Task 2's registry lookup as a literal package name.
    if include.is_empty() {
        return;
    }
    if include.contains("$(") {
        gaps.push(Gap::MsBuildExpression(include));
    } else {
        names.insert(include);
    }
}

/// Parses a `.csproj`/`.fsproj`/`.vbproj` or a `Directory.Packages.props`:
/// the three element names in [`PACKAGE_ELEMENTS`], read wherever they
/// appear as a start or self-closing tag, for their `Include` attribute.
///
/// One code path for both files, because both read the same three elements
/// — a `.csproj` conventionally uses `PackageReference` and a
/// `Directory.Packages.props` conventionally uses `PackageVersion` and
/// `GlobalPackageReference`, but nothing here needs to know which file it is
/// reading to make sense of any of the three.
fn parse_project(text: &str) -> Manifest {
    let mut names = BTreeSet::new();
    let mut gaps = Vec::new();
    // Counts elements opened by a `Start` and not yet closed by a matching
    // `End`. Self-closing (`Empty`) tags never touch it, since they open and
    // close in a single event.
    let mut depth: u32 = 0;

    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => {
                if depth != 0 {
                    return unparseable();
                }
                break;
            }
            Ok(Event::Start(start)) => {
                depth += 1;
                read_package(&start, reader.decoder(), &mut names, &mut gaps);
            }
            Ok(Event::Empty(start)) => {
                read_package(&start, reader.decoder(), &mut names, &mut gaps);
            }
            Ok(Event::End(_)) => {
                depth -= 1;
            }
            Ok(_) => {}
            Err(_) => return unparseable(),
        }
        buf.clear();
    }

    Manifest {
        names,
        gaps,
        ecosystem: Ecosystem::NuGet,
    }
}

/// NuGet: `.csproj`/`.fsproj`/`.vbproj`, `Directory.Packages.props`, and
/// `packages.config`.
pub struct NuGet;

impl EcosystemSpec for NuGet {
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::NuGet
    }

    /// Windows filesystems are case-insensitive, so a project file named
    /// `Api.CSPROJ` is exactly as real as one named `Api.csproj`; the
    /// comparison is done on a lowercased copy rather than by slicing a
    /// fixed-length suffix off the end, which would risk landing off a
    /// UTF-8 character boundary on a filename with a multi-byte character
    /// near the end.
    fn matches(&self, filename: &str) -> bool {
        let lower = filename.to_ascii_lowercase();
        lower.ends_with(".csproj")
            || lower.ends_with(".fsproj")
            || lower.ends_with(".vbproj")
            || lower == "directory.packages.props"
            || lower == "packages.config"
    }

    /// `packages.config` is recognised only to a [`Gap::PackagesConfig`],
    /// never parsed — not even when it is well-formed and holds what looks
    /// like a package declaration. Every other filename `matches` admits is
    /// a `.csproj`/`.fsproj`/`.vbproj` or a `Directory.Packages.props`, and
    /// all four route to [`parse_project`], which reads the same three
    /// elements regardless of which of them it is given.
    fn parse(&self, filename: &str, text: &str) -> Manifest {
        if filename.eq_ignore_ascii_case("packages.config") {
            return Manifest {
                names: BTreeSet::new(),
                gaps: vec![Gap::PackagesConfig],
                ecosystem: Ecosystem::NuGet,
            };
        }
        parse_project(text)
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
    fn csproj_is_recognised_case_insensitively() {
        assert!(NuGet.matches("Api.csproj"));
        assert!(NuGet.matches("API.CSPROJ"));
        assert!(!NuGet.matches("csproj"));
        assert!(!NuGet.matches("x.csproj.bak"));
    }

    #[test]
    fn fsproj_and_vbproj_are_recognised_too() {
        assert!(NuGet.matches("App.fsproj"));
        assert!(NuGet.matches("Lib.vbproj"));
    }

    #[test]
    fn directory_packages_props_is_recognised() {
        assert!(NuGet.matches("Directory.Packages.props"));
        assert!(NuGet.matches("directory.packages.props"));
        assert!(!NuGet.matches("Packages.props"));
    }

    #[test]
    fn packages_config_is_recognised() {
        assert!(NuGet.matches("packages.config"));
        assert!(NuGet.matches("Packages.Config"));
    }

    // parse: .csproj

    #[test]
    fn package_references_yield_their_include_names() {
        let text = r#"<Project><ItemGroup>
            <PackageReference Include="Ruff" Version="1.0" />
            <PackageReference Include="Httpx" Version="2.0" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert_eq!(manifest.names, set(&["Httpx", "Ruff"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn element_form_package_reference_is_read() {
        let text = r#"<Project><ItemGroup>
            <PackageReference Include="X" Version="1.0">
                <PrivateAssets>all</PrivateAssets>
            </PackageReference>
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert_eq!(manifest.names, set(&["X"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn project_reference_is_not_a_package() {
        let text = r#"<Project><ItemGroup>
            <ProjectReference Include="..\..\x.csproj" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn a_commented_out_reference_is_invisible() {
        let text = r#"<Project><ItemGroup>
            <!-- <PackageReference Include="Fake" /> -->
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn a_bom_does_not_break_parsing() {
        // The driving repo's real test csproj opens with a UTF-8 BOM.
        let text = "\u{feff}<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup>\
                     <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.3\" />\
                     </ItemGroup></Project>";
        let manifest = NuGet.parse("Api.csproj", text);
        assert_eq!(manifest.names, set(&["Newtonsoft.Json"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// MSBuild itself trims an `Include` value before using it — XML
    /// attribute-value normalisation only touches line endings, not spaces,
    /// so this is real, writable syntax and not an edge case invented for
    /// the test. Without trimming, `"  Newtonsoft.Json  "` would flow into
    /// the registry lookup verbatim and produce a spurious Tier A block on a
    /// project that restores fine.
    #[test]
    fn an_include_with_surrounding_whitespace_is_trimmed() {
        let text = r#"<Project><ItemGroup>
            <PackageReference Include="  Newtonsoft.Json  " Version="13.0.1" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert_eq!(manifest.names, set(&["Newtonsoft.Json"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// An `Include` that is empty, or trims to empty, must not surface as an
    /// empty-string package name — that would reach Task 2's registry
    /// lookup as a literal name and could only ever come back `Missing`.
    /// Silently skipped, the same as a `PackageReference` with no `Include`
    /// attribute at all, rather than raising a gap: there is no unresolved
    /// reference here to point a reader at, unlike an MSBuild expression.
    #[test]
    fn an_include_that_trims_to_empty_yields_no_name() {
        let text = r#"<Project><ItemGroup>
            <PackageReference Include="   " Version="1.0" />
            <PackageReference Include="" Version="1.0" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn an_msbuild_expression_is_a_gap_not_a_name() {
        let text = r#"<Project><ItemGroup>
            <PackageReference Include="$(SomeProp)" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert!(manifest.names.is_empty());
        assert_eq!(
            manifest.gaps,
            vec![Gap::MsBuildExpression("$(SomeProp)".to_string())]
        );
    }

    /// Truncated: no closing tags at all. `Reader` does not treat an
    /// element left open at end of input as an error on its own — a
    /// streaming parser has no obligation to demand a complete document —
    /// so the module's own depth tracking is what catches it.
    ///
    /// A `PackageReference` already read before the truncation point must
    /// be discarded along with everything else: a file that cannot be
    /// fully read must not present as partially clean.
    #[test]
    fn malformed_xml_is_unparseable() {
        let text = r#"<Project><ItemGroup><PackageReference Include="Ruff" />"#;
        let manifest = NuGet.parse("Api.csproj", text);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
        assert_eq!(manifest.gaps, vec![Gap::Unparseable]);
    }

    #[test]
    fn an_empty_project_yields_an_empty_manifest() {
        let manifest = NuGet.parse("Api.csproj", "<Project></Project>");
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // parse: Directory.Packages.props

    #[test]
    fn package_version_entries_yield_names() {
        let text = r#"<Project><ItemGroup>
            <PackageVersion Include="X" Version="1.0" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Directory.Packages.props", text);
        assert_eq!(manifest.names, set(&["X"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn global_package_reference_yields_names() {
        let text = r#"<Project><ItemGroup>
            <GlobalPackageReference Include="Y" />
        </ItemGroup></Project>"#;
        let manifest = NuGet.parse("Directory.Packages.props", text);
        assert_eq!(manifest.names, set(&["Y"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    // parse: packages.config

    /// `packages.config` is recognised only to a gap, never parsed for
    /// names — even when it is well-formed and holds what looks like a
    /// package declaration. Migrating to PackageReference is what makes it
    /// visible to Plumb, not a smarter reader for the legacy format.
    #[test]
    fn packages_config_is_a_gap_never_names() {
        let text = r#"<packages><package id="Z" version="1.0" /></packages>"#;
        let manifest = NuGet.parse("packages.config", text);
        assert!(manifest.names.is_empty());
        assert_eq!(manifest.gaps, vec![Gap::PackagesConfig]);
    }

    // ecosystem stamp

    #[test]
    fn parse_stamps_nuget() {
        let probes: [(&str, &str); 8] = [
            (
                "Api.csproj",
                r#"<Project><ItemGroup><PackageReference Include="Ruff" /></ItemGroup></Project>"#,
            ),
            ("App.fsproj", "<Project></Project>"),
            ("Lib.vbproj", "<Project></Project>"),
            (
                "Directory.Packages.props",
                r#"<Project><ItemGroup><PackageVersion Include="X" Version="1.0" /></ItemGroup></Project>"#,
            ),
            ("packages.config", "<packages></packages>"),
            (
                "Api.csproj",
                r#"<Project><ItemGroup><PackageReference Include="$(SomeProp)" /></ItemGroup></Project>"#,
            ),
            ("Api.csproj", "<Project><ItemGroup>"),
            ("Api.csproj", "<Project></Project>"),
        ];
        for (filename, text) in probes {
            assert_eq!(
                NuGet.parse(filename, text).ecosystem,
                Ecosystem::NuGet,
                "{filename}"
            );
        }
    }
}
