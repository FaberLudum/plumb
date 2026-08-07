// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use crate::architecture::{Architecture, Module};

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
///
/// This is a documented heuristic, not a parser: extraction is line-based,
/// one `text.lines()` pass, matching a small set of surface patterns per
/// language family. It misses imports that span multiple lines (a
/// parenthesised Python `from x import (a, b)` split across lines, a
/// multi-line `use` group in Rust) and imports produced by macro expansion
/// (a `use` a Rust macro generates is never a literal source line, so there
/// is nothing for this pass to match), and it does not know about strings or
/// comments, so an import-shaped line inside either can false-positive.
/// That trade is deliberate: it keeps this crate free of a parser
/// dependency per language, and boundary findings are Tier B precisely
/// because of heuristics like this one.
pub fn imports(path: &str, text: &str) -> Vec<String> {
    match extension(path).as_str() {
        "rs" => rust_imports(text),
        "py" => python_imports(text),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => javascript_imports(text),
        _ => Vec::new(),
    }
}

/// Boundary violations committed by one file's imports.
///
/// `path` is attributed to a module by longest path-prefix match; a file
/// under no declared module is skipped, not flagged. Each extracted import
/// is resolved to a target module by name; an import matching no module
/// (`std`, `serde`, third-party packages, …) is not ours to police and is
/// ignored. An import targeting the file's own module is always allowed.
/// Everything else must be named in the file's module's `May depend on:`
/// list, or it is a violation.
pub fn violations(architecture: &Architecture, path: &str, text: &str) -> Vec<Violation> {
    let Some(from) = module_of(architecture, path) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for import in imports(path, text) {
        let Some(target) = target_of(architecture, &import) else {
            continue;
        };
        if target.path == from.path {
            continue;
        }
        if from
            .may_depend_on
            .iter()
            .any(|entry| permits(entry, target))
        {
            continue;
        }
        found.push(Violation {
            file: path.to_string(),
            from_module: from.path.clone(),
            to_module: target.path.clone(),
            import,
        });
    }
    found
}

/// The distinct cross-module `(from_module, to_module)` pairs one file's
/// imports resolve to.
///
/// `violations` without the permission check: every pair this finds is a
/// coupling edge whether or not the architecture's `May depend on:` list
/// allows it — a caller measuring coupling (`plumb report`) cares about the
/// edge existing, not about whether it was permitted. A file under no
/// declared module contributes nothing, the same as `violations`, and an
/// import targeting the file's own module is never a pair.
///
/// Distinct and in first-occurrence order: a file importing the same
/// external module twice (two `use` lines resolving to the same target)
/// must not count as two edges, and the order a pair first appears in is
/// what stays stable across a re-run of the same text.
pub fn edges(architecture: &Architecture, path: &str, text: &str) -> Vec<(String, String)> {
    let Some(from) = module_of(architecture, path) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = Vec::new();
    for import in imports(path, text) {
        let Some(target) = target_of(architecture, &import) else {
            continue;
        };
        if target.path == from.path {
            continue;
        }
        let pair = (from.path.clone(), target.path.clone());
        if !found.contains(&pair) {
            found.push(pair);
        }
    }
    found
}

/// Whether `path` is worth reading at all for the boundary gate: a known
/// source extension, attributed to a module the architecture declares.
///
/// Path-only — no `text` parameter — on purpose. These are exactly the two
/// filters [`violations`] applies before it can report anything: an
/// unrecognised extension is matched by no arm of [`imports`], and a file
/// under no declared module is skipped by [`violations`] outright before a
/// single import is even extracted. Answering both without a byte of the
/// file's content is what lets a caller decide not to read it — see
/// `plumb-cli`'s `check.rs`, which calls this before `std::fs::read` so a
/// diff touching an unrelated multi-gigabyte binary pays nothing for a scan
/// whose answer was always going to be "no findings".
pub fn is_scannable(architecture: &Architecture, path: &str) -> bool {
    is_known_extension(&extension(path)) && module_of(architecture, path).is_some()
}

/// Whether `extension` (already lowercased by [`extension`]) is one
/// [`imports`] does anything with.
///
/// This list must be kept identical to the extensions matched in
/// [`imports`]'s dispatch. The two are not merged into one because
/// [`imports`] needs `text` to decide *which* extractor to run, while
/// [`is_scannable`] must answer with no `text` at all — but a language added
/// to one list and not the other silently reopens the gap `is_scannable`
/// exists to close: `every_known_extension_is_dispatched_by_imports` pins
/// that they stay in step.
fn is_known_extension(extension: &str) -> bool {
    matches!(
        extension,
        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
    )
}

/// The lowercased extension of `path`'s final path segment, or empty when it
/// has none.
///
/// Two things a caller must not have to know: a path with no `.` in its
/// final segment has no extension — `rsplit('.')` alone answers that with the
/// whole remaining string instead, which a dotless file named exactly `rs`
/// would then match against the `"rs"` arm below — and matching must be
/// case-insensitive, since `LIB.RS` is as much a Rust file as `lib.rs`.
fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// The module owning `path`: longest component-wise prefix match.
fn module_of<'a>(architecture: &'a Architecture, path: &str) -> Option<&'a Module> {
    architecture
        .modules
        .iter()
        .filter(|module| {
            path.strip_prefix(module.path.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|module| module.path.len())
}

/// `-` and `_` are the same letter for matching: a crate named
/// `plumb-core` is imported as `plumb_core`.
fn normalise(name: &str) -> String {
    name.replace('-', "_")
}

/// The module an import name targets: last path segment match.
fn target_of<'a>(architecture: &'a Architecture, import: &str) -> Option<&'a Module> {
    let wanted = normalise(import);
    architecture.modules.iter().find(|module| {
        module
            .path
            .rsplit('/')
            .next()
            .is_some_and(|segment| normalise(segment) == wanted)
    })
}

/// Whether `permitted` (an entry of May-depend-on) names `module`.
fn permits(permitted: &str, module: &Module) -> bool {
    permitted == module.path
        || module
            .path
            .rsplit('/')
            .next()
            .is_some_and(|segment| normalise(segment) == normalise(permitted))
}

/// Stops NAME at `::`, `;`, whitespace or `{` — whichever comes first.
fn rust_name(rest: &str) -> &str {
    let end = rest
        .find(|c: char| c == ':' || c == ';' || c.is_whitespace() || c == '{')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `use NAME::…;` / `use NAME;` (also `pub use`), `extern crate NAME;`.
/// `crate`, `self` and `super` name the current crate, not a dependency.
fn rust_imports(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("pub use ")
            .or_else(|| trimmed.strip_prefix("use "))
            .or_else(|| trimmed.strip_prefix("extern crate "));
        if let Some(rest) = rest {
            let name = rust_name(rest);
            if !name.is_empty() && name != "crate" && name != "self" && name != "super" {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Stops a dotted Python module path at the first `.` or whitespace.
fn python_name(rest: &str) -> &str {
    let end = rest
        .find(|c: char| c == '.' || c.is_whitespace())
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `import NAME[.rest]` and `from NAME[.rest] import …`. `from .` is a
/// relative import and is skipped.
///
/// A bare `import` may name several modules on one line, comma-separated —
/// `import os, sys` — which `from NAME import …` cannot: there the comma
/// separates *imported names* from one module, not modules themselves, so
/// only the single name after `from` is ever a module. Only the `import` arm
/// therefore splits on `,`; the `from` arm keeps taking one name, as before.
fn python_imports(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("from ") {
            if rest.starts_with('.') {
                continue;
            }
            let name = python_name(rest);
            if !name.is_empty() {
                names.push(name.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            for segment in rest.split(',') {
                let name = python_name(segment.trim());
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

/// The quoted spec after the first `from '`/`from "`/`require('`/`require("`
/// found in `line`, if any.
fn javascript_spec(line: &str) -> Option<&str> {
    for marker in ["from '", "from \"", "require('", "require(\""] {
        if let Some(index) = line.find(marker) {
            let quote = marker.chars().last().unwrap_or('\'');
            let after = &line[index + marker.len()..];
            if let Some(end) = after.find(quote) {
                return Some(&after[..end]);
            }
        }
    }
    None
}

/// The module segment(s) a JS/TS import spec names: the first path
/// segment, or the first two joined for a `@scope/name` package. A
/// relative spec (starting with `.`) is skipped.
fn javascript_segment(spec: &str) -> Option<String> {
    if spec.starts_with('.') {
        return None;
    }
    if let Some(scoped) = spec.strip_prefix('@') {
        let mut segments = scoped.splitn(2, '/');
        let scope = segments.next().unwrap_or("");
        let name = segments
            .next()
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("");
        return if name.is_empty() {
            Some(format!("@{scope}"))
        } else {
            Some(format!("@{scope}/{name}"))
        };
    }
    let segment = spec.split('/').next().unwrap_or("");
    if segment.is_empty() {
        None
    } else {
        Some(segment.to_string())
    }
}

fn javascript_imports(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(javascript_spec)
        .filter_map(javascript_segment)
        .collect()
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
        let text =
            "import requests\nfrom flask import Flask\nfrom . import sibling\nimport os.path\n";
        assert_eq!(imports("api/app.py", text), vec!["requests", "flask", "os"]);
    }

    /// `import os, sys` used to extract `os,` — the comma glued onto the
    /// name because the same single-name extraction ran over the whole
    /// comma-joined rest of the line — matching no module and so silently
    /// clearing the boundary check for both names on that line.
    #[test]
    fn a_multi_name_import_yields_every_name() {
        assert_eq!(imports("app.py", "import os, sys\n"), vec!["os", "sys"]);
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

    /// No `.` anywhere in the path used to hand the whole trailing string —
    /// the entire path, since `rsplit('.')` with no match yields the input
    /// unchanged — to the extension match. A file literally named `rs`, with
    /// no extension at all, would then land in the `"rs"` arm by accident.
    #[test]
    fn a_dotless_path_has_no_extension() {
        assert!(imports("rs", "use plumb_core::manifest;\n").is_empty());
        assert!(imports("tools/rs", "use plumb_core::manifest;\n").is_empty());
    }

    /// `LIB.RS` is as much a Rust file as `lib.rs`; matching the extension
    /// case-sensitively silently skipped every uppercase or mixed-case name.
    #[test]
    fn extension_matching_is_case_insensitive() {
        let text = "use plumb_core::manifest;\n";
        assert_eq!(
            imports("crates/x/src/LIB.RS", text),
            vec!["plumb_core"],
            "an uppercase extension was not recognised"
        );
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

    /// The two path-only filters `violations` applies before it can report
    /// anything, exposed so a caller can decide not to read a file's content
    /// at all when neither would ever let it through.
    #[test]
    fn is_scannable_requires_both_a_known_extension_and_a_declared_module() {
        let architecture = two_crates();
        assert!(
            is_scannable(&architecture, "crates/plumb-core/src/lib.rs"),
            "a Rust file under a declared module must be scannable"
        );
        assert!(
            !is_scannable(&architecture, "crates/plumb-core/README.md"),
            "an unrecognised extension must not be scannable, even under a declared module"
        );
        assert!(
            !is_scannable(&architecture, "tools/gen.rs"),
            "a file under no declared module must not be scannable, even with a known extension"
        );
        assert!(
            !is_scannable(&architecture, "tools/README"),
            "neither condition holding must not be scannable"
        );
    }

    /// `is_scannable`'s extension list and `imports`'s dispatch must name the
    /// same languages. This would fail the moment one names a language the
    /// other does not: `is_scannable` returning true for an extension
    /// `imports` does nothing with would let a scannable file through with
    /// no findings ever possible, and the reverse would skip reading a file
    /// `imports` could actually have extracted something from.
    /// `edges` is `violations` without the permission check: every
    /// cross-module pair a file's imports resolve to, distinct, regardless
    /// of whether the architecture's `May depend on:` list allows it.
    /// Order is first-occurrence: the second `plumb_cli` import contributes
    /// no second pair, but does not reorder the one already found.
    #[test]
    fn edges_reports_distinct_cross_module_pairs_in_first_occurrence_order() {
        let found = edges(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use plumb_cli::check;\nuse plumb_cli::git;\nuse serde::Serialize;\n",
        );
        assert_eq!(
            found,
            vec![(
                "crates/plumb-core".to_string(),
                "crates/plumb-cli".to_string()
            )]
        );
    }

    #[test]
    fn edges_is_empty_for_a_file_under_no_module() {
        assert!(edges(&two_crates(), "tools/gen.rs", "use plumb_cli::check;\n").is_empty());
    }

    #[test]
    fn edges_skips_self_imports() {
        let found = edges(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn every_known_extension_is_dispatched_by_imports() {
        for ext in ["rs", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs"] {
            let path = format!("x.{ext}");
            assert!(
                is_known_extension(ext),
                "{ext} is dispatched by imports but not recognised by is_known_extension"
            );
            // A line every one of these extractors recognises as an import,
            // so a wired-up extractor never returns empty here by coincidence.
            let text = "import x\nuse x\nfrom 'x'\nrequire('x')\n";
            assert!(
                !imports(&path, text).is_empty(),
                "{ext} is recognised by is_known_extension but imports() extracted nothing for it"
            );
        }
    }
}
