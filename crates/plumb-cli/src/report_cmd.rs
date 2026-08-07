// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! `plumb report`: a last-N-commits table of volume, coupling and deletion
//! metrics — the longitudinal companion to `check --diff`'s single-diff
//! view, and Tier C instrumentation rather than a gate: no finding, no
//! exit-code effect.
//!
//! `.plumb/architecture.md` is read once for the whole run, against the
//! **current** working tree — a documented simplification, since the
//! architecture at an older commit may have declared different modules.
//! Absent, coupling has nothing to resolve against and every commit's
//! `coupling_delta` reads as `None`; the report still runs on volume alone,
//! and one line on stderr says so, once, not per commit.

use crate::git;
use anyhow::Result;
use plumb_core::architecture::{self, Architecture};
use plumb_core::boundaries;
use plumb_core::metrics::{self, CommitStats};
use std::io::Write;
use std::path::Path;

/// Runs the longitudinal report over the last `commits` first-parent commits
/// from HEAD and writes the rendered report to `output`.
pub fn run(repo: &Path, commits: usize, json: bool, mut output: impl Write) -> Result<()> {
    let architecture = read_architecture(repo);
    if architecture.is_none() {
        eprintln!(
            "plumb: no .plumb/architecture.md found, so coupling cannot be resolved; \
             every commit's coupling reads as unknown"
        );
    }

    let history = git::recent_commits(repo, commits)?;
    let mut stats = Vec::with_capacity(history.len());
    for commit in &history {
        let rows = git::numstat(repo, &commit.hash)?;
        let added: u64 = rows.iter().map(|(added, _, _)| added).sum();
        let removed: u64 = rows.iter().map(|(_, removed, _)| removed).sum();
        let coupling_delta = architecture
            .as_ref()
            .map(|architecture| coupling_delta(repo, architecture, &commit.hash, &rows));

        stats.push(CommitStats {
            hash: commit.hash.clone(),
            subject: commit.subject.clone(),
            added,
            removed,
            coupling_delta,
        });
    }

    let rendered = if json {
        metrics::render_json(&stats)
    } else {
        metrics::render_text(&stats)
    };
    // Exactly one trailing newline either way, the same normalisation
    // `check`'s report writer applies: `render_text` supplies its own and
    // `render_json` does not.
    writeln!(output, "{}", rendered.trim_end_matches('\n'))?;
    Ok(())
}

/// Reads and parses `.plumb/architecture.md`, or `None` when it is absent.
///
/// An unreadable file (permissions, not present) is read the same as an
/// absent one: this report has no gate to fail, so there is nothing to
/// report beyond the one stderr line `run` already prints.
fn read_architecture(repo: &Path) -> Option<Architecture> {
    std::fs::read_to_string(repo.join(".plumb").join("architecture.md"))
        .ok()
        .map(|text| architecture::parse(&text))
}

/// The coupling delta of one commit: the sum, over every numstat path
/// `boundaries::is_scannable` accepts, of `edges(child side) -
/// edges(parent side)`.
///
/// A side that is absent or unreadable at its reference contributes the
/// empty set — [`side`] folds `Err` and `Ok(None)` into the same empty
/// string, so a file the commit deleted (absent at the child side) or
/// introduced (absent at the parent side) contributes its edges wholly to
/// the other side's count, and a side this process cannot decode does not
/// abort a Tier C report the way `git::show_at_head` aborts the Tier A
/// manifest gate.
fn coupling_delta(
    repo: &Path,
    architecture: &Architecture,
    hash: &str,
    rows: &[(u64, u64, String)],
) -> i64 {
    let parent = format!("{hash}^");
    let mut delta: i64 = 0;
    for (_, _, path) in rows {
        if !boundaries::is_scannable(architecture, path) {
            continue;
        }
        let child_text = side(repo, hash, path);
        let parent_text = side(repo, &parent, path);
        let child_edges = boundaries::edges(architecture, path, &child_text).len() as i64;
        let parent_edges = boundaries::edges(architecture, path, &parent_text).len() as i64;
        delta += child_edges - parent_edges;
    }
    delta
}

/// The content of `path` at `reference`, or the empty string when it is
/// absent there or could not be read — the "absent side contributes the
/// empty set" rule [`coupling_delta`]'s doc comment describes.
fn side(repo: &Path, reference: &str, path: &str) -> String {
    git::show_at(repo, reference, path)
        .ok()
        .flatten()
        .unwrap_or_default()
}
