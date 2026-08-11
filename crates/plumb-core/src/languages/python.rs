// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use super::Language;
use crate::index::Symbol;

/// Stops a dotted Python module path at `,`, whitespace or `(` — the dots
/// inside the path are kept, so the whole dotted path comes back as written.
fn python_name(rest: &str) -> &str {
    let end = rest
        .find(|c: char| c == ',' || c.is_whitespace() || c == '(')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `import NAME[.rest]` and `from NAME[.rest] import …`, kept as the full
/// dotted path. `from .` is a relative import and is skipped.
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

/// Extract public symbols from Python source code.
fn python_symbols(text: &str) -> Vec<Symbol> {
    let mut found = Vec::new();
    for line in text.lines() {
        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Only top-level definitions (no leading whitespace)
        let first_char = match line.chars().next() {
            Some(c) => c,
            None => continue,
        };

        if first_char.is_whitespace() {
            // This line is indented; skip it completely
            continue;
        }

        // Try to match "def NAME" or "async def NAME" — `async` stripped
        // first so the kind stays `def` either way.
        if let Some(rest) = line
            .strip_prefix("async ")
            .unwrap_or(line)
            .strip_prefix("def ")
        {
            if let Some(n) = extract_python_name(rest).filter(|name| !name.starts_with('_')) {
                found.push(Symbol {
                    kind: "def".to_string(),
                    name: n,
                });
            }
        } else if let Some(rest) = line.strip_prefix("class ") {
            // Try to match "class NAME"
            if let Some(n) = extract_python_name(rest).filter(|name| !name.starts_with('_')) {
                found.push(Symbol {
                    kind: "class".to_string(),
                    name: n,
                });
            }
        }
    }
    found
}

/// Extract the name from a Python def/class line: stops at `(` for def, or at whitespace/`:` for class.
fn extract_python_name(text: &str) -> Option<String> {
    let name = text
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>();
    if name.is_empty() { None } else { Some(name) }
}

pub struct Python;

impl Language for Python {
    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }
    fn imports(&self, _path: &str, text: &str) -> Vec<String> {
        python_imports(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        python_symbols(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn python_imports_yield_the_dotted_path() {
        let text = "import requests\nfrom flask import Flask\nfrom . import sibling\nimport os.path\nfrom ai_energy_manager.documents import domain\n";
        assert_eq!(
            imports("api/app.py", text),
            vec![
                "requests",
                "flask",
                "os.path",
                "ai_energy_manager.documents"
            ]
        );
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
    fn python_top_level_public_defs_and_classes_only() {
        let text = "def visible(x):\nclass Widget:\ndef _private():\n    def nested():\n";
        let found = symbols("api/app.py", text);
        let names: Vec<&str> = found.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["visible", "Widget"]);
    }

    /// `async def` at top level must be extracted the same as `def`, kind
    /// `def`, and the leading-underscore exclusion still applies to it.
    #[test]
    fn python_async_def_is_extracted_as_kind_def() {
        let text = "async def fetch():\ndef sync_fetch():\nasync def _async_helper():\n";
        let found = symbols("api/app.py", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("def", "fetch"), ("def", "sync_fetch")]);
    }
}
