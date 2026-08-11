// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! NuGet: `.csproj`/`.fsproj`/`.vbproj`, `Directory.Packages.props`, and
//! `packages.config` (recognised by filename, never parsed).

use super::EcosystemSpec;
use crate::manifest::{Ecosystem, Gap, Manifest};

/// NuGet: `.csproj`/`.fsproj`/`.vbproj`, `Directory.Packages.props`, and
/// `packages.config`.
pub struct NuGet;

impl EcosystemSpec for NuGet {
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
