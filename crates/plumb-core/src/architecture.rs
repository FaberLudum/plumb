// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The heading as written, trailing slash stripped: "crates/plumb-core".
    pub path: String,
    /// Permitted dependencies, as written after "May depend on:", cleaned.
    pub may_depend_on: Vec<String>,
}

pub struct Architecture {
    pub modules: Vec<Module>,
}

pub fn parse(text: &str) -> Architecture {
    let mut modules: Vec<Module> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            let path = heading.trim().trim_end_matches('/').to_string();
            if !path.is_empty() {
                modules.push(Module {
                    path,
                    may_depend_on: Vec::new(),
                });
            }
        } else if let Some(rest) = trimmed.trim_start().strip_prefix("May depend on:")
            && let Some(current) = modules.last_mut()
        {
            current.may_depend_on = parse_dependencies(rest);
        }
    }
    Architecture { modules }
}

/// The text after "May depend on:", cleaned into a list.
///
/// Prose after an em dash is commentary, not a dependency; "(none)" is the
/// explicit empty list. Trailing periods are punctuation, not path syntax.
fn parse_dependencies(rest: &str) -> Vec<String> {
    let before_dash = rest.split('—').next().unwrap_or("");
    if before_dash.trim_start().starts_with("(none)") {
        return Vec::new();
    }
    before_dash
        .split(',')
        .map(|item| item.trim().trim_end_matches('.').trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOGFOOD_SHAPE: &str = "\
# Architecture — Plumb

Prose that is not a module.

## crates/plumb-core

Responsibility: pure logic.

May depend on: (none) — plumb-core has no internal dependencies.

## crates/plumb-cli

Responsibility: binary entry point.

May depend on: plumb-core.

## docs/research/

Responsibility: the evidence base.
";

    #[test]
    fn every_heading_becomes_a_module_with_trailing_slash_stripped() {
        let architecture = parse(DOGFOOD_SHAPE);
        let paths: Vec<&str> = architecture
            .modules
            .iter()
            .map(|module| module.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["crates/plumb-core", "crates/plumb-cli", "docs/research"]
        );
    }

    #[test]
    fn none_with_trailing_prose_is_the_empty_list() {
        let architecture = parse(DOGFOOD_SHAPE);
        assert!(architecture.modules[0].may_depend_on.is_empty());
    }

    #[test]
    fn dependencies_are_trimmed_of_periods_and_whitespace() {
        let architecture = parse(DOGFOOD_SHAPE);
        assert_eq!(architecture.modules[1].may_depend_on, vec!["plumb-core"]);
    }

    #[test]
    fn a_module_without_the_line_depends_on_nothing() {
        let architecture = parse(DOGFOOD_SHAPE);
        assert!(architecture.modules[2].may_depend_on.is_empty());
    }

    #[test]
    fn a_comma_separated_list_yields_every_item() {
        let text = "## a\n\nMay depend on: b, c/d.\n\n## b\n\n## c/d\n";
        let architecture = parse(text);
        assert_eq!(architecture.modules[0].may_depend_on, vec!["b", "c/d"]);
    }

    #[test]
    fn top_level_heading_is_not_a_module() {
        let architecture = parse("# Architecture — X\n\n## m\n");
        assert_eq!(architecture.modules.len(), 1);
    }

    /// A real `May depend on:` list that wraps onto a following line used to
    /// be silently truncated: only the first physical line was read, so the
    /// entries after the line break vanished and the boundary gate warned on
    /// edges the architecture explicitly permitted.
    #[test]
    fn a_wrapped_may_depend_on_list_yields_every_entry() {
        let text = "\
## alembic

May depend on: ai, conversations, documents, facts, identity,
interventions, persistence, tenancy, ai_energy_manager.

## ai
";
        let architecture = parse(text);
        assert_eq!(
            architecture.modules[0].may_depend_on,
            vec![
                "ai",
                "conversations",
                "documents",
                "facts",
                "identity",
                "interventions",
                "persistence",
                "tenancy",
                "ai_energy_manager"
            ]
        );
    }

    /// The `(none) — prose` case can also wrap onto a second line; the
    /// joined text still starts with `(none)`, so it must still parse to
    /// the empty list rather than picking up words from the prose as
    /// dependency entries.
    #[test]
    fn a_wrapped_none_with_prose_stays_empty() {
        let text = "\
## persistence

May depend on: (none) — persistence imports no other
sub-package within this codebase.

## other
";
        let architecture = parse(text);
        assert!(architecture.modules[0].may_depend_on.is_empty());
    }

    /// Wrapping must stop at the first blank line: prose that follows a
    /// wrapped list on its own paragraph must not be swallowed into the
    /// dependency list.
    #[test]
    fn a_wrapped_list_stops_at_the_next_blank_line() {
        let text = "\
## m

May depend on: a, b,
c, d.

Responsibility: prose that must not be swallowed.

## other
";
        let architecture = parse(text);
        assert_eq!(architecture.modules.len(), 2);
        assert_eq!(
            architecture.modules[0].may_depend_on,
            vec!["a", "b", "c", "d"]
        );
        assert!(architecture.modules[1].may_depend_on.is_empty());
    }
}
