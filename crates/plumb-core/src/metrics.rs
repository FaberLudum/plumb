// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Per-commit longitudinal metrics: volume, deletion ratio, coupling delta,
//! and the decay signature (volume and coupling rising together).

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommitStats {
    pub hash: String,
    pub subject: String,
    pub added: u64,
    pub removed: u64,
    /// `None` when no architecture is declared, so coupling cannot be resolved.
    pub coupling_delta: Option<i64>,
}

/// Deletion ratio: removed / added as `f64`.
///
/// Returns `None` when added is 0, never panics on division.
pub fn deletion_ratio(stats: &CommitStats) -> Option<f64> {
    if stats.added == 0 {
        None
    } else {
        Some(stats.removed as f64 / stats.added as f64)
    }
}

/// Decay signature: volume delta > 0 AND coupling delta is `Some(d)` with d > 0.
///
/// This is the pattern from taxonomy §4.5 (volume and coupling rising together),
/// surfaced but never judged: no finding, no exit-code effect.
pub fn has_decay_signature(stats: &CommitStats) -> bool {
    let volume_positive = stats.added > stats.removed;
    let coupling_positive = stats.coupling_delta.is_some_and(|d| d > 0);
    volume_positive && coupling_positive
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Summary {
    pub commits: usize,
    pub added: u64,
    pub removed: u64,
    pub mean_deletion_ratio: Option<f64>,
    pub decay_commits: Vec<String>,
}

/// Summarize the stats: count totals, compute mean deletion ratio, name decay commits.
pub fn summarize(stats: &[CommitStats]) -> Summary {
    let ratios: Vec<f64> = stats.iter().filter_map(deletion_ratio).collect();

    let mean_deletion_ratio = if ratios.is_empty() {
        None
    } else {
        Some(ratios.iter().sum::<f64>() / ratios.len() as f64)
    };

    let decay_commits = stats
        .iter()
        .filter(|s| has_decay_signature(s))
        .map(|s| s.hash.clone())
        .collect();

    Summary {
        commits: stats.len(),
        added: stats.iter().map(|s| s.added).sum(),
        removed: stats.iter().map(|s| s.removed).sum(),
        mean_deletion_ratio,
        decay_commits,
    }
}

/// Render a text report of the stats and summary.
///
/// One row per commit, followed by a summary block.
pub fn render_text(stats: &[CommitStats]) -> String {
    let mut output = String::new();

    for stat in stats {
        let hash_short = &stat.hash[..7.min(stat.hash.len())];
        let ratio_str = deletion_ratio(stat)
            .map(|r| format!("{:.2}", r))
            .unwrap_or_else(|| "-".to_string());
        let coupling_str = stat
            .coupling_delta
            .map(|d| {
                if d == 0 {
                    "0".to_string()
                } else {
                    format!("{:+}", d)
                }
            })
            .unwrap_or_else(|| "-".to_string());
        // Truncate subject to 40 characters on char boundaries, never byte slicing.
        let subject_short: String = stat.subject.chars().take(40).collect();

        output.push_str(&format!(
            "{}  +{:<6} -{:<6} ratio {}  coupling {}  {}\n",
            hash_short, stat.added, stat.removed, ratio_str, coupling_str, subject_short
        ));
    }

    let summary = summarize(stats);
    output.push('\n');
    output.push_str(&format!(
        "commits: {}, added: {}, removed: {}\n",
        summary.commits, summary.added, summary.removed
    ));
    if let Some(ratio) = summary.mean_deletion_ratio {
        output.push_str(&format!("mean deletion ratio: {:.2}\n", ratio));
    }

    // Decay line: list short hashes when K > 0.
    let decay_line = if summary.decay_commits.is_empty() {
        format!(
            "decay signature: {} of {}\n",
            summary.decay_commits.len(),
            summary.commits
        )
    } else {
        let short_hashes: Vec<&str> = summary
            .decay_commits
            .iter()
            .map(|h| &h[..7.min(h.len())])
            .collect();
        format!(
            "decay signature: {} of {} ({})\n",
            summary.decay_commits.len(),
            summary.commits,
            short_hashes.join(", ")
        )
    };
    output.push_str(&decay_line);

    output
}

/// Render a JSON report of the stats and summary.
///
/// Shape: `{"commits": [...], "summary": {...}}` with deletion_ratio injected.
pub fn render_json(stats: &[CommitStats]) -> String {
    #[derive(serde::Serialize)]
    struct CommitWithRatio {
        hash: String,
        subject: String,
        added: u64,
        removed: u64,
        deletion_ratio: Option<f64>,
        coupling_delta: Option<i64>,
    }

    let commits_with_ratio: Vec<CommitWithRatio> = stats
        .iter()
        .map(|s| CommitWithRatio {
            hash: s.hash.clone(),
            subject: s.subject.clone(),
            added: s.added,
            removed: s.removed,
            deletion_ratio: deletion_ratio(s),
            coupling_delta: s.coupling_delta,
        })
        .collect();

    #[derive(serde::Serialize)]
    struct Report {
        commits: Vec<CommitWithRatio>,
        summary: Summary,
    }

    let report = Report {
        commits: commits_with_ratio,
        summary: summarize(stats),
    };

    serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, added: u64, removed: u64, coupling: Option<i64>) -> CommitStats {
        CommitStats {
            hash: hash.to_string(),
            subject: format!("change {}", &hash[..3.min(hash.len())]),
            added,
            removed,
            coupling_delta: coupling,
        }
    }

    #[test]
    fn deletion_ratio_is_removed_over_added() {
        let ratio = deletion_ratio(&commit("a", 100, 25, None));
        assert_eq!(ratio, Some(0.25));
    }

    #[test]
    fn a_commit_that_adds_nothing_has_no_ratio() {
        assert_eq!(deletion_ratio(&commit("a", 0, 10, None)), None);
    }

    #[test]
    fn decay_needs_volume_and_coupling_rising_together() {
        assert!(has_decay_signature(&commit("a", 50, 10, Some(2))));
        assert!(!has_decay_signature(&commit("b", 50, 10, Some(0))));
        assert!(!has_decay_signature(&commit("c", 50, 10, Some(-1))));
        assert!(!has_decay_signature(&commit("d", 5, 10, Some(2))));
        assert!(!has_decay_signature(&commit("e", 50, 10, None)));
    }

    #[test]
    fn summary_counts_totals_and_names_decay_commits() {
        let stats = vec![
            commit("aaaaaaa", 100, 20, Some(3)),
            commit("bbbbbbb", 10, 40, Some(-1)),
            commit("ccccccc", 0, 5, None),
        ];
        let summary = summarize(&stats);
        assert_eq!(summary.commits, 3);
        assert_eq!(summary.added, 110);
        assert_eq!(summary.removed, 65);
        assert_eq!(summary.decay_commits, vec!["aaaaaaa"]);
        // Mean over the two commits that have a ratio: (0.2 + 4.0) / 2.
        assert_eq!(summary.mean_deletion_ratio, Some(2.1));
    }

    #[test]
    fn text_report_carries_rows_and_summary() {
        let stats = vec![
            commit("aaaaaaa1234", 100, 20, Some(3)),
            commit("bbbbbbb5678", 0, 5, None),
        ];
        let text = render_text(&stats);
        assert!(text.contains("aaaaaaa"), "{text}");
        assert!(!text.contains("aaaaaaa1234"), "hash not shortened: {text}");
        assert!(text.contains("+100"), "{text}");
        assert!(text.contains("-20"), "{text}");
        assert!(text.contains("decay signature: 1 of 2"), "{text}");
        // Decay commit short hash should be listed.
        assert!(text.contains("aaaaaaa"), "decay hash shortened: {text}");
        assert!(text.ends_with('\n'), "{text:?}");
    }

    #[test]
    fn text_report_truncates_subject_on_char_boundaries() {
        // 45 × 'é' (multi-byte character) should truncate at 40 chars without panic.
        let long_subject = "é".repeat(45);
        let stat = CommitStats {
            hash: "abc1234".to_string(),
            subject: long_subject.clone(),
            added: 10,
            removed: 5,
            coupling_delta: Some(1),
        };
        let text = render_text(&[stat]);
        // Should not panic; subject should be truncated.
        assert!(text.contains("abc1234"), "{text}");
        // Verify it doesn't contain the whole 45-char subject.
        assert!(!text.contains(&long_subject), "{text}");
    }

    #[test]
    fn coupling_zero_renders_without_sign() {
        let stat = commit("abc1234", 50, 10, Some(0));
        let text = render_text(&[stat]);
        // Some(0) should render as "0", not "+0".
        assert!(text.contains("coupling 0"), "{text}");
        assert!(!text.contains("coupling +0"), "{text}");
    }

    #[test]
    fn json_report_is_parseable_and_spells_null_ratios() {
        let stats = vec![commit("aaaaaaa", 0, 5, None)];
        let json = render_json(&stats);
        let value: serde_json::Value = serde_json::from_str(&json).expect("parses");
        assert_eq!(
            value["commits"][0]["deletion_ratio"],
            serde_json::Value::Null
        );
        assert_eq!(
            value["commits"][0]["coupling_delta"],
            serde_json::Value::Null
        );
        assert_eq!(value["summary"]["commits"], 1);
    }

    #[test]
    fn rendering_is_deterministic() {
        let stats = vec![commit("aaaaaaa", 1, 2, Some(1))];
        assert_eq!(render_text(&stats), render_text(&stats));
        assert_eq!(render_json(&stats), render_json(&stats));
    }
}
