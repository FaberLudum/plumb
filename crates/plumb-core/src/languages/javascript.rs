// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use super::Language;
use crate::index::Symbol;

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

/// Extract public symbols from JavaScript/TypeScript source code.
fn javascript_symbols(text: &str) -> Vec<Symbol> {
    let mut found = Vec::new();
    for line in text.lines() {
        if let Some(symbol) = javascript_symbol(line) {
            found.push(symbol);
        }
    }
    found
}

/// Match a single line for a JavaScript/TypeScript export.
fn javascript_symbol(line: &str) -> Option<Symbol> {
    let trimmed = line.trim_start();

    // export function NAME, export async function NAME
    if let Some(rest) = trimmed.strip_prefix("export async function ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "function".to_string(),
                name,
            });
        }
    }

    if let Some(rest) = trimmed.strip_prefix("export function ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "function".to_string(),
                name,
            });
        }
    }

    // export default function NAME, export default async function NAME
    if let Some(rest) = trimmed.strip_prefix("export default async function ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "function".to_string(),
                name,
            });
        }
    }

    if let Some(rest) = trimmed.strip_prefix("export default function ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "function".to_string(),
                name,
            });
        }
    }

    // export class NAME
    if let Some(rest) = trimmed.strip_prefix("export class ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "class".to_string(),
                name,
            });
        }
    }

    // export const NAME
    if let Some(rest) = trimmed.strip_prefix("export const ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "const".to_string(),
                name,
            });
        }
    }

    // export let NAME
    if let Some(rest) = trimmed.strip_prefix("export let ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "let".to_string(),
                name,
            });
        }
    }

    // export interface NAME
    if let Some(rest) = trimmed.strip_prefix("export interface ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "interface".to_string(),
                name,
            });
        }
    }

    // export type NAME
    if let Some(rest) = trimmed.strip_prefix("export type ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "type".to_string(),
                name,
            });
        }
    }

    // export enum NAME
    if let Some(rest) = trimmed.strip_prefix("export enum ") {
        let name = crate::index::identifier(rest);
        if !name.is_empty() {
            return Some(Symbol {
                kind: "enum".to_string(),
                name,
            });
        }
    }

    None
}

pub struct JavaScript;

impl Language for JavaScript {
    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx", "mjs", "cjs"]
    }
    fn imports(&self, text: &str) -> Vec<String> {
        javascript_imports(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        javascript_symbols(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn javascript_imports_yield_the_full_spec_and_skip_relative() {
        let text = "import x from 'react-dom/client';\nconst y = require(\"lodash\");\nimport z from './local';\nimport s from '@scope/pkg/deep';\n";
        assert_eq!(
            imports("web/index.ts", text),
            vec!["react-dom/client", "lodash", "@scope/pkg/deep"]
        );
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

    #[test]
    fn javascript_exported_declarations_are_extracted() {
        let text = "\
export function start() {}\n\
export async function stop() {}\n\
export const LIMIT = 3;\n\
export default function main() {}\n\
export default async function mainAsync() {}\n\
export { rename } from './other';\n\
const internal = 1;\n";
        let found = symbols("web/index.ts", text);
        let names: Vec<&str> = found.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["start", "stop", "LIMIT", "main", "mainAsync"]);
    }
}
