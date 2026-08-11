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

/// The import paths of `text`, as written, per the extension of `path`.
///
/// Full paths, not top-level names: `use plumb_core::manifest;` yields
/// `plumb_core::manifest`, not `plumb_core`, and `from pkg.facts import
/// totals` yields `pkg.facts`, not `pkg`. Resolving a path to a module is
/// `target_of`'s job, done by matching segments rather than by truncating
/// here.
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
        "rs" => crate::languages::for_extension("rs")
            .map(|language| language.imports(text))
            .unwrap_or_default(),
        "py" => crate::languages::for_extension("py")
            .map(|language| language.imports(text))
            .unwrap_or_default(),
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
            .any(|entry| permits(entry, target, architecture))
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

/// The module an import name targets: longest trailing-module /
/// leading-import segment match.
///
/// An import splits on `::`, `.` and `/` into segments `I`; a module path
/// splits on `/` into segments `M`, both normalised (`-` → `_`). A module
/// matches with score `k` when its last `k` segments equal `I`'s first `k`
/// segments; the import resolves to the module with the highest `k` over
/// all declared modules. A score of 1 is exactly the old last-segment rule,
/// so every previously-resolving import still resolves the same way — a
/// sub-package heading simply wins now by scoring higher when the import
/// names it more specifically (`pkg.facts` scores 2 against `src/pkg/facts`
/// and only 1 against `src/pkg`, so the deeper module wins).
///
/// Ties go to the first-declared module: `Iterator::max_by_key` keeps the
/// *last* maximum on a tie, which is the wrong direction here, so this
/// walks the modules in declaration order and only replaces the current
/// best on a strictly higher score.
fn target_of<'a>(architecture: &'a Architecture, import: &str) -> Option<&'a Module> {
    let import_segments: Vec<String> = import
        .split(['.', '/', ':'])
        .filter(|segment| !segment.is_empty())
        .map(normalise)
        .collect();
    if import_segments.is_empty() {
        return None;
    }
    let mut best: Option<(&Module, usize)> = None;
    for module in &architecture.modules {
        let module_segments: Vec<String> = module.path.split('/').map(normalise).collect();
        let max_k = import_segments.len().min(module_segments.len());
        let score = (1..=max_k)
            .rev()
            .find(|&k| module_segments[module_segments.len() - k..] == import_segments[..k]);
        let Some(score) = score else { continue };
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((module, score));
        }
    }
    best.map(|(module, _)| module)
}

/// Whether `permitted` (an entry of May-depend-on) covers `target`: either
/// the entry names `target` itself, or it names a module that is a
/// path-ancestor of `target` — permitting a parent implicitly permits its
/// whole subtree, so a deep import does not need every intermediate
/// sub-module listed by name.
///
/// An entry that resolves to no declared module (a foreign package, a typo)
/// falls back to the direct name-match against `target` alone, which is the
/// pre-existing rule and the only thing such an entry could ever mean.
fn permits(permitted: &str, target: &Module, architecture: &Architecture) -> bool {
    if names_module(permitted, target) {
        return true;
    }
    resolve_module(permitted, architecture).is_some_and(|entry_module| {
        target.path == entry_module.path
            || target.path.starts_with(&format!("{}/", entry_module.path))
    })
}

/// Whether `permitted` names `module` itself: the full heading as declared,
/// or the module's last path segment, `-`/`_` normalised.
fn names_module(permitted: &str, module: &Module) -> bool {
    permitted == module.path
        || module
            .path
            .rsplit('/')
            .next()
            .is_some_and(|segment| normalise(segment) == normalise(permitted))
}

/// The declared module `permitted` names, if any, by the same rules as
/// [`names_module`]. First-declared wins when more than one module shares a
/// last segment, matching [`target_of`]'s tie-breaking.
fn resolve_module<'a>(permitted: &str, architecture: &'a Architecture) -> Option<&'a Module> {
    architecture
        .modules
        .iter()
        .find(|module| names_module(permitted, module))
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

/// The JS/TS import spec as written, verbatim (`react-dom/client`,
/// `@scope/pkg/deep`). A relative spec (starting with `.`) is skipped, and so
/// is a `node:`-prefixed spec (`node:fs`, `node:path`): it names a Node
/// builtin, never a project module, and `target_of` splits on `:` along with
/// `.` and `/`, so left unskipped its leading segment `node` could otherwise
/// resolve against a declared module whose leaf happens to be named `node`.
fn javascript_segment(spec: &str) -> Option<String> {
    if spec.is_empty() || spec.starts_with('.') || spec.starts_with("node:") {
        None
    } else {
        Some(spec.to_string())
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
    fn javascript_imports_yield_the_full_spec_and_skip_relative() {
        let text = "import x from 'react-dom/client';\nconst y = require(\"lodash\");\nimport z from './local';\nimport s from '@scope/pkg/deep';\n";
        assert_eq!(
            imports("web/index.ts", text),
            vec!["react-dom/client", "lodash", "@scope/pkg/deep"]
        );
    }

    #[test]
    fn an_unknown_extension_has_no_imports() {
        assert!(imports("README.md", "use x::y;\n").is_empty());
    }

    /// A `node:`-prefixed spec names a Node builtin (`node:fs`, `node:path`),
    /// never a project module. Left unskipped, splitting on `:` in
    /// `target_of` hands the resolver a leading segment `node` that can
    /// collide with a declared module whose leaf happens to be named `node`,
    /// exactly the false-positive class the relative-spec skip already
    /// exists for.
    #[test]
    fn a_node_builtin_spec_is_skipped() {
        assert!(imports("web/index.ts", "import fs from 'node:fs';\n").is_empty());
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
            vec!["plumb_core::manifest"],
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
        assert_eq!(found[0].import, "plumb_cli::check");
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

    fn monolith() -> architecture::Architecture {
        architecture::parse(
            "## src/pkg/documents\n\nMay depend on: (none).\n\n\
             ## src/pkg/facts\n\nMay depend on: documents.\n\n\
             ## src/pkg\n\nMay depend on: (none).\n",
        )
    }

    #[test]
    fn a_subpackage_import_resolves_to_the_deepest_matching_module() {
        let found = violations(
            &monolith(),
            "src/pkg/documents/store.py",
            "from pkg.facts import totals\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "src/pkg/facts");
        assert_eq!(found[0].import, "pkg.facts");
    }

    #[test]
    fn a_permitted_subpackage_dependency_is_not_a_violation() {
        let found = violations(
            &monolith(),
            "src/pkg/facts/totals.py",
            "from pkg.documents import domain\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_intra_subpackage_import_is_a_self_import() {
        let found = violations(
            &monolith(),
            "src/pkg/facts/totals.py",
            "from pkg.facts.helpers import round_kwh\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_longer_match_beats_a_shorter_one() {
        // `pkg.documents` must reach src/pkg/documents (score 2), not stop at
        // src/pkg (score 1), even though src/pkg is also declared.
        let found = violations(
            &monolith(),
            "src/pkg/facts/totals.py",
            "import pkg.documents.domain\n",
        );
        assert!(
            found.is_empty(),
            "resolved to src/pkg instead of the deeper module: {found:?}"
        );
    }

    #[test]
    fn crate_level_rust_resolution_is_unchanged() {
        let architecture = architecture::parse(
            "## crates/plumb-core\n\nMay depend on: (none).\n\n## crates/plumb-cli\n\nMay depend on: plumb-core.\n",
        );
        let found = violations(
            &architecture,
            "crates/plumb-cli/src/main.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
        let found = violations(
            &architecture,
            "crates/plumb-core/src/lib.rs",
            "use plumb_cli::check;\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_tie_goes_to_the_first_declared_module() {
        let architecture = architecture::parse(
            "## apps/api\n\nMay depend on: (none).\n\n## services/api\n\nMay depend on: (none).\n",
        );
        let found = violations(&architecture, "services/api/x.py", "import api.routes\n");
        // `api` matches both at score 1; first declared wins, so the target is
        // apps/api and the import from services/api is a violation against it.
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "apps/api");
    }

    fn ancestor_fixture() -> architecture::Architecture {
        architecture::parse(
            "## alembic\n\nMay depend on: ai_energy_manager.\n\n\
             ## other\n\nMay depend on: documents.\n\n\
             ## other2\n\nMay depend on: identity.\n\n\
             ## ai_energy_manager\n\nMay depend on: (none).\n\n\
             ## ai_energy_manager/identity\n\nMay depend on: (none).\n\n\
             ## documents\n\nMay depend on: (none).\n\n\
             ## facts\n\nMay depend on: (none).\n",
        )
    }

    /// A permitted entry naming an ancestor module must cover its
    /// sub-modules too: `alembic` may depend on `ai_energy_manager`, and an
    /// import resolving to the deeper `ai_energy_manager/identity` must be
    /// permitted by that same entry, not flagged for lacking its own literal
    /// listing.
    #[test]
    fn a_permitted_ancestor_covers_its_sub_module() {
        let found = violations(
            &ancestor_fixture(),
            "alembic/migrate.py",
            "from ai_energy_manager.identity.infrastructure import token\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    /// An entry naming one sub-module must not permit an unrelated sibling
    /// sub-module: `other` may depend on `documents`, which says nothing
    /// about `facts`.
    #[test]
    fn a_sibling_is_not_permitted_by_an_unrelated_entry() {
        let found = violations(
            &ancestor_fixture(),
            "other/x.py",
            "from facts import totals\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "facts");
    }

    /// An entry naming a module deeper than the target must not permit the
    /// target: `other2` may depend on `identity` (resolving to
    /// `ai_energy_manager/identity`), which does not cover the shallower
    /// parent `ai_energy_manager` itself.
    #[test]
    fn an_entry_deeper_than_the_target_does_not_permit_the_target() {
        let found = violations(
            &ancestor_fixture(),
            "other2/x.py",
            "import ai_energy_manager\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "ai_energy_manager");
    }

    /// The ancestor-permission path resolves the entry to a module and then
    /// checks a slash-suffixed prefix, so a name that merely shares a
    /// hyphen/underscore-extended string prefix must not be treated as a
    /// descendant: `src/ai_energy_manager_extra` is a sibling of
    /// `src/ai_energy_manager`, not a sub-module of it, the same
    /// component-wise-not-string-prefix rule `module_of` already enforces
    /// for file attribution.
    #[test]
    fn an_ancestor_entry_does_not_permit_a_hyphen_extended_sibling() {
        let architecture = architecture::parse(
            "## other3\n\nMay depend on: ai_energy_manager.\n\n\
             ## src/ai_energy_manager\n\nMay depend on: (none).\n\n\
             ## src/ai_energy_manager_extra\n\nMay depend on: (none).\n",
        );
        let found = violations(
            &architecture,
            "other3/x.py",
            "import ai_energy_manager_extra\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "src/ai_energy_manager_extra");
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
