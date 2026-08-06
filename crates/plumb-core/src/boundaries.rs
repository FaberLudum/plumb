// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use crate::architecture::Architecture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Repository-relative path of the importing file.
    pub file: String,
    /// The module the file belongs to (heading as declared).
    pub from_module: String,
    /// The module the import targets (heading as declared).
    pub to_module: String,
    /// The import name as written in the source.
    pub import: String,
}

/// Top-level names imported by `text`, per the extension of `path`.
pub fn imports(path: &str, text: &str) -> Vec<String> {
    todo!()
}

/// Boundary violations committed by one file's imports.
pub fn violations(architecture: &Architecture, path: &str, text: &str) -> Vec<Violation> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture;

    #[test]
    fn rust_use_lines_yield_the_crate_name() {
        let text = "use plumb_core::manifest;\npub use serde::Serialize;\nuse std::path::Path;\n";
        assert_eq!(
            imports("crates/x/src/main.rs", text),
            vec!["plumb_core", "serde", "std"]
        );
    }

    #[test]
    fn rust_crate_self_and_super_are_not_imports() {
        let text = "use crate::git;\nuse self::inner;\nuse super::outer;\n";
        assert!(imports("crates/x/src/main.rs", text).is_empty());
    }

    #[test]
    fn python_imports_yield_the_top_name() {
        let text = "import requests\nfrom flask import Flask\nfrom . import sibling\nimport os.path\n";
        assert_eq!(
            imports("api/app.py", text),
            vec!["requests", "flask", "os"]
        );
    }

    #[test]
    fn javascript_imports_yield_the_first_segment_and_skip_relative() {
        let text = "import x from 'react-dom/client';\nconst y = require(\"lodash\");\nimport z from './local';\nimport s from '@scope/pkg/deep';\n";
        assert_eq!(
            imports("web/index.ts", text),
            vec!["react-dom", "lodash", "@scope/pkg"]
        );
    }

    #[test]
    fn an_unknown_extension_has_no_imports() {
        assert!(imports("README.md", "use x::y;\n").is_empty());
    }

    fn two_crates() -> architecture::Architecture {
        architecture::parse(
            "## crates/plumb-core\n\nMay depend on: (none).\n\n## crates/plumb-cli\n\nMay depend on: plumb-core.\n",
        )
    }

    #[test]
    fn a_permitted_import_is_not_a_violation() {
        let found = violations(
            &two_crates(),
            "crates/plumb-cli/src/main.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_undeclared_import_is_a_violation_naming_both_modules() {
        let found = violations(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use plumb_cli::check;\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].from_module, "crates/plumb-core");
        assert_eq!(found[0].to_module, "crates/plumb-cli");
        assert_eq!(found[0].import, "plumb_cli");
        assert_eq!(found[0].file, "crates/plumb-core/src/lib.rs");
    }

    #[test]
    fn imports_of_foreign_crates_are_ignored() {
        let found = violations(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use serde::Serialize;\nuse std::fmt;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_file_under_no_module_is_skipped() {
        let found = violations(&two_crates(), "tools/gen.rs", "use plumb_cli::check;\n");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn attribution_is_component_wise_not_string_prefix() {
        let architecture = architecture::parse(
            "## crates/plumb-core\n\nMay depend on: (none).\n\n## crates/plumb-cli\n\nMay depend on: (none).\n",
        );
        let found = violations(
            &architecture,
            "crates/plumb-core-extras/src/lib.rs",
            "use plumb_cli::check;\n",
        );
        assert!(
            found.is_empty(),
            "a hyphen sibling was attributed by string prefix: {found:?}"
        );
    }

    #[test]
    fn a_self_import_is_allowed() {
        let found = violations(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
