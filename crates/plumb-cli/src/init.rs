// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! `plumb init`: write the `.plumb/` working agreement.
//!
//! Greenfield and brownfield are one code path. A repository with code in it
//! changes the *defaults* — the module list is read from the directories that
//! hold source files — and the human corrects them at the prompt. Deriving
//! intent from code is an authoring problem, not a verification problem, and
//! this binary only automates what is deterministic.

use anyhow::{Context, Result, bail};
use plumb_core::layer1::{Bootstrap, greenfield};
use std::io::{BufRead, Write};
use std::path::Path;

const SOURCE_EXTENSIONS: &[&str] = &["rs", "py", "ts", "js", "cs", "go", "java"];
const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules", ".plumb", "docs", "tests"];

pub fn run(
    repo: &Path,
    defaults: bool,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<()> {
    let plumb_dir = repo.join(".plumb");
    if plumb_dir.exists() {
        bail!(
            ".plumb already exists in this repository; init writes a new working \
             agreement and will not overwrite one"
        );
    }

    let suggested = Bootstrap {
        project_name: repo
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string()),
        modules: detect_modules(repo),
        error_handling: "errors propagate to the caller; no silently swallowed failures"
            .to_string(),
        test_shape: "one behaviour per test, named as a sentence about that behaviour".to_string(),
    };

    let answers = if defaults {
        suggested
    } else {
        ask(&mut input, &mut output, suggested)?
    };

    let artefacts = greenfield(&answers);
    std::fs::create_dir_all(plumb_dir.join("decisions")).context("creating .plumb/decisions")?;
    for (name, content) in [
        ("architecture.md", &artefacts.architecture),
        ("conventions.md", &artefacts.conventions),
        ("invariants.md", &artefacts.invariants),
        ("index.md", &artefacts.index),
    ] {
        let path = plumb_dir.join(name);
        std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        writeln!(output, "wrote .plumb/{name}")?;
    }
    writeln!(
        output,
        "review .plumb/architecture.md and fill in each module's `May depend on:` line"
    )?;
    Ok(())
}

/// One prompt per answer, default shown in brackets, empty line keeps it.
fn ask(
    input: &mut impl BufRead,
    output: &mut impl Write,
    suggested: Bootstrap,
) -> Result<Bootstrap> {
    let project_name = prompt(input, output, "project name", &suggested.project_name)?;
    let modules_line = prompt(
        input,
        output,
        "modules (comma-separated)",
        &suggested.modules.join(", "),
    )?;
    let error_handling = prompt(input, output, "error handling", &suggested.error_handling)?;
    let test_shape = prompt(input, output, "test shape", &suggested.test_shape)?;

    let modules: Vec<String> = modules_line
        .split(',')
        .map(|module| module.trim().to_string())
        .filter(|module| !module.is_empty())
        .collect();

    Ok(Bootstrap {
        project_name,
        modules: if modules.is_empty() {
            suggested.modules
        } else {
            modules
        },
        error_handling,
        test_shape,
    })
}

fn prompt(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    default: &str,
) -> Result<String> {
    write!(output, "{label} [{default}]: ")?;
    output.flush()?;
    let mut line = String::new();
    input.read_line(&mut line).context("reading an answer")?;
    let answer = line.trim();
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer.to_string()
    })
}

/// Top-level directories holding at least one source file; root files count
/// as a module named `root`; an empty repository suggests `core`.
fn detect_modules(repo: &Path) -> Vec<String> {
    let mut modules = Vec::new();
    let Ok(entries) = std::fs::read_dir(repo) else {
        return vec!["core".to_string()];
    };
    let mut root_has_sources = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) && contains_source(&path) {
                modules.push(name);
            }
        } else if has_source_extension(&path) {
            root_has_sources = true;
        }
    }
    modules.sort();
    if root_has_sources {
        modules.push("root".to_string());
    }
    if modules.is_empty() {
        modules.push("core".to_string());
    }
    modules
}

fn contains_source(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if contains_source(&path) {
                return true;
            }
        } else if has_source_extension(&path) {
            return true;
        }
    }
    false
}

fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
}
