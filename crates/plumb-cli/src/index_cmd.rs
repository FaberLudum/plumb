// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! `plumb index`: regenerate `.plumb/index.md` from the tracked source files.
//!
//! The scan itself is pure — `plumb_core::index::{symbols, render}` do the
//! parsing and rendering — so this module's own job is small: list what git
//! tracks, read each one, and hand the pairs to `render`. Everything that
//! touches disk or a subprocess lives here rather than in `plumb-core`, per
//! the architecture boundary.

use crate::git;
use anyhow::{Context, Result, bail};
use plumb_core::adapters::opens_with_marker;
use plumb_core::index::{Symbol, has_scannable_extension, render, symbols};
use std::io::Write;
use std::path::Path;

/// The generated instruction file paths `plumb adapters` writes, relative to
/// the repository root.
///
/// Duplicated from `plumb_core::adapters::render` rather than called through
/// it, because `render` needs a full `Artefacts` — all four `.plumb/` files
/// read and parsed — just to answer "does a target exist on disk", and an
/// index scan should not fail (or need to guess a project name) over a
/// `.plumb/` that is present but incomplete. Kept honest by
/// `generated_file_paths_match_what_adapters_renders` below.
const GENERATED_FILE_PATHS: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    ".cursorrules",
    ".github/copilot-instructions.md",
];

/// Reads every one of `tracked`'s paths under `repo` and extracts its public
/// symbols, in the same order they were listed.
///
/// The shared core of `plumb index` and `check --diff`'s stale-index gate:
/// both need "what would a scan of this tree produce right now", and a
/// second copy of this loop in `check.rs` is exactly the kind of duplicated
/// dispatch that drifts the first time this one learns a new extension or a
/// new skip condition. `pub(crate)` rather than private, so `check.rs` can
/// call it directly — both modules are `plumb-cli`, so this is wiring within
/// a crate rather than a boundary crossing.
///
/// A tracked path that cannot be read — permission denied, a submodule
/// gitlink with nothing checked out, a symlink pointing nowhere — holds no
/// extractable symbols either way, so it is skipped rather than failing the
/// whole scan over one file nobody can open.
///
/// `has_scannable_extension` is asked before a single byte is read: it is
/// the same path-only filter `symbols`'s own dispatch already applies after
/// the fact, so a path it rejects — a lockfile, an image, any other tracked
/// file `symbols` could never extract anything from — is never opened at
/// all. This scan runs over every tracked file on every `check --diff` (the
/// index-staleness gate, not only `plumb index` itself), so skipping the
/// read here is what keeps that from reading the whole tree's binaries into
/// memory on every invocation.
pub(crate) fn scan_entries(repo: &Path, tracked: &[String]) -> Vec<(String, Vec<Symbol>)> {
    let mut entries = Vec::with_capacity(tracked.len());
    for path in tracked {
        if !has_scannable_extension(path) {
            continue;
        }
        let Ok(bytes) = std::fs::read(repo.join(path)) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        entries.push((path.clone(), symbols(path, &text)));
    }
    entries
}

/// Regenerates `.plumb/index.md` from every file git tracks.
pub fn run(repo: &Path, mut output: impl Write) -> Result<()> {
    if !repo.join(".plumb").exists() {
        bail!("no .plumb directory here; run `plumb init` first");
    }

    let tracked = git::tracked_files(repo)?;
    let entries = scan_entries(repo, &tracked);
    let symbol_count: usize = entries.iter().map(|(_, syms)| syms.len()).sum();

    let rendered = render(&entries);
    let index_path = repo.join(".plumb").join("index.md");
    std::fs::write(&index_path, &rendered)
        .with_context(|| format!("writing {}", index_path.display()))?;

    writeln!(output, "wrote .plumb/index.md")?;
    writeln!(
        output,
        "{} files scanned, {symbol_count} symbols found",
        tracked.len()
    )?;

    // Existence alone is not adoption: a hand-written `CLAUDE.md`/`AGENTS.md`
    // that predates `.plumb/` exists too, and `plumb adapters` would refuse
    // to touch it (see its overwrite guard). Reminding a user to run a
    // command that will only refuse is worse than saying nothing, so the
    // reminder fires only when at least one existing target actually opens
    // with plumb's own marker — the same `opens_with_marker` check the
    // overwrite guard and `check --diff`'s drift gate both make.
    let any_generated = GENERATED_FILE_PATHS.iter().any(|path| {
        std::fs::read(repo.join(path)).is_ok_and(|bytes| {
            let first_line = String::from_utf8_lossy(&bytes)
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            opens_with_marker(&first_line)
        })
    });
    if any_generated {
        writeln!(
            output,
            "run `plumb adapters` to refresh the instruction files"
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `scan_entries` used to read every tracked path's full bytes before
    /// asking whether the extension could ever yield a symbol, so a
    /// lockfile, an image, or any other tracked file with no scannable
    /// extension was read into memory on every scan for no reason — the
    /// `.plumb/index.md` freshness check runs this over every tracked file
    /// on every `check --diff`, not only over a diff. `has_scannable_extension`
    /// gates the read now, so a path it rejects must not appear in the
    /// returned entries at all — `render`'s own empty-symbol filter already
    /// hides such an entry from the document either way, so this cannot
    /// change what `plumb index` writes, only what it reads to get there.
    #[test]
    fn scan_entries_skips_paths_with_no_scannable_extension() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("notes.txt"), "pub fn x() {}\n").expect("write");
        std::fs::write(dir.path().join("lib.rs"), "pub fn y() {}\n").expect("write");

        let tracked = vec!["notes.txt".to_string(), "lib.rs".to_string()];
        let entries = scan_entries(dir.path(), &tracked);

        let paths: Vec<&str> = entries.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths, vec!["lib.rs"], "entries were {entries:?}");
    }

    /// If this ever drifts, the "instruction files exist" check above would
    /// start missing a real target or watching one that no longer exists.
    #[test]
    fn generated_file_paths_match_what_adapters_renders() {
        let artefacts = plumb_core::layer1::Artefacts {
            architecture: String::new(),
            conventions: String::new(),
            invariants: String::new(),
            index: String::new(),
        };
        let rendered = plumb_core::adapters::render("project", &artefacts);
        let rendered_paths: Vec<&str> = rendered.iter().map(|adapter| adapter.path).collect();
        assert_eq!(rendered_paths, GENERATED_FILE_PATHS);
    }
}
